namespace OkPlayer.Core;

/// <summary>
/// Pure geometry for aspect-locked window resizing (the Shift-resize feature). Given a proposed OUTER
/// window rectangle from a live resize drag, the dragged edge, the target client aspect ratio, and the
/// non-client insets (outer minus client, i.e. the resize borders), it returns a rectangle whose CLIENT
/// area matches the aspect. The edge(s) the user is dragging stay put; the free edge moves to compensate.
/// Kept engine- and UI-free so it can be unit-tested headlessly.
/// </summary>
public static class AspectResize
{
    // Win32 WMSZ_* edge codes (the wParam of WM_SIZING).
    public const int Left = 1, Right = 2, Top = 3, TopLeft = 4, TopRight = 5, Bottom = 6, BottomLeft = 7, BottomRight = 8;

    /// <summary>Adjust a proposed outer rect so its client area holds <paramref name="aspect"/> (width/height).
    /// <paramref name="frameW"/>/<paramref name="frameH"/> are the non-client insets (outer size − client size).
    /// Returns the original rect unchanged when the inputs can't yield a valid client box.</summary>
    public static (int Left, int Top, int Right, int Bottom) Constrain(
        int left, int top, int right, int bottom, int edge, double aspect, int frameW, int frameH)
    {
        int clientW = (right - left) - frameW;
        int clientH = (bottom - top) - frameH;
        if (aspect <= 0 || clientW <= 0 || clientH <= 0)
            return (left, top, right, bottom);

        bool horizontal = edge is Left or Right;  // dragging a vertical edge → width is the user's intent
        bool vertical = edge is Top or Bottom;     // dragging a horizontal edge → height is the user's intent

        if (horizontal)
        {
            // Width leads: derive height, grow/shrink downward (keep the top edge).
            int newClientH = (int)System.Math.Round(clientW / aspect);
            bottom = top + newClientH + frameH;
        }
        else if (vertical)
        {
            // Height leads: derive width, grow/shrink rightward (keep the left edge).
            int newClientW = (int)System.Math.Round(clientH * aspect);
            right = left + newClientW + frameW;
        }
        else
        {
            // Corner: width leads height; move the vertical edge that belongs to the dragged corner.
            int newClientH = (int)System.Math.Round(clientW / aspect);
            if (edge is TopLeft or TopRight)
                top = bottom - newClientH - frameH;
            else
                bottom = top + newClientH + frameH;
        }
        return (left, top, right, bottom);
    }
}
