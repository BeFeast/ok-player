using System;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using OkPlayer.App.ViewModels;
using Windows.System;

namespace OkPlayer.App.Views;

/// <summary>
/// The Main Player surface: the video plane + auto-hiding floating chrome (titlebar + OSC), the
/// seekbar, and the keyboard map — per the interaction handoff. Hosts the engine via MpvVideoPanel
/// and binds it through <see cref="PlayerViewModel"/>.
/// </summary>
public sealed partial class PlayerView : UserControl
{
    // A synthetic source so the surface is demonstrable without a file; real open arrives next.
    private const string DemoSource = "av://lavfi:testsrc2=size=1280x720:rate=30:duration=600";

    private readonly Microsoft.UI.Dispatching.DispatcherQueueTimer _idleTimer;
    private bool _chromeVisible = true;

    public PlayerViewModel Vm { get; } = new();

    /// <summary>F / the fullscreen button: toggle fullscreen (the window owns the presenter).</summary>
    public event EventHandler? ToggleFullscreenRequested;
    /// <summary>Esc: leave fullscreen if in it.</summary>
    public event EventHandler? ExitFullscreenRequested;

    public PlayerView()
    {
        InitializeComponent();

        _idleTimer = DispatcherQueue.CreateTimer();
        _idleTimer.Interval = TimeSpan.FromMilliseconds(2500); // canonical idle timeout
        _idleTimer.IsRepeating = false;
        _idleTimer.Tick += (_, _) => HideChrome();

        Video.EngineReady += OnEngineReady;
        Seek.SeekRequested += OnSeekRequested;
        Seek.ScrubStateChanged += scrubbing => Vm.IsScrubbing = scrubbing;
        Vm.PropertyChanged += OnVmPropertyChanged;
        Loaded += OnLoaded;
    }

    private void OnLoaded(object sender, RoutedEventArgs e)
    {
        RootGrid.Focus(FocusState.Programmatic);
        RevealChrome();
    }

    private void OnEngineReady(object? sender, EventArgs e)
    {
        if (Video.Engine is { } engine)
        {
            Vm.Attach(engine, DispatcherQueue);
            Video.Open(DemoSource);
        }
        RevealChrome();
    }

    private void OnVmPropertyChanged(object? sender, System.ComponentModel.PropertyChangedEventArgs e)
    {
        if (e.PropertyName == nameof(PlayerViewModel.IsPaused))
        {
            if (Vm.IsPaused)
                RevealChrome();     // paused: chrome stays visible indefinitely
            else
                ResetIdleTimer();   // playing: allow auto-hide
        }
    }

    private void OnSeekRequested(double fraction)
    {
        Vm.SeekToFraction(fraction);
        RevealChrome();
    }

    // ---- chrome visibility ----

    private void RevealChrome()
    {
        if (!_chromeVisible)
        {
            _chromeVisible = true;
            TitleChrome.IsHitTestVisible = true;
            BottomChrome.IsHitTestVisible = true;
            ChromeShowSb.Begin();
        }
        ResetIdleTimer();
    }

    private void HideChrome()
    {
        if (!_chromeVisible || !Vm.IsPlaying) // paused (or already hidden) keeps chrome up
            return;
        _chromeVisible = false;
        TitleChrome.IsHitTestVisible = false;
        BottomChrome.IsHitTestVisible = false;
        ChromeHideSb.Begin();
    }

    private void ResetIdleTimer()
    {
        _idleTimer.Stop();
        if (Vm.IsPlaying)
            _idleTimer.Start();
    }

    // ---- input ----

    private void OnRootPointerMoved(object sender, PointerRoutedEventArgs e) => RevealChrome();

    private void OnVideoTapped(object sender, TappedRoutedEventArgs e)
    {
        Vm.TogglePlay();
        RevealChrome();
    }

    private void OnRootKeyDown(object sender, KeyRoutedEventArgs e)
    {
        bool handled = true;
        switch (e.Key)
        {
            case VirtualKey.Space:
            case (VirtualKey)0x4B: Vm.TogglePlay(); break;        // K
            case VirtualKey.Left:  Vm.SeekRelative(-5); break;
            case VirtualKey.Right: Vm.SeekRelative(5); break;
            case (VirtualKey)0x4A: Vm.SeekRelative(-10); break;   // J
            case (VirtualKey)0x4C: Vm.SeekRelative(10); break;    // L
            case VirtualKey.Up:    Vm.NudgeVolume(5); break;
            case VirtualKey.Down:  Vm.NudgeVolume(-5); break;
            case (VirtualKey)0xBE: Vm.FrameStep(true); break;     // .
            case (VirtualKey)0xBC: Vm.FrameStep(false); break;    // ,
            case (VirtualKey)0x4D: Vm.ToggleMute(); break;        // M
            case (VirtualKey)0x46: ToggleFullscreenRequested?.Invoke(this, EventArgs.Empty); break; // F
            case (VirtualKey)0x53: Vm.TakeScreenshot(); break;    // S
            case VirtualKey.Escape: ExitFullscreenRequested?.Invoke(this, EventArgs.Empty); break;
            default: handled = false; break;
        }
        if (handled)
        {
            e.Handled = true;
            RevealChrome();
        }
    }

    // ---- OSC clicks ----

    private void OnPlayClick(object sender, RoutedEventArgs e) { Vm.TogglePlay(); RevealChrome(); }
    private void OnPrevClick(object sender, RoutedEventArgs e) { Vm.JumpChapter(-1); RevealChrome(); }
    private void OnNextClick(object sender, RoutedEventArgs e) { Vm.JumpChapter(1); RevealChrome(); }
    private void OnVolumeClick(object sender, RoutedEventArgs e) { Vm.ToggleMute(); RevealChrome(); }
    private void OnSpeedClick(object sender, RoutedEventArgs e) { Vm.CycleSpeed(); RevealChrome(); }
    private void OnScreenshotClick(object sender, RoutedEventArgs e) { Vm.TakeScreenshot(); RevealChrome(); }
    private void OnFullscreenClick(object sender, RoutedEventArgs e) => ToggleFullscreenRequested?.Invoke(this, EventArgs.Empty);
    private void OnTrailingTimeTapped(object sender, TappedRoutedEventArgs e) => Vm.ToggleTimeLabel();
}
