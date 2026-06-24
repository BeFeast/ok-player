using System;
using System.Globalization;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;
using Microsoft.UI.Xaml.Input;
using OkPlayer.App.ViewModels;
using Windows.ApplicationModel.DataTransfer;
using Windows.Storage;
using Windows.System;

namespace OkPlayer.App.Views;

/// <summary>
/// The Main Player surface: the video plane + auto-hiding floating chrome (titlebar + OSC), the
/// seekbar, and the keyboard map — per the interaction handoff. Hosts the engine via MpvVideoPanel
/// and binds it through <see cref="PlayerViewModel"/>.
/// </summary>
public sealed partial class PlayerView : UserControl
{
    private readonly Microsoft.UI.Dispatching.DispatcherQueueTimer _idleTimer;
    private readonly Microsoft.UI.Dispatching.DispatcherQueueTimer _toastTimer;
    private bool _chromeVisible = true;
    private bool _panelOpen;
    private bool _syncingChapter;
    private bool _settingVolumeSlider;

    public PlayerViewModel Vm { get; } = new();

    /// <summary>The auto-hiding top bar, used as the window's title-bar drag region.</summary>
    public FrameworkElement TitleBarElement => TitleChrome;

    /// <summary>F / the fullscreen button: toggle fullscreen (the window owns the presenter).</summary>
    public event EventHandler? ToggleFullscreenRequested;
    /// <summary>Esc: leave fullscreen if in it.</summary>
    public event EventHandler? ExitFullscreenRequested;
    /// <summary>Ctrl+O: ask the host to show a file picker.</summary>
    public event EventHandler? OpenFileRequested;

    public PlayerView()
    {
        InitializeComponent();

        _idleTimer = DispatcherQueue.CreateTimer();
        _idleTimer.Interval = TimeSpan.FromMilliseconds(2500); // canonical idle timeout
        _idleTimer.IsRepeating = false;
        _idleTimer.Tick += (_, _) => HideChrome();

        _toastTimer = DispatcherQueue.CreateTimer();
        _toastTimer.Interval = TimeSpan.FromMilliseconds(1700);
        _toastTimer.IsRepeating = false;
        _toastTimer.Tick += (_, _) => ToastHideSb.Begin();

        Video.EngineReady += OnEngineReady;
        Seek.SeekRequested += OnSeekRequested;
        Seek.ScrubStateChanged += scrubbing => Vm.IsScrubbing = scrubbing;
        Vm.PropertyChanged += OnVmPropertyChanged;
        Vm.ToastRequested += ShowToast;
        Vm.Chapters.CollectionChanged += (_, _) => UpdateChaptersEmpty();
        PanelHideSb.Completed += (_, _) => ChaptersPanel.Visibility = Visibility.Collapsed;
        // Handle keys on the UserControl itself (a Control holds focus reliably, unlike a Grid).
        KeyDown += OnRootKeyDown;
        Loaded += OnLoaded;
    }

    private void OnLoaded(object sender, RoutedEventArgs e)
    {
        Focus(FocusState.Programmatic);
        RevealChrome();
    }

