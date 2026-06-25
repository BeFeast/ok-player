using System;
using System.Threading.Tasks;
using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Windows.Storage.Pickers;

namespace OkPlayer.App;

public sealed partial class MainWindow : Window
{
    private static readonly string[] MediaExtensions =
    {
        ".mkv", ".mp4", ".m4v", ".avi", ".mov", ".webm", ".m2ts", ".ts", ".wmv", ".flv",
        ".mp3", ".flac", ".m4a", ".opus", ".wav", ".ogg", ".mka",
    };

    private bool _fullscreen;

    public MainWindow()
    {
        InitializeComponent();
        Title = "OK Player";

        // Immersive: extend content under the title-bar band so the video reaches the top edge;
        // the auto-hiding top bar is the drag region, and the caption buttons go transparent with
        // white glyphs so Mica/video shows through.
        ExtendsContentIntoTitleBar = true;
        SetTitleBar(Player.TitleBarElement);
        Player.MediaPresenceChanged += (_, hasMedia) => SetCaptionForVideo(hasMedia);
        SetCaptionForVideo(false); // start on the light welcome shell (dark caption glyphs)

        Player.ToggleFullscreenRequested += (_, _) => SetFullscreen(!_fullscreen);
        Player.ExitFullscreenRequested += (_, _) => SetFullscreen(false);
        Player.OpenFileRequested += async (_, _) => await OpenFileAsync();
        Player.FitToVideoRequested += (_, size) => FitToVideo(size.Width, size.Height);
        Player.SettingsRequested += (_, _) => OpenSettings();
        Closed += (_, _) =>
        {
            Player.SaveProgress();                 // persist resume position on app close
            App.Settings.Changed -= ApplyAppTheme; // don't keep this closed window rooted
            _settingsWindow?.Close();              // don't leave Settings as a headless window
        };
        ApplyAppTheme();
        App.Settings.Changed += ApplyAppTheme; // theme chosen in Settings applies to the player too
    }

    private void ApplyAppTheme()
    {
        if (Content is FrameworkElement root)
            root.RequestedTheme = App.Settings.Current.Theme == "Light" ? ElementTheme.Light : ElementTheme.Default;
    }

    private SettingsWindow? _settingsWindow;

    private void OpenSettings()
    {
        if (_settingsWindow is null)
        {
            _settingsWindow = new SettingsWindow();
            _settingsWindow.Closed += (_, _) => _settingsWindow = null; // single instance; clear on close
        }
        _settingsWindow.Activate();
    }

    private void SetCaptionForVideo(bool overVideo)
    {
        var tb = AppWindow.TitleBar;
        tb.ButtonBackgroundColor = Microsoft.UI.Colors.Transparent;
        tb.ButtonInactiveBackgroundColor = Microsoft.UI.Colors.Transparent;
        if (overVideo)
        {
            tb.ButtonForegroundColor = Microsoft.UI.Colors.White;
            tb.ButtonInactiveForegroundColor = Windows.UI.Color.FromArgb(0xB0, 0xFF, 0xFF, 0xFF);
            tb.ButtonHoverBackgroundColor = Windows.UI.Color.FromArgb(0x33, 0xFF, 0xFF, 0xFF);
            tb.ButtonHoverForegroundColor = Microsoft.UI.Colors.White;
            tb.ButtonPressedBackgroundColor = Windows.UI.Color.FromArgb(0x22, 0xFF, 0xFF, 0xFF);
            tb.ButtonPressedForegroundColor = Microsoft.UI.Colors.White;
        }
        else
        {
            // light Mica shell — let the caption glyphs follow the system theme (dark on light)
            tb.ButtonForegroundColor = null;
            tb.ButtonInactiveForegroundColor = null;
            tb.ButtonHoverBackgroundColor = null;
            tb.ButtonHoverForegroundColor = null;
            tb.ButtonPressedBackgroundColor = null;
            tb.ButtonPressedForegroundColor = null;
        }
    }

    private const int DWMWA_BORDER_COLOR = 34;
    private const uint DWMWA_COLOR_NONE = 0xFFFFFFFE;
    private const uint DWMWA_COLOR_DEFAULT = 0xFFFFFFFF;

