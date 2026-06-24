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
        ConfigureCaptionButtons();

        Player.ToggleFullscreenRequested += (_, _) => SetFullscreen(!_fullscreen);
        Player.ExitFullscreenRequested += (_, _) => SetFullscreen(false);
        Player.OpenFileRequested += async (_, _) => await OpenFileAsync();
    }

    private void ConfigureCaptionButtons()
    {
        var titleBar = AppWindow.TitleBar;
        titleBar.ButtonBackgroundColor = Microsoft.UI.Colors.Transparent;
        titleBar.ButtonInactiveBackgroundColor = Microsoft.UI.Colors.Transparent;
        titleBar.ButtonForegroundColor = Microsoft.UI.Colors.White;
        titleBar.ButtonInactiveForegroundColor = Windows.UI.Color.FromArgb(0xB0, 0xFF, 0xFF, 0xFF);
        titleBar.ButtonHoverBackgroundColor = Windows.UI.Color.FromArgb(0x33, 0xFF, 0xFF, 0xFF);
        titleBar.ButtonHoverForegroundColor = Microsoft.UI.Colors.White;
        titleBar.ButtonPressedBackgroundColor = Windows.UI.Color.FromArgb(0x22, 0xFF, 0xFF, 0xFF);
        titleBar.ButtonPressedForegroundColor = Microsoft.UI.Colors.White;
    }

    private void SetFullscreen(bool on)
    {
        if (on == _fullscreen)
            return;
        _fullscreen = on;
        AppWindow.SetPresenter(on ? AppWindowPresenterKind.FullScreen : AppWindowPresenterKind.Overlapped);
    }

    private async Task OpenFileAsync()
    {
        var picker = new FileOpenPicker { SuggestedStartLocation = PickerLocationId.VideosLibrary };
        // Unpackaged: associate the picker with this window's HWND.
        WinRT.Interop.InitializeWithWindow.Initialize(picker, WinRT.Interop.WindowNative.GetWindowHandle(this));
        foreach (var ext in MediaExtensions)
            picker.FileTypeFilter.Add(ext);

        var file = await picker.PickSingleFileAsync();
        if (file is not null)
            Player.OpenMedia(file.Path);
    }
}