    private void OnEngineReady(object? sender, EventArgs e)
    {
        if (Video.Engine is { } engine)
            Vm.Attach(engine, DispatcherQueue);
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
        else if (e.PropertyName == nameof(PlayerViewModel.CurrentChapterIndex))
        {
            _syncingChapter = true;
            ChapterList.SelectedIndex = Vm.CurrentChapterIndex;
            _syncingChapter = false;
        }
        else if (e.PropertyName == nameof(PlayerViewModel.HasMedia))
        {
            EmptyHint.Visibility = Vm.HasMedia ? Visibility.Collapsed : Visibility.Visible;
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
        if (!_chromeVisible || !Vm.IsPlaying || _panelOpen) // paused / panel-open / already-hidden keeps chrome up
            return;
        _chromeVisible = false;
        TitleChrome.IsHitTestVisible = false;
        BottomChrome.IsHitTestVisible = false;
        ChromeHideSb.Begin();
    }

    private void ResetIdleTimer()
    {
        _idleTimer.Stop();
        if (Vm.IsPlaying && !_panelOpen)
            _idleTimer.Start();
    }

    // ---- input ----

    private void OnRootPointerMoved(object sender, PointerRoutedEventArgs e) => RevealChrome();

    // Reclaim keyboard focus when the surface (video/scrim/chrome background) is clicked, so the
    // key map (Space, S, …) keeps working. Buttons don't steal focus (AllowFocusOnInteraction=False)
    // and flyout content lives in a popup, so neither is affected.
    private void OnRootPointerPressed(object sender, PointerRoutedEventArgs e)
        => Focus(FocusState.Programmatic);

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
            case (VirtualKey)0x43: TogglePanel(); break;          // C
            case VirtualKey.Escape:
                if (_panelOpen) TogglePanel();
                else ExitFullscreenRequested?.Invoke(this, EventArgs.Empty);
                break;
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

    // ---- switchers ----

    private void OnSpeedStepClick(object sender, RoutedEventArgs e)
    {
        if (sender is Button { Tag: string tag } &&
            double.TryParse(tag, NumberStyles.Any, CultureInfo.InvariantCulture, out double speed))
            Vm.SetSpeed(speed);
        RevealChrome();
    }

    private void OnSubtitleOffClick(object sender, RoutedEventArgs e) { Vm.SetSubtitleOff(); RevealChrome(); }

    private void OnSubtitleTrackClick(object sender, RoutedEventArgs e)
    {
        if (sender is FrameworkElement { DataContext: TrackInfo track })
            Vm.SelectSubtitle(track);
        RevealChrome();
    }

    private void OnAudioTrackClick(object sender, RoutedEventArgs e)
    {
        if (sender is FrameworkElement { DataContext: TrackInfo track })
            Vm.SelectAudio(track);
        RevealChrome();
    }

    private void OnSubDelayMinus(object sender, RoutedEventArgs e) => Vm.NudgeSubDelay(-50);
    private void OnSubDelayPlus(object sender, RoutedEventArgs e) => Vm.NudgeSubDelay(50);

    // ---- chapters panel ----

    private void OnChaptersClick(object sender, RoutedEventArgs e) => TogglePanel();

    private void TogglePanel()
    {
        _panelOpen = !_panelOpen;
        if (_panelOpen)
        {
            UpdateChaptersEmpty();
            ChaptersPanel.Visibility = Visibility.Visible;
            PanelShowSb.Begin();
            RevealChrome(); // an open panel pins the chrome
        }
        else
        {
            PanelHideSb.Begin(); // the Completed handler collapses it
            ResetIdleTimer();
        }
    }

    private void OnChapterSelectionChanged(object sender, SelectionChangedEventArgs e)
    {
        if (_syncingChapter)
            return;
        if (ChapterList.SelectedItem is ChapterInfo chapter)
            Vm.SeekToChapter(chapter);
    }

    private void UpdateChaptersEmpty()
        => ChaptersEmpty.Visibility = Vm.Chapters.Count == 0 ? Visibility.Visible : Visibility.Collapsed;

    // ---- volume & overflow ----

    private void OnVolumeFlyoutOpened(object? sender, object e)
    {
        _settingVolumeSlider = true;     // suppress the echo from seeding the slider
        VolumeSlider.Value = Vm.Volume;
        _settingVolumeSlider = false;
    }

    private void OnVolumeSliderChanged(object sender, RangeBaseValueChangedEventArgs e)
    {
        if (!_settingVolumeSlider)
            Vm.SetVolume(e.NewValue);
    }

    private void OnVolumeMuteClick(object sender, RoutedEventArgs e) => Vm.ToggleMute();
    private void OnAbLoopClick(object sender, RoutedEventArgs e) => Vm.ToggleAbLoop();
    private void OnOpenFromMenu(object sender, RoutedEventArgs e) => OpenFileRequested?.Invoke(this, EventArgs.Empty);

    // ---- toasts ----

    private void ShowToast(string message)
    {
        ToastText.Text = message;
        ToastShowSb.Begin();
        _toastTimer.Stop();
        _toastTimer.Start();
    }

    // ---- open media ----

    /// <summary>Load a local path or URL into the engine.</summary>
    public void OpenMedia(string pathOrUrl)
    {
        Video.Open(pathOrUrl);
        RevealChrome(); // show the controls when a file opens (drag-drop / picker)
    }

    private void OnOpenAccelerator(KeyboardAccelerator sender, KeyboardAcceleratorInvokedEventArgs args)
    {
        OpenFileRequested?.Invoke(this, EventArgs.Empty);
        args.Handled = true;
    }

    private void OnDragOver(object sender, DragEventArgs e)
    {
        if (e.DataView.Contains(StandardDataFormats.StorageItems))
            e.AcceptedOperation = DataPackageOperation.Copy;
    }

    private async void OnDrop(object sender, DragEventArgs e)
    {
        if (!e.DataView.Contains(StandardDataFormats.StorageItems))
            return;
        var items = await e.DataView.GetStorageItemsAsync();
        if (items.Count > 0 && items[0] is StorageFile file)
            OpenMedia(file.Path);
    }
}