    [System.Runtime.InteropServices.DllImport("dwmapi.dll")]
    private static extern int DwmSetWindowAttribute(IntPtr hwnd, int attribute, ref uint value, int size);

    private void SetFullscreen(bool on)
    {
        if (on == _fullscreen)
            return;
        _fullscreen = on;
        AppWindow.SetPresenter(on ? AppWindowPresenterKind.FullScreen : AppWindowPresenterKind.Overlapped);
        // Win11 draws a 1px window border, which shows as a white hairline over the black letterbox at the
        // top edge in full screen. Removing the DWM border colour helps; fully eliminating the non-client
        // hairline needs WM_NCCALCSIZE window subclassing (tracked separately).
        uint color = on ? DWMWA_COLOR_NONE : DWMWA_COLOR_DEFAULT;
        IntPtr hwnd = WinRT.Interop.WindowNative.GetWindowHandle(this);
        DwmSetWindowAttribute(hwnd, DWMWA_BORDER_COLOR, ref color, sizeof(uint));
    }

    /// <summary>Size the window to the video's aspect and place it fully inside the monitor: shrink to fit
    /// ~94% of the work area (never upscaling past native), then centre it so the screen edges never clip it.</summary>
    private void FitToVideo(int w, int h)
    {
        if (w <= 0 || h <= 0 || _fullscreen)
            return;
        var work = DisplayArea.GetFromWindowId(AppWindow.Id, DisplayAreaFallback.Nearest).WorkArea;
        // One scale for both axes keeps the client at the video's exact aspect (video fills it, no black
        // margins); clamp to <=1 so a small video is never blown up past its native size.
        double scale = Math.Min(1.0, Math.Min(work.Width * 0.94 / w, work.Height * 0.94 / h));
        // A single scale on both axes (Max(1,…) only guards against a zero size) keeps the exact video
        // aspect — independent per-axis minimums would distort it and reintroduce black margins.
        int cw = Math.Max(1, (int)Math.Round(w * scale));
        int ch = Math.Max(1, (int)Math.Round(h * scale));
        IntPtr hwnd = WinRT.Interop.WindowNative.GetWindowHandle(this);
        AppWindow.ResizeClient(new Windows.Graphics.SizeInt32(cw, ch));
        // ResizeClient's height arg excludes the extended title-bar band, but the real content client (what
        // mpv fills) includes it — so the client comes out taller than asked and mpv letterboxes the video.
        // Measure that delta and re-request the height minus it, so the client lands on the exact video aspect.
        if (GetClientRect(hwnd, out var rc))
        {
            int delta = (rc.Bottom - rc.Top) - ch;
            if (delta > 0 && ch - delta > 0)
                AppWindow.ResizeClient(new Windows.Graphics.SizeInt32(cw, ch - delta));
        }
        // Centre the whole window within the work area so a window sized for a large video never extends past
        // the monitor edges.
        var outer = AppWindow.Size;
        int x = work.X + Math.Max(0, (work.Width - outer.Width) / 2);
        int y = work.Y + Math.Max(0, (work.Height - outer.Height) / 2);
        AppWindow.Move(new Windows.Graphics.PointInt32(x, y));
    }

    [System.Runtime.InteropServices.DllImport("user32.dll")]
    private static extern bool GetClientRect(IntPtr hWnd, out NativeRect lpRect);

    private struct NativeRect { public int Left, Top, Right, Bottom; }

    private async Task OpenFileAsync()
    {
        var picker = new FileOpenPicker { SuggestedStartLocation = PickerLocationId.VideosLibrary };
        // Unpackaged: associate the picker with this window's HWND.
        WinRT.Interop.InitializeWithWindow.Initialize(picker, WinRT.Interop.WindowNative.GetWindowHandle(this));
        foreach (var ext in MediaExtensions)
            picker.FileTypeFilter.Add(ext);

        try
        {
            var file = await picker.PickSingleFileAsync();
            if (file is not null)
                Player.OpenMedia(file.Path); // OpenMedia is itself non-throwing
        }
        catch (Exception)
        {
            // Picker failure is non-fatal; swallow so the async-void caller can't crash the app.
        }
    }
}
