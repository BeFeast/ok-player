using System;
using System.Globalization;
using System.Linq;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;
using Microsoft.UI.Xaml.Input;
using OkPlayer.App.Services;
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
    private bool _chromeVisible; // starts false to match the chrome's initial Opacity=0, so the first RevealChrome actually animates it in
    private bool _panelOpen;
    private bool _syncingChapter;
    private bool _settingVolumeSlider;
    private readonly ThumbnailService _thumbs = new();
    private int _previewToken; // ignores stale async thumbnail results
    private bool _viewUnloaded; // guards against duplicate Unloaded disposing the thumbnail engine twice
    private bool _generatingChapters; // prevents overlapping chapter-thumbnail passes

    public PlayerViewModel Vm { get; } = new();

    /// <summary>The auto-hiding top bar, used as the window's title-bar drag region.</summary>
    public FrameworkElement TitleBarElement => TitleChrome;

    /// <summary>F / the fullscreen button: toggle fullscreen (the window owns the presenter).</summary>
    public event EventHandler? ToggleFullscreenRequested;
    /// <summary>Esc: leave fullscreen if in it.</summary>
    public event EventHandler? ExitFullscreenRequested;
    /// <summary>Ctrl+O / Welcome card: ask the host to show a file picker.</summary>
    public event EventHandler? OpenFileRequested;
    /// <summary>True when media is loaded (chrome is over video); false on the light welcome shell. Host adapts caption buttons.</summary>
    public event EventHandler<bool>? MediaPresenceChanged;

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
        Seek.HoverChanged += OnSeekHover;
        Seek.HoverEnded += OnSeekHoverEnded;
        Unloaded += (_, _) =>
        {
            if (_viewUnloaded) return;
            _viewUnloaded = true;
            System.Threading.Tasks.Task.Run(() => _thumbs.Dispose());
        };
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
        ApplyMediaPresence();
    }

    // Light-first shell: over Mica show the Welcome card with no video plane / no over-video chrome;
    // once media is loaded, show the video plane + reveal the OSC, and let the host darken→whiten the
    // caption buttons.
    private void ApplyMediaPresence()
    {
        bool has = Vm.HasMedia;
        WelcomeCard.Visibility = has ? Visibility.Collapsed : Visibility.Visible;
        Video.Visibility = has ? Visibility.Visible : Visibility.Collapsed;
        MediaPresenceChanged?.Invoke(this, has);
        if (has)
        {
            RevealChrome();
        }
        else
        {
            _idleTimer.Stop();
            _chromeVisible = false;
            TitleChrome.IsHitTestVisible = false;
            BottomChrome.IsHitTestVisible = false;
            ChromeHideSb.Begin();
        }
    }

    private void OnWelcomeOpenTapped(object sender, TappedRoutedEventArgs e)
        => OpenFileRequested?.Invoke(this, EventArgs.Empty);

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
            ApplyMediaPresence();
        }
    }

    private void OnSeekRequested(double fraction)
    {
        Vm.SeekToFraction(fraction);
        RevealChrome();
    }

    // ---- seek hover frame-preview ----

    private void OnSeekHover(double fraction, double xInBar)
    {
        if (!Vm.HasMedia || Vm.Duration <= 0)
        {
            OnSeekHoverEnded(); // media gone/replaced under the pointer — hide any lingering preview
            return;
        }
        double time = fraction * Vm.Duration;
        PreviewTime.Text = FormatPreviewTime(time);

        // Center the preview on the cursor (in RootGrid space), clamped to stay on-screen.
        double xInRoot = Seek.TransformToVisual(RootGrid).TransformPoint(new Windows.Foundation.Point(xInBar, 0)).X;
        double pw = PreviewPanel.ActualWidth > 0 ? PreviewPanel.ActualWidth : 180;
        double maxLeft = Math.Max(8, RootGrid.ActualWidth - pw - 8);
        PreviewTransform.X = Math.Clamp(xInRoot - pw / 2, 8, maxLeft);
        PreviewPanel.Opacity = 1;

        int token = ++_previewToken;
        _ = RequestPreviewAsync(time, token);
    }

    private async System.Threading.Tasks.Task RequestPreviewAsync(double time, int token)
    {
        try
        {
            string? path = await _thumbs.GetThumbnailAsync(time, () => token != _previewToken);
            if (path is null || token != _previewToken)
                return; // stale (cursor moved on) or no frame (e.g. audio-only) — leave the frame hidden
            PreviewImage.Source = new Microsoft.UI.Xaml.Media.Imaging.BitmapImage(new Uri(path));
            PreviewImageFrame.Visibility = Visibility.Visible;
        }
        catch { /* transient failure — keep the previous frame; never fault this fire-and-forget task */ }
    }

    private void OnSeekHoverEnded()
    {
        _previewToken++;           // discard any in-flight thumbnail so it can't flash on the next hover
        PreviewPanel.Opacity = 0;
        PreviewImageFrame.Visibility = Visibility.Collapsed; // next hover shows the timestamp first, frame when ready
    }

    private static string FormatPreviewTime(double seconds)
    {
        var ts = TimeSpan.FromSeconds(Math.Max(0, seconds));
        return ts.TotalHours >= 1
            ? $"{(int)ts.TotalHours}:{ts.Minutes:00}:{ts.Seconds:00}"
            : $"{ts.Minutes}:{ts.Seconds:00}";
    }

    // ---- chrome visibility ----

    private void RevealChrome()
    {
        if (!Vm.HasMedia)
            return; // no over-video chrome on the light welcome shell
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
        // no media / paused / panel-open / already-hidden all keep the chrome up.
        if (!_chromeVisible || !Vm.HasMedia || !Vm.IsPlaying || _panelOpen)
            return;
        // An open flyout/menu (volume, speed, subtitle, audio, overflow) renders in a popup; pointer
        // moves inside it don't reset the idle timer, so pin chrome while any popup is open.
        if (XamlRoot is not null &&
            Microsoft.UI.Xaml.Media.VisualTreeHelper.GetOpenPopupsForXamlRoot(XamlRoot).Count > 0)
        {
            _idleTimer.Start(); // re-check after the popup closes
            return;
        }
        _chromeVisible = false;
        TitleChrome.IsHitTestVisible = false;
        BottomChrome.IsHitTestVisible = false;
        ChromeHideSb.Begin();
    }

    private void ResetIdleTimer()
    {
        _idleTimer.Stop();
        if (Vm.HasMedia && Vm.IsPlaying && !_panelOpen)
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
            _ = GenerateChapterThumbnailsAsync(); // fill in chapter previews lazily
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

    private async System.Threading.Tasks.Task GenerateChapterThumbnailsAsync()
    {
        if (!Vm.HasMedia || _generatingChapters)
            return;
        _generatingChapters = true;
        try
        {
            // The thumbnail engine opens the file asynchronously; if the panel opened first, wait for
            // it to become ready (up to ~10s) so the thumbnails still fill in rather than silently no-op.
            for (int i = 0; i < 67 && !_thumbs.IsReady && _panelOpen; i++)
                await System.Threading.Tasks.Task.Delay(150);

            foreach (var ch in Vm.Chapters.ToList())
            {
                if (!_panelOpen)
                    break; // panel closed — stop generating (cached thumbs remain for next open)
                if (ch.Thumbnail is not null)
                    continue;
                string? path = await _thumbs.GetThumbnailAsync(ch.Time + 0.5, () => !_panelOpen); // a hair past the boundary
                if (path is null || !_panelOpen)
                    continue;
                ch.Thumbnail = new Microsoft.UI.Xaml.Media.Imaging.BitmapImage(new Uri(path));
            }
        }
        catch { /* transient — leave remaining thumbnails null (retried on next panel open) */ }
        finally { _generatingChapters = false; }
    }

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

    /// <summary>Load a local path or URL into the engine. Never throws to the caller — a failed open
    /// surfaces a toast (a genuine decode/format failure later arrives as an EndFile(Error) toast).</summary>
    public void OpenMedia(string pathOrUrl)
    {
        try
        {
            Video.Open(pathOrUrl); // may throw on engine-init failure — do this before mutating UI state
            Vm.OnOpening();        // load accepted: clear the prior file's playhead/duration/chapter/HasMedia
            RevealChrome();        // show the controls when a file opens (drag-drop / picker)
            _ = _thumbs.OpenAsync(pathOrUrl); // arm the seek-preview engine for this file (fire-and-forget)
        }
        catch (Exception)
        {
            ShowToast("Couldn't open this file");
        }
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
        // async void: a transient first-time DataView access can throw — never let it escape to the UI thread.
        var deferral = e.GetDeferral();
        try
        {
            var items = await e.DataView.GetStorageItemsAsync();
            var file = items.OfType<StorageFile>().FirstOrDefault();
            if (file is not null)
                OpenMedia(file.Path);
            else if (items.Count > 0)
                ShowToast("Drop a media file"); // folder / non-file drop: feedback instead of silence
        }
        catch (Exception)
        {
            ShowToast("Couldn't open dropped item");
        }
        finally
        {
            deferral.Complete();
        }
    }
}
