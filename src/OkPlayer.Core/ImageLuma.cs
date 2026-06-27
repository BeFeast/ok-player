using System;

namespace OkPlayer.Core;

/// <summary>Brightness scoring for poster-frame selection. Pure (no image decoding) so the math is unit-tested:
/// the app decodes a candidate PNG to BGRA8 bytes via the platform codec, this scores it, and the brightest
/// non-black frame wins. A single fixed grab often lands on a fade/black scene (studio logos, dark openings),
/// which is why "Continue watching" posters came out black.</summary>
public static class ImageLuma
{
    /// <summary>Mean perceptual luma (0–255) of a BGRA8 pixel buffer, subsampled every <paramref name="stride"/>
    /// bytes (default every 16th pixel) — far cheaper than every pixel and more than enough to tell a black/fade
    /// frame from a lit one. <paramref name="stride"/> is floored to a 4-byte (whole-pixel) multiple. Returns 0
    /// for an empty/too-short buffer.</summary>
    public static double MeanBgra(ReadOnlySpan<byte> bgra, int stride = 64)
    {
        stride -= stride % 4;     // keep sampling aligned to pixel starts (BGRA = 4 bytes/pixel)
        if (stride < 4)
            stride = 4;
        double sum = 0;
        int count = 0;
        for (int i = 0; i + 2 < bgra.Length; i += stride)
        {
            // Rec. 601 luma: green dominates perceived brightness, blue barely registers.
            sum += 0.114 * bgra[i] + 0.587 * bgra[i + 1] + 0.299 * bgra[i + 2];
            count++;
        }
        return count > 0 ? sum / count : 0;
    }
}
