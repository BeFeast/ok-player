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
        Player.ToggleFullscreenRequested += (_, _) => SetFullscreen(!_fullscreen);
        Player.ExitFullscreenRequested += (_, _) => SetFullscreen(false);
        Player.OpenFileRequested += async (_, _) => await OpenFileAsync();
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
