namespace OkPlayer.Core;

/// <summary>
/// Pure geometry for the "Fit window to video" action. The window is first sized to the video's aspect, but
/// the OS can clamp it UP to the minimum window size (WM_GETMINMAXINFO) — most often on a small display, where
/// a video that fits narrower than the minimum width gets a window wider than the video aspect, so mpv
/// pillarboxes it with black bars on the sides (and the analogous case top/bottom). Given the video's display
/// size and the MEASURED client after the clamp, this returns the client size to re-request so the video
/// fills the (minimum-size) window: keep the clamped axis and grow the other to the video aspect. Returns null
/// when the client already matches the video aspect within a pixel (no visible bars), so the caller can skip a
/// redundant resize. Engine- and UI-free so it can be unit-tested headlessly.
/// </summary>
public static class WindowFit
{
    /// <summary>The client size (actual content area, in physical pixels) that makes the video fill the window
    /// with no letterbox, or null if the current <paramref name="clientW"/>×<paramref name="clientH"/> already
    /// matches the video aspect to within ~1px. When the window was clamped wider than the video, the height is
    /// grown to match; when clamped taller, the width is grown. The clamped (larger) axis is preserved because
    /// the OS won't let the window go below it.</summary>
    public static (int Width, int Height)? FillClient(int videoW, int videoH, int clientW, int clientH)
    {
        if (videoW <= 0 || videoH <= 0 || clientW <= 0 || clientH <= 0)
            return null;

        double videoAspect = (double)videoW / videoH;
        // Total black-bar pixels if the video keeps its aspect inside the current client.
        double sideBars = clientW - clientH * videoAspect;   // >0 → client too WIDE → bars left/right
        double vertBars = clientH - clientW / videoAspect;   // >0 → client too TALL → bars top/bottom

        if (sideBars >= 1.0) // too wide (width was clamped up) → grow height to the video aspect
        {
            int targetH = (int)System.Math.Round(clientW / videoAspect);
            return targetH > 0 && targetH != clientH ? (clientW, targetH) : null;
        }
        if (vertBars >= 1.0) // too tall (height was clamped up) → grow width to the video aspect
        {
            int targetW = (int)System.Math.Round(clientH * videoAspect);
            return targetW > 0 && targetW != clientW ? (targetW, clientH) : null;
        }
        return null; // already filled within a pixel
    }
}
