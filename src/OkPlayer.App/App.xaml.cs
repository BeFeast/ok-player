using Microsoft.UI.Xaml;
using OkPlayer.App.Services;

namespace OkPlayer.App;

/// <summary>Application entry point. The generated Main bootstraps the Windows App SDK runtime
/// (unpackaged) before this type is constructed.</summary>
public partial class App : Application
{
    private Window? _window;

    /// <summary>The one shared user-settings instance (single source of truth across all windows).</summary>
    public static SettingsService Settings { get; } = new();

    public App()
    {
        InitializeComponent();
    }

    protected override void OnLaunched(LaunchActivatedEventArgs args)
    {
        _window = new MainWindow();
        _window.Activate();
    }
}
