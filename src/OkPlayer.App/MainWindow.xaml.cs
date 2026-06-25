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
        Closed += (_, _) => Player.SaveProgress(); // persist resume position on app close
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

    private void SetFullscreen(bool on)
    {
        if (on == _fullscreen)
            return;
        _fullscreen = on;
        AppWindow.SetPresenter(on ? AppWindowPresenterKind.FullScreen : AppWindowPresenterKind.Overlapped);
    }

    /// <summary>Resize the window's client area to the video's native pixels (1:1), clamped to ~95% of the screen.</summary>
    private void FitToVideo(int w, int h)
    {
        if (w <= 0 || h <= 0 || _fullscreen)
            return;
        var area = DisplayArea.GetFromWindowId(AppWindow.Id, DisplayAreaFallback.Nearest);
        int maxW = (int)(area.WorkArea.Width * 0.95);
        int maxH = (int)(area.WorkArea.Height * 0.95);
        double scale = Math.Min(1.0, Math.Min((double)maxW / w, (double)maxH / h));
        AppWindow.ResizeClient(new Windows.Graphics.SizeInt32((int)(w * scale), (int)(h * scale)));
    }

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
