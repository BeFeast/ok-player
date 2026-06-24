using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;

namespace OkPlayer.App;

public sealed partial class MainWindow : Window
{
    private bool _fullscreen;

    public MainWindow()
    {
        InitializeComponent();
        Title = "OK Player";
        Player.ToggleFullscreenRequested += (_, _) => SetFullscreen(!_fullscreen);
        Player.ExitFullscreenRequested += (_, _) => SetFullscreen(false);
    }

    private void SetFullscreen(bool on)
    {
        if (on == _fullscreen)
            return;
        _fullscreen = on;
        AppWindow.SetPresenter(on ? AppWindowPresenterKind.FullScreen : AppWindowPresenterKind.Overlapped);
    }
}
