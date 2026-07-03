//! Brightness scoring for poster-frame selection — port of `src/OkPlayer.Core/ImageLuma.cs`;
//! the C# suite in `tests/OkPlayer.Tests/ImageLumaTests.cs` is the executable spec. Pure (no
//! image decoding) so the math is unit-tested: the app decodes a candidate frame to BGRA8
//! bytes via the platform codec, this scores it, and the brightest non-black frame wins. A
//! single fixed grab often lands on a fade/black scene (studio logos, dark openings), which is
//! why "Continue watching" posters came out black.

/// The C# default sampling stride in bytes (≈ every 13th pixel — a *prime* step).
pub const DEFAULT_STRIDE: usize = 52;

/// Mean perceptual luma (0–255) of a BGRA8 pixel buffer, subsampled every `stride` bytes.
/// A prime pixel stride is coprime to typical frame widths, so the sampled column index
/// advances each row and the scan sweeps the whole frame rather than a fixed set of columns —
/// a divisor stride (e.g. 16px on a 320-wide frame) would only ever sample columns 0, 16,
/// 32, …, so a frame bright between them could score dark. Far cheaper than every pixel and
/// enough to tell a black/fade frame from a lit one. `stride` is floored to a 4-byte
/// (whole-pixel) multiple. Returns 0 for an empty/too-short buffer.
pub fn mean_bgra(bgra: &[u8], stride: usize) -> f64 {
    let mut stride = stride - stride % 4; // keep sampling aligned to pixel starts (BGRA = 4 bytes/pixel)
    if stride < 4 {
        stride = 4;
    }
    let mut sum = 0.0;
    let mut count = 0u64;
    let mut i = 0;
    while i + 2 < bgra.len() {
        // Rec. 601 luma: green dominates perceived brightness, blue barely registers.
        sum += 0.114 * f64::from(bgra[i])
            + 0.587 * f64::from(bgra[i + 1])
            + 0.299 * f64::from(bgra[i + 2]);
        count += 1;
        i += stride;
    }
    if count > 0 { sum / count as f64 } else { 0.0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fill(pixels: usize, b: u8, g: u8, r: u8) -> Vec<u8> {
        let mut buf = vec![0u8; pixels * 4];
        for px in buf.chunks_exact_mut(4) {
            px[0] = b;
            px[1] = g;
            px[2] = r;
            px[3] = 255;
        }
        buf
    }

    fn assert_close(actual: f64, expected: f64, tolerance: f64) {
        assert!(
            (actual - expected).abs() < tolerance,
            "expected {expected} ± {tolerance}, got {actual}"
        );
    }

    #[test]
    fn all_black_scores_zero() {
        assert_close(mean_bgra(&fill(64, 0, 0, 0), DEFAULT_STRIDE), 0.0, 1e-3);
    }

    #[test]
    fn all_white_scores_full() {
        // 0.114 + 0.587 + 0.299 = 1.0
        assert_close(
            mean_bgra(&fill(64, 255, 255, 255), DEFAULT_STRIDE),
            255.0,
            1e-3,
        );
    }

    #[test]
    fn green_dominates_luma() {
        // ≈ 149.7
        assert_close(
            mean_bgra(&fill(64, 0, 255, 0), DEFAULT_STRIDE),
            0.587 * 255.0,
            0.05,
        );
    }

    #[test]
    fn lit_frame_beats_black_frame() {
        let black = mean_bgra(&fill(64, 8, 8, 8), DEFAULT_STRIDE); // a near-black fade
        let lit = mean_bgra(&fill(64, 120, 140, 130), DEFAULT_STRIDE); // an ordinary lit scene
        assert!(
            lit > black + 60.0,
            "lit {lit:.1} should clearly beat black {black:.1}"
        );
    }

    #[test]
    fn empty_scores_zero() {
        assert_close(mean_bgra(&[], DEFAULT_STRIDE), 0.0, 1e-3);
    }

    #[test]
    fn stride_is_floored_to_whole_pixels_and_still_samples() {
        // An odd stride must not drift off pixel boundaries; a uniform buffer still scores its color.
        let mid = mean_bgra(&fill(64, 100, 100, 100), 7);
        assert_close(mid, 100.0, 1e-3);
    }

    #[test]
    fn default_stride_sweeps_columns_not_just_a_divisor_set() {
        // A 320-wide frame bright only on ODD columns. A 16px (divisor of 320) stride samples
        // columns 0,16,32,… — all even — and would score the frame black; the default prime
        // stride must drift across columns row to row and pick the brightness up.
        const W: usize = 320;
        const H: usize = 16;
        let mut buf = vec![0u8; W * H * 4];
        for y in 0..H {
            for x in (1..W).step_by(2) {
                let i = (y * W + x) * 4;
                buf[i] = 200;
                buf[i + 1] = 200;
                buf[i + 2] = 200;
                buf[i + 3] = 255;
            }
        }
        assert!(
            mean_bgra(&buf, 64) < 2.0,
            "a 16px divisor stride only sees the dark even columns"
        );
        assert!(
            mean_bgra(&buf, DEFAULT_STRIDE) > 60.0,
            "the default prime stride sweeps the odd columns too"
        );
    }
}
