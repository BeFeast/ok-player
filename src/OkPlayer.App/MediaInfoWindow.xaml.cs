using System;
using System.Runtime.InteropServices;
using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using OkPlayer.App.ViewModels;

namespace OkPlayer.App;

/// <summary>A movable, self-contained inspector window for the Media-info card. The card used to be a fixed,
/// centred over-video overlay that couldn't be repositioned (a tester complaint); hosting it in its own window
/// makes it draggable and placeable anywhere, like the Settings window. Hosts the shared
/// <see cref="Views.MediaInfoCard"/>: the card's Close/Done close the window, and Copy is forwarded to the host
/// via <see cref="CopyRequested"/> (the host owns the clipboard text + toast).</summary>
public sealed partial class MediaInfoWindow : Window
{
    /// <summary>Raised when the card's "Copy all" is clicked — the host copies and toasts.</summary>
    public event EventHandler? CopyRequested;

    public MediaInfoWindow(MediaInfoViewModel model)
    {
        InitializeComponent();
        Title = "Media information";
        ExtendsContentIntoTitleBar = true;
        SetTitleBar(DragRegion); // drag by the card header (the card draws its own chrome, no native title bar)
        if (AppWindow.Presenter is OverlappedPresenter p)
        {
            p.IsMaximizable = false;
            p.IsMinimizable = false;
            p.IsResizable = false; // fixed-size inspector; the card scrolls internally for long track lists
        }
        // Dark caption glyphs on the light card surface.
        var tb = AppWindow.TitleBar;
        tb.ButtonBackgroundColor = Microsoft.UI.Colors.Transparent;
        tb.ButtonInactiveBackgroundColor = Microsoft.UI.Colors.Transparent;
        tb.ButtonForegroundColor = Windows.UI.Color.FromArgb(0x8C, 0, 0, 0);
        tb.ButtonHoverForegroundColor = Windows.UI.Color.FromArgb(0xFF, 0, 0, 0);
        tb.ButtonHoverBackgroundColor = Windows.UI.Color.FromArgb(0x14, 0, 0, 0);
        tb.ButtonPressedBackgroundColor = Windows.UI.Color.FromArgb(0x22, 0, 0, 0);
        ResizeForDpi();
        Card.DataContext = model;
        Card.CloseRequested += (_, _) => Close();
        Card.CopyRequested += (_, _) => CopyRequested?.Invoke(this, EventArgs.Empty);
    }

    [DllImport("user32.dll")]
    private static extern uint GetDpiForWindow(IntPtr hwnd);

    // Size in LOGICAL pixels (the card is 660 wide); AppWindow.Resize takes physical pixels, so scale by the
    // live window DPI, mirroring SettingsWindow. The height is a sensible default; the user can resize.
    private void ResizeForDpi()
    {
        int width = 660, height = 720;
        try
        {
            IntPtr hwnd = WinRT.Interop.WindowNative.GetWindowHandle(this);
            uint dpi = GetDpiForWindow(hwnd);
            if (dpi > 0)
            {
                double scale = dpi / 96.0;
                width = (int)Math.Round(660 * scale);
                height = (int)Math.Round(720 * scale);
            }
        }
        catch { /* DPI query failed — keep the safe unscaled logical size */ }
        AppWindow.Resize(new Windows.Graphics.SizeInt32(width, height));
    }
}
