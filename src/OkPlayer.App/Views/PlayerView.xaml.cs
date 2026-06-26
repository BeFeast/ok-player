using System;
using System.Collections.Generic;
using System.Collections.ObjectModel;
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
    private readonly ThumbnailService _thumbs = new();
    private readonly ThumbnailService _posterThumbs = new(); // decode-only engine for continue-watching posters
    private readonly HistoryService _history = App.History; // shared instance; Settings' "Clear history" reflects here
    private readonly Microsoft.UI.Dispatching.DispatcherQueueTimer _saveTimer;
    private string? _currentPath;
    private OkPlayer.Core.Playlist? _playlist; // the opened file's folder, in natural order (null for streams)
    // Session play-modes — persist across folder changes and are applied to each new playlist.
    private bool _autoAdvance = true;          // PRD: auto-advance defaults on
    private OkPlayer.Core.RepeatMode _repeat = OkPlayer.Core.RepeatMode.Off;
    private bool _shuffle;
    private double _resumeTarget = -1; // pending resume position, applied on the first Duration after open

    /// <summary>Continue-watching cards shown on the welcome screen (bound from XAML).</summary>
    public ObservableCollection<RecentEntry> Recents { get; } = new();

    /// <summary>Bookmarks for the current file, shown in the Chapters panel (bound from XAML).</summary>
    public ObservableCollection<BookmarkEntry> Bookmarks { get; } = new();
    private int _previewToken; // ignores stale async thumbnail results
    private bool _viewUnloaded; // guards against duplicate Unloaded disposing the thumbnail engine twice
    private int _openGeneration;      // bumps per file open; a stale chapter-warm pass bails on mismatch
    private bool _chapterWarmBusy;     // a chapter-thumbnail warm pass is running (single-flight)
    private bool _chapterWarmDirty;    // the chapter set changed (or a retry is wanted) — re-walk it
    private int _timelineWarmGen = -1; // the open generation a coarse seek-preview warm is already running for
    private System.Threading.Tasks.Task<bool>? _thumbReady; // resolves when the decode engine has the current file

    public PlayerViewModel Vm { get; } = new();

    /// <summary>The auto-hiding top bar, used as the window's title-bar drag region.</summary>
    public FrameworkElement TitleBarElement => TitleChrome;

    /// <summary>Surfaces that double as window-drag handles — just the video plane. A press-drag on empty
    /// video space moves the window like the title bar; a plain click still falls through to play/pause.
    /// The welcome shell is deliberately excluded: it is a ScrollViewer full of buttons, so a drag there
    /// must scroll the recents / click a card, not move the window (the title bar still drags it).</summary>
    internal UIElement[] WindowDragSurfaces => new UIElement[] { Video };

    /// <summary>The playing video's display aspect (width/height), or 0 when nothing is loaded — drives
    /// aspect-locked window resizing (hold Shift while dragging an edge).</summary>
    public double VideoAspect => Vm.VideoWidth > 0 && Vm.VideoHeight > 0
        ? (double)Vm.VideoWidth / Vm.VideoHeight
        : 0;

    /// <summary>x:Bind helper: bool -> Visibility (for icon state toggles in XAML).</summary>
    public static Visibility VisIf(bool value) => value ? Visibility.Visible : Visibility.Collapsed;

    /// <summary>F / the fullscreen button: toggle fullscreen (the window owns the presenter).</summary>
    public event EventHandler? ToggleFullscreenRequested;
    /// <summary>Esc: leave fullscreen if in it.</summary>
    public event EventHandler? ExitFullscreenRequested;
    /// <summary>Ctrl+O / Welcome card: ask the host to show a file picker.</summary>
    public event EventHandler? OpenFileRequested;
    /// <summary>True when media is loaded (chrome is over video); false on the light welcome shell. Host adapts caption buttons.</summary>
    public event EventHandler<bool>? MediaPresenceChanged;
    /// <summary>Resize the window to the video's native pixel size (clamped to the screen). Host owns the AppWindow.</summary>
    public event EventHandler<(int Width, int Height)>? FitToVideoRequested;
    /// <summary>Open the Settings window. The host owns the single SettingsWindow instance.</summary>
    public event EventHandler? SettingsRequested;

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

        _saveTimer = DispatcherQueue.CreateTimer();
        _saveTimer.Interval = TimeSpan.FromSeconds(10); // periodically persist the resume position
        _saveTimer.IsRepeating = true;
        _saveTimer.Tick += (_, _) => SaveProgress();
        _saveTimer.Start();

        Video.EngineReady += OnEngineReady;
        Video.ScreenshotSaved += (_, _) => DispatcherQueue.TryEnqueue(() => ShowToast("Screenshot saved"));
        Video.ScreenshotForClipboard += (_, ok) => DispatcherQueue.TryEnqueue(() => OnClipboardFrameReady(ok));
        Video.SubtitleAdded += (_, ok) => DispatcherQueue.TryEnqueue(() => OnSubtitleAdded(ok));
        MediaInfoCardView.CloseRequested += (_, _) => CloseMediaInfo();
        MediaInfoCardView.CopyRequested += (_, _) => OnMediaInfoCopy();
        VolumeCtl.Vm = Vm;
        Seek.SeekRequested += OnSeekRequested;
        Seek.ScrubStateChanged += scrubbing => Vm.IsScrubbing = scrubbing;
        Seek.HoverChanged += OnSeekHover;
        Seek.HoverEnded += OnSeekHoverEnded;
        Unloaded += (_, _) =>
        {
            if (_viewUnloaded) return;
            _viewUnloaded = true;
            _saveTimer.Stop();
            _history.Changed -= OnHistoryChanged; // shared instance outlives the view — don't leak the handler
            SaveProgress();
            System.Threading.Tasks.Task.Run(() => { _thumbs.Dispose(); _posterThumbs.Dispose(); });
        };
        Vm.PropertyChanged += OnVmPropertyChanged;
        Vm.ToastRequested += ShowToast;
        // "Clear history" / retention prune can fire from the Settings window — refresh when it does.
        _history.Changed += OnHistoryChanged;
        Vm.EndReached += OnEndReached; // auto-advance the folder playlist when a file plays out
        SetPanelTab(false);            // the right panel opens on the Chapters tab by default
        Vm.Chapters.CollectionChanged += (_, _) =>
        {
            UpdateChaptersEmpty(); // seek-bar ticks bind Vm.ChapterFractions
            // Re-warm when the chapter set changes (embedded chapters arriving after user ones, edits, …).
            // Defer so a multi-step rebuild (clear + N adds) settles before we snapshot the list.
            DispatcherQueue.TryEnqueue(WarmChapterThumbnails);
        };
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
        VideoBackdrop.Visibility = has ? Visibility.Visible : Visibility.Collapsed;
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
            LoadRecents();
        }
    }

    private void OnWelcomeOpenTapped(object sender, TappedRoutedEventArgs e)
        => OpenFileRequested?.Invoke(this, EventArgs.Empty);

    private void OnWelcomeOpenClick(object sender, RoutedEventArgs e)
        => OpenFileRequested?.Invoke(this, EventArgs.Empty);

    private async void OnOpenUrlClick(object sender, RoutedEventArgs e)
    {
        var input = new TextBox { PlaceholderText = "https://…  or  smb://host/share/file.mkv" };
        var dialog = new ContentDialog
        {
            Title = "Open URL",
            Content = input,
            PrimaryButtonText = "Open",
            CloseButtonText = "Cancel",
            DefaultButton = ContentDialogButton.Primary,
            XamlRoot = XamlRoot,
        };
        try
        {
            if (await dialog.ShowAsync() == ContentDialogResult.Primary && !string.IsNullOrWhiteSpace(input.Text))
                OpenMedia(input.Text.Trim());
        }
        catch { /* another content dialog is already open — ignore the concurrent open */ }
    }

    private void OnHistoryClick(object sender, RoutedEventArgs e)
        => ShowToast("History view is coming soon");

    private void OnEngineReady(object? sender, EventArgs e)
    {
        if (Video.Engine is { } engine)
        {
            Vm.Attach(engine, DispatcherQueue);
            Vm.SetVolume(App.Settings.Current.DefaultVolume); // start at the configured default volume (Settings -> Audio)
            string device = App.Settings.Current.AudioDevice;
            if (!string.IsNullOrEmpty(device))
                Vm.RestoreAudioDevice(device); // restore the remembered device only if it still exists
        }
        if (_pendingInitialPath is { } path)
        {
            _pendingInitialPath = null;
            OpenMedia(path); // a command-line file queued before the engine was ready
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
        else if (e.PropertyName == nameof(PlayerViewModel.Duration))
        {
            TryResume(); // seek-bar chapter ticks update via the Vm.ChapterFractions binding
            WarmTimeline(); // preemptively warm a coarse grid of seek-preview frames for instant scrubbing
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
        if (!Vm.HasMedia || !double.IsFinite(Vm.Duration) || Vm.Duration <= 0)
        {
            OnSeekHoverEnded(); // media gone/replaced or duration unknown — hide any lingering preview
            return;
        }
        double time = fraction * Vm.Duration;
        PreviewTime.Text = FormatPreviewTime(time);
        string chapter = ChapterTitleAt(time);
        PreviewChapter.Text = chapter;
        PreviewChapter.Visibility = string.IsNullOrEmpty(chapter) ? Visibility.Collapsed : Visibility.Visible;

        // Center the preview on the cursor (in RootGrid space), clamped to stay on-screen.
        double xInRoot = Seek.TransformToVisual(RootGrid).TransformPoint(new Windows.Foundation.Point(xInBar, 0)).X;
        double pw = PreviewPanel.ActualWidth > 0 ? PreviewPanel.ActualWidth : 180;
        double maxLeft = Math.Max(8, RootGrid.ActualWidth - pw - 8);
        PreviewTransform.X = Math.Clamp(xInRoot - pw / 2, 8, maxLeft);
        PreviewPanel.Opacity = 1;

        // Instant placeholder: show the nearest already-cached frame immediately so scrubbing feels instant,
        // then refine to the exact second below (which keyframe-seeks only if the cursor settles). Capped
        // distance so the placeholder is never wildly off the cursor.
        string? near = _thumbs.PeekNearestCached(time, 45);
        if (near is not null)
        {
            PreviewImage.Source = new Microsoft.UI.Xaml.Media.Imaging.BitmapImage(new Uri(near));
            PreviewImageFrame.Visibility = Visibility.Visible;
        }

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

    private string ChapterTitleAt(double time)
    {
        string title = string.Empty;
        foreach (var ch in Vm.Chapters) // chapters are ordered by time; keep the last one that started
        {
            if (ch.Time <= time + 0.05)
                title = ch.Title;
            else
                break;
        }
        return title;
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
            Vm.SetSubtitleMargin(true); // lift subtitles above the OSC
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
        Vm.SetSubtitleMargin(false); // drop subtitles back toward the bottom
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

    private void OnVideoDoubleTapped(object sender, DoubleTappedRoutedEventArgs e)
    {
        // The first of the two taps already fired OnVideoTapped (a play/pause toggle); undo it so a
        // double-click toggles only full screen, leaving playback as it was.
        Vm.TogglePlay();
        ToggleFullscreenRequested?.Invoke(this, EventArgs.Empty);
        RevealChrome();
    }

    private void OnRootKeyDown(object sender, KeyRoutedEventArgs e)
    {
        bool handled = true;
        switch (e.Key)
        {
            case VirtualKey.Space:
            case (VirtualKey)0x4B: Vm.TogglePlay(); break;        // K
            case VirtualKey.Left:  Vm.SeekRelative(-App.Settings.Current.SkipStep); break;
            case VirtualKey.Right: Vm.SeekRelative(App.Settings.Current.SkipStep); break;
            case (VirtualKey)0x4A: Vm.SeekRelative(-10); break;   // J
            case (VirtualKey)0x4C: Vm.SeekRelative(10); break;    // L
            case VirtualKey.Up:    Vm.NudgeVolume(5); break;
            case VirtualKey.Down:  Vm.NudgeVolume(-5); break;
            case (VirtualKey)0xBE: Vm.FrameStep(true); break;     // .
            case (VirtualKey)0xBC: Vm.FrameStep(false); break;    // ,
            case (VirtualKey)0x4D: Vm.ToggleMute(); break;        // M
            case (VirtualKey)0x46: ToggleFullscreenRequested?.Invoke(this, EventArgs.Empty); break; // F
            case (VirtualKey)0x53: DoScreenshot(); break;         // S
            case (VirtualKey)0x49: OpenMediaInfo(); break;        // I
            case (VirtualKey)0x43: TogglePanel(); break;          // C
            case VirtualKey.PageDown: PlayNext(); break;          // next file in the folder playlist
            case VirtualKey.PageUp:   PlayPrevious(); break;      // previous file
            case VirtualKey.Escape:
                if (_mediaInfoOpen) CloseMediaInfo();
                else if (_panelOpen) TogglePanel();
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
    private void OnScreenshotClick(object sender, RoutedEventArgs e) { DoScreenshot(); RevealChrome(); }
    private void OnScreenshotWithSubsClick(object sender, RoutedEventArgs e) { Video.Screenshot(includeSubtitles: true); RevealChrome(); }
    private void OnCopyFrameClick(object sender, RoutedEventArgs e) { DoCopyFrame(); RevealChrome(); }

    /// <summary>Take a screenshot via the render panel. The grab runs while the render loop keeps driving the
    /// pipeline (vo=libmpv is fed by it), so it never freezes the app. The toast fires on the ScreenshotSaved
    /// event (i.e. on success).</summary>
    private void DoScreenshot() => Video.Screenshot();

    private int _clipboardSeq;
    // Each grab gets its own temp file (so a second grab can't overwrite the frame a pending copy hasn't read
    // yet) and the paths are dequeued in request order — mpv replies for one id arrive FIFO.
    private readonly System.Collections.Generic.Queue<string> _clipboardPending = new();

    /// <summary>Grab the current frame to a unique temp file, then copy it onto the Windows clipboard.</summary>
    private void DoCopyFrame()
    {
        string dir = System.IO.Path.Combine(System.IO.Path.GetTempPath(), "OkPlayer");
        System.IO.Directory.CreateDirectory(dir);
        string path = System.IO.Path.Combine(dir, $"clipboard-frame-{++_clipboardSeq}.png");
        // Enqueue only if the grab was actually submitted; otherwise no reply arrives and a stale path would
        // desync the queue, making every later reply copy the wrong (or a missing) frame.
        if (Video.ScreenshotToClipboard(path))
            _clipboardPending.Enqueue(path);
    }

    private void OnClipboardFrameReady(bool ok)
    {
        if (_clipboardPending.Count == 0)
            return; // one reply per submitted grab keeps this in sync; dequeue regardless of success
        string path = _clipboardPending.Dequeue();
        if (ok)
        {
            _ = CopyFrameToClipboard(path);
        }
        else
        {
            try { System.IO.File.Delete(path); } catch { /* never written */ }
            ShowToast("Couldn't copy the frame");
        }
    }

    private async System.Threading.Tasks.Task CopyFrameToClipboard(string path)
    {
        try
        {
            // Read the PNG into memory and hand the clipboard an in-memory stream, so the backing temp file can
            // be deleted immediately and a later grab overwriting/removing it can't change what gets pasted.
            byte[] bytes = await System.IO.File.ReadAllBytesAsync(path);
            var ras = new Windows.Storage.Streams.InMemoryRandomAccessStream();
            using (var writer = new Windows.Storage.Streams.DataWriter(ras))
            {
                writer.WriteBytes(bytes);
                await writer.StoreAsync();
                await writer.FlushAsync();
                writer.DetachStream();
            }
            ras.Seek(0);
            var data = new Windows.ApplicationModel.DataTransfer.DataPackage
            {
                RequestedOperation = Windows.ApplicationModel.DataTransfer.DataPackageOperation.Copy,
            };
            data.SetBitmap(Windows.Storage.Streams.RandomAccessStreamReference.CreateFromStream(ras));
            Windows.ApplicationModel.DataTransfer.Clipboard.SetContent(data);
            ShowToast("Frame copied to clipboard");
        }
        catch { ShowToast("Couldn't copy the frame"); }
        finally { try { System.IO.File.Delete(path); } catch { /* best effort */ } }
    }
    private void OnFullscreenClick(object sender, RoutedEventArgs e) => ToggleFullscreenRequested?.Invoke(this, EventArgs.Empty);

    private void OnFitToVideoClick(object sender, RoutedEventArgs e)
    {
        if (Vm.VideoWidth > 0 && Vm.VideoHeight > 0)
            FitToVideoRequested?.Invoke(this, (Vm.VideoWidth, Vm.VideoHeight));
    }

    /// <summary>Pin the window above others. The owning window applies it (it holds the AppWindow); the
    /// toggle's own IsChecked is the menu's source of truth.</summary>
    public event EventHandler<bool>? AlwaysOnTopRequested;

    private void OnAlwaysOnTopClick(object sender, RoutedEventArgs e)
    {
        bool on = AlwaysOnTopToggle.IsChecked;
        AlwaysOnTopRequested?.Invoke(this, on);
        ShowToast(on ? "Always on top" : "Always on top off");
    }

    // ---- video-plane adjustments (Video submenu) ----

    private void OnAspectClick(object sender, RoutedEventArgs e)
    {
        if (sender is FrameworkElement { Tag: string ratio })
        {
            Vm.SetAspect(ratio);
            ShowToast(ratio == "no" ? "Aspect: Auto" : $"Aspect: {ratio}");
        }
    }

    private void OnRotateClick(object sender, RoutedEventArgs e)
    {
        Vm.RotateVideo();
        ShowToast("Rotated 90°");
    }

    private void OnFillScreenClick(object sender, RoutedEventArgs e)
        => ShowToast(Vm.ToggleFillScreen() ? "Fill screen on" : "Fill screen off");

    private void OnResetVideoClick(object sender, RoutedEventArgs e)
    {
        Vm.ResetVideoAdjustments();
        ShowToast("Video reset");
    }

    /// <summary>Seek to an exact typed timecode (pillar 4: precise navigation). Accepts "90", "1:30",
    /// "1:23:45"; clamps to the file's duration and rejects invalid input.</summary>
    private async void OnGoToTimeClick(object sender, RoutedEventArgs e)
    {
        if (!Vm.HasMedia || !double.IsFinite(Vm.Duration) || Vm.Duration <= 0)
            return;
        var input = new TextBox { PlaceholderText = "e.g. 1:23:45" };
        var dialog = new ContentDialog
        {
            Title = "Go to time",
            Content = input,
            PrimaryButtonText = "Go",
            CloseButtonText = "Cancel",
            DefaultButton = ContentDialogButton.Primary,
            XamlRoot = XamlRoot,
        };
        try
        {
            if (await dialog.ShowAsync() != ContentDialogResult.Primary)
                return;
            if (OkPlayer.Core.TimeCode.Parse(input.Text) is not { } seconds)
            {
                ShowToast("Enter a time like 1:23:45");
                return;
            }
            // The file can end / fail / be replaced while the dialog is open — re-check before seeking so
            // we don't divide by a now-zero duration or claim a jump that didn't happen.
            if (!Vm.HasMedia || !double.IsFinite(Vm.Duration) || Vm.Duration <= 0)
            {
                ShowToast("No video to seek");
                return;
            }
            double target = Math.Clamp(seconds, 0, Vm.Duration);
            Vm.SeekToFraction(target / Vm.Duration);
            ShowToast($"Jumped to {FormatPreviewTime(target)}");
        }
        catch { /* another content dialog is already open — ignore the concurrent open */ }
    }

    private void OnAddBookmarkClick(object sender, RoutedEventArgs e)
    {
        if (_currentPath is { } path && Vm.HasMedia && _history.AddBookmark(path, Vm.Position))
        {
            ShowToast($"Bookmark added at {FormatPreviewTime(Vm.Position)}");
            LoadBookmarks();
        }
    }

    private void LoadBookmarks()
    {
        Bookmarks.Clear();
        if (_currentPath is { } path)
            foreach (double t in _history.GetBookmarks(path))
                Bookmarks.Add(new BookmarkEntry { Time = t, TimeText = FormatPreviewTime(t) });
        BookmarksHeader.Text = $"BOOKMARKS · {Bookmarks.Count}";
        BookmarksSection.Visibility = Bookmarks.Count > 0 ? Visibility.Visible : Visibility.Collapsed;
    }

    private void OnBookmarkJump(object sender, RoutedEventArgs e)
    {
        if (sender is FrameworkElement { DataContext: BookmarkEntry b } && Vm.Duration > 0)
            Vm.SeekToFraction(b.Time / Vm.Duration);
    }

    private void OnBookmarkDelete(object sender, RoutedEventArgs e)
    {
        if (sender is FrameworkElement { DataContext: BookmarkEntry b } && _currentPath is { } path)
        {
            _history.RemoveBookmark(path, b.Time);
            LoadBookmarks();
        }
    }

    // ---- chapter editor: user-authored chapters live in the sidecar, merged with the file's own ----

    private void OnAddChapterClick(object sender, RoutedEventArgs e)
    {
        if (_currentPath is { } path && Vm.HasMedia && Vm.Duration > 0 &&
            _history.AddUserChapter(path, Vm.Position, $"Chapter at {FormatPreviewTime(Vm.Position)}"))
        {
            ShowToast($"Chapter added at {FormatPreviewTime(Vm.Position)}");
            LoadUserChapters();
        }
    }

    private void LoadUserChapters()
    {
        var list = new List<(double, string)>();
        if (_currentPath is { } path)
            foreach (var c in _history.GetUserChapters(path))
                list.Add((c.Time, c.Title));
        Vm.SetUserChapters(list);
    }

    private void OnChapterDelete(object sender, RoutedEventArgs e)
    {
        if (sender is FrameworkElement { DataContext: ChapterInfo c } && _currentPath is { } path)
        {
            _history.RemoveUserChapter(path, c.Time);
            LoadUserChapters();
        }
    }

    private async void OnChapterRename(object sender, RoutedEventArgs e)
    {
        if (sender is not FrameworkElement { DataContext: ChapterInfo c } || _currentPath is not { } path)
            return;
        var input = new TextBox { Text = c.Title, SelectionStart = c.Title.Length };
        var dialog = new ContentDialog
        {
            Title = "Rename chapter",
            Content = input,
            PrimaryButtonText = "Save",
            CloseButtonText = "Cancel",
            DefaultButton = ContentDialogButton.Primary,
            XamlRoot = XamlRoot,
        };
        try
        {
            if (await dialog.ShowAsync() == ContentDialogResult.Primary && !string.IsNullOrWhiteSpace(input.Text))
            {
                _history.RenameUserChapter(path, c.Time, input.Text.Trim());
                LoadUserChapters();
            }
        }
        catch { /* another content dialog is already open — ignore the concurrent open */ }
    }

    /// <summary>Persist the current file's resume position. Safe to call any time (no-op without media).</summary>
    public void SaveProgress()
    {
        if (_currentPath is { } path && Vm.HasMedia && Vm.Duration > 0)
        {
            // Near-EOF counts as completed: store 0 so it neither auto-resumes nor lingers half-watched.
            double position = Vm.Position < Vm.Duration - 30 ? Vm.Position : 0;
            _history.Record(path, position, Vm.Duration);
        }
    }

    private void TryResume()
    {
        if (_resumeTarget <= 0 || Vm.Duration <= 0)
            return;
        double target = _resumeTarget;
        _resumeTarget = -1; // apply once per open
        // PRD: skip resume when < 5% watched or within 30s of the end.
        if (target > Vm.Duration * 0.05 && target < Vm.Duration - 30)
        {
            Vm.SeekToFraction(target / Vm.Duration);
            ShowToast($"Resumed at {FormatPreviewTime(target)}");
        }
    }

    private void LoadRecents()
    {
        Recents.Clear();
        foreach (var (path, rec) in _history.Recents(30))
        {
            // Continue-watching = genuinely resumable progress only (the resume thresholds: > 5% watched
            // and not within 30s of the end). Completed files (stored at position 0) are excluded.
            if (rec.Duration <= 0 || rec.Position <= rec.Duration * 0.05 || rec.Position >= rec.Duration - 30)
                continue;
            double progress = Math.Clamp(rec.Position / rec.Duration, 0, 1);
            var entry = new RecentEntry
            {
                Path = path,
                Title = string.IsNullOrEmpty(rec.Title) ? System.IO.Path.GetFileNameWithoutExtension(path) : rec.Title!,
                Meta = FormatRuntime(rec.Duration),
                TimeLeft = FormatTimeLeft(rec.Duration - rec.Position),
                Progress = progress,
                PlaceholderGradient = PosterGradient(Recents.Count),
            };
            if (!string.IsNullOrEmpty(rec.PosterPath) && System.IO.File.Exists(rec.PosterPath))
                entry.Poster = new Microsoft.UI.Xaml.Media.Imaging.BitmapImage(new Uri(rec.PosterPath!));
            Recents.Add(entry);
            if (Recents.Count >= 6)
                break;
        }
        // Two welcome layouts: recents-forward "Continue watching" when there is resumable history,
        // else the centred first-run hero.
        bool hasRecents = Recents.Count > 0;
        WelcomeVariationA.Visibility = hasRecents ? Visibility.Visible : Visibility.Collapsed;
        WelcomeFirstRun.Visibility = hasRecents ? Visibility.Collapsed : Visibility.Visible;
        _ = GeneratePostersAsync(); // fill any missing posters in the background
    }

    private void OnRecentClick(object sender, RoutedEventArgs e)
    {
        if (sender is FrameworkElement { Tag: string path })
            OpenMedia(path);
    }

    private static string FormatRuntime(double seconds)
    {
        int total = (int)seconds;
        int h = total / 3600, m = total % 3600 / 60;
        return h > 0 ? $"{h}h {m}m" : $"{m}m";
    }

    private static string FormatTimeLeft(double seconds)
    {
        int total = (int)Math.Max(0, seconds);
        int h = total / 3600, m = total % 3600 / 60;
        return h > 0 ? $"{h}h {m}m left" : $"{Math.Max(1, m)}m left";
    }

    // Rotating band-04 placeholder gradients so a card without a poster still looks designed.
    // Soft light placeholders shown while a poster frame decodes (or if a file can't produce one) — they sit
    // on the light Mica shell, so they read as clean "loading" cards rather than the old near-black blocks.
    private static readonly (string A, string B)[] PosterPalette =
    {
        ("#FFE7EEF4", "#FFCFDCE8"), ("#FFE6EEEB", "#FFCEDED7"), ("#FFEFE9E2", "#FFDBD0C4"),
        ("#FFEAEAF2", "#FFD3D3E4"), ("#FFEDEAE6", "#FFD8D0C6"),
    };

    private static Microsoft.UI.Xaml.Media.Brush PosterGradient(int index)
    {
        var (a, b) = PosterPalette[index % PosterPalette.Length];
        return new Microsoft.UI.Xaml.Media.LinearGradientBrush
        {
            StartPoint = new Windows.Foundation.Point(0.1, 0),
            EndPoint = new Windows.Foundation.Point(0.9, 1),
            GradientStops =
            {
                new Microsoft.UI.Xaml.Media.GradientStop { Color = Hex(a), Offset = 0 },
                new Microsoft.UI.Xaml.Media.GradientStop { Color = Hex(b), Offset = 1 },
            },
        };
    }

    private static Windows.UI.Color Hex(string s)
        => Windows.UI.Color.FromArgb(0xFF,
            System.Convert.ToByte(s.Substring(3, 2), 16),
            System.Convert.ToByte(s.Substring(5, 2), 16),
            System.Convert.ToByte(s.Substring(7, 2), 16));

    private bool _generatingPosters;

    private async System.Threading.Tasks.Task GeneratePostersAsync()
    {
        if (_generatingPosters)
            return;
        _generatingPosters = true;
        try
        {
            string dir = System.IO.Path.Combine(
                Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData), "OkPlayer", "posters");
            System.IO.Directory.CreateDirectory(dir);
            foreach (var entry in Recents.ToList())
            {
                if (entry.Poster is not null || Vm.HasMedia) // a poster already, or playback started — stop
                    continue;
                var rec = _history.Get(entry.Path);
                double when = rec is { Duration: > 0 } ? Math.Max(3, rec.Duration * 0.2) : 30;
                if (!await _posterThumbs.OpenAsync(entry.Path))
                    continue;
                string? frame = await _posterThumbs.GetThumbnailAsync(when);
                if (frame is null || !System.IO.File.Exists(frame))
                    continue;
                string poster = System.IO.Path.Combine(dir, PosterHash(entry.Path) + ".png");
                try { System.IO.File.Copy(frame, poster, overwrite: true); } catch { continue; }
                _history.SetPoster(entry.Path, poster);
                DispatcherQueue.TryEnqueue(() => entry.Poster = new Microsoft.UI.Xaml.Media.Imaging.BitmapImage(new Uri(poster)));
            }
        }
        catch { /* best effort */ }
        finally { _generatingPosters = false; }
    }

    private static string PosterHash(string path)
    {
        byte[] hash = System.Security.Cryptography.SHA1.HashData(System.Text.Encoding.UTF8.GetBytes(path));
        return System.Convert.ToHexString(hash);
    }

    // ---- media info (design band 13: Streams) ----

    private bool _mediaInfoOpen; // in-flight guard: one dialog / one read at a time

    private MediaInfoViewModel? _mediaInfoModel;

    private void OnMediaInfoClick(object sender, RoutedEventArgs e) => OpenMediaInfo();

    private void OnSettingsClick(object sender, RoutedEventArgs e) => SettingsRequested?.Invoke(this, EventArgs.Empty);

    /// <summary>History was cleared or pruned out-of-band (from the Settings window). Refresh the
    /// welcome recents and, if a file is open, its now-stale bookmarks/user-chapters too.</summary>
    private void OnHistoryChanged() => DispatcherQueue.TryEnqueue(() =>
    {
        LoadRecents();
        if (_currentPath is not null)
        {
            LoadBookmarks();
            LoadUserChapters();
        }
    });

    /// <summary>Toggle the incognito session: while on, nothing is written to history (no resume
    /// position, no recents). Session-scoped — resets off on restart. Existing recents stay visible.</summary>
    private void OnPrivateModeClick(object sender, RoutedEventArgs e)
    {
        _history.Private = PrivateModeToggle.IsChecked;
        ShowToast(_history.Private ? "Private session on — not saving history" : "Private session off");
    }

    /// <summary>Show (or toggle) the Media-info card. The ~40 property reads run OFF the UI thread (each is a
    /// synchronous mpv_get_property that would deadlock the UI thread against a briefly-busy core); only the
    /// finished, string-only view-model is marshalled back. Its brushes/fonts bind lazily on the UI thread.</summary>
    private async void OpenMediaInfo()
    {
        if (_mediaInfoOpen)
        {
            CloseMediaInfo();
            return;
        }
        if (!Vm.HasMedia || Video.Engine is not { } engine)
            return;
        string? path = _currentPath; // pin the file we're reading so a mid-read switch can't show stale info
        MediaInfoViewModel model;
        try { model = await System.Threading.Tasks.Task.Run(() => BuildMediaInfo(engine, path)); }
        catch { return; } // engine torn down mid-read — just don't show the card
        if (!Vm.HasMedia || _mediaInfoOpen || _currentPath != path)
            return; // the file changed, or the card was toggled, while we were reading
        _mediaInfoModel = model;
        MediaInfoCardView.DataContext = _mediaInfoModel;
        MediaInfoHost.Visibility = Visibility.Visible;
        _mediaInfoOpen = true;
    }

    private void CloseMediaInfo()
    {
        MediaInfoHost.Visibility = Visibility.Collapsed;
        _mediaInfoOpen = false;
    }

    private void OnMediaInfoCopy()
    {
        if (_mediaInfoModel is { } m && CopyMediaInfo(m))
            ShowToast("Copied");
    }

    private static string FormatBytes(long b)
        => b >= (1L << 30) ? $"{b / (double)(1L << 30):0.0} GB"
         : b >= (1L << 20) ? $"{b / (double)(1L << 20):0.0} MB"
         : $"{b / 1024.0:0} KB";

    private static MediaInfoViewModel BuildMediaInfo(OkPlayer.Mpv.MpvContext e, string? path)
    {
        var m = new MediaInfoViewModel
        {
            FileName = string.IsNullOrEmpty(path) ? (e.GetPropertyString("media-title") ?? string.Empty) : System.IO.Path.GetFileName(path),
            DirectoryPath = string.IsNullOrEmpty(path) ? string.Empty : System.IO.Path.GetDirectoryName(path) + "\\",
        };

        var file = new InfoSection { Eyebrow = "FILE · CONTAINER" };
        file.Add("Container", FriendlyContainer(e.GetPropertyString("file-format")));
        long? size = e.GetPropertyLong("file-size");
        if (size is long s) file.Add("File size", FormatBytes(s));
        double? dur = e.GetPropertyDouble("duration");
        if (dur is double d) file.Add("Duration", FormatPreviewTime(d));
        if (size is long sz && dur is double du && du > 1)
            file.Add("Overall bitrate", $"{sz * 8.0 / du / 1_000_000:0.0} Mb/s");
        m.StreamSections.Add(file);

        var video = new InfoSection { Eyebrow = "VIDEO", IdChip = SelectedTrackChip(e, "video") };
        video.Add("Codec", FriendlyVideoCodec(e.GetPropertyString("video-codec")));
        video.Add("Profile", SelectedTrackProp(e, "video", "codec-profile"));
        long? vw = e.GetPropertyLong("video-params/w") ?? e.GetPropertyLong("width");
        long? vh = e.GetPropertyLong("video-params/h") ?? e.GetPropertyLong("height");
        if (vw is long ww && vh is long hh) video.Add("Resolution", $"{ww} × {hh}");
        if ((e.GetPropertyDouble("container-fps") ?? e.GetPropertyDouble("estimated-vf-fps")) is double f && f > 0)
            video.Add("Frame rate", $"{f:0.###} fps");
        string? pix = e.GetPropertyString("video-params/pixelformat");
        video.Add("Bit depth", BitDepthFromPixfmt(pix));
        video.Add("Pixel format", pix, mono: true);
        m.StreamSections.Add(video);

        string? gamma = e.GetPropertyString("video-params/gamma");
        string? prim = e.GetPropertyString("video-params/primaries");
        if (gamma is "pq" or "hlg" || prim == "bt.2020")
        {
            var hdr = new InfoSection { Eyebrow = "HDR · COLOR", Badge = gamma == "hlg" ? "HLG" : "HDR10", BadgeAmber = true };
            hdr.Add("Primaries", prim?.ToUpperInvariant());
            hdr.Add("Transfer", gamma == "pq" ? "ST 2084 (PQ)" : gamma == "hlg" ? "HLG" : gamma);
            if ((e.GetPropertyDouble("video-params/max-luma") ?? e.GetPropertyDouble("video-params/sig-peak")) is double mx)
            {
                double mn = e.GetPropertyDouble("video-params/min-luma") ?? 0;
                hdr.Add("Mastering", $"{mn:0.####}–{mx:0} nits");
            }
            m.StreamSections.Add(hdr);
        }

        ReadTrackSections(e, m);
        BuildStats(e, m);
        return m;
    }

    private static void ReadTrackSections(OkPlayer.Mpv.MpvContext e, MediaInfoViewModel m)
    {
        long count = e.GetPropertyLong("track-list/count") ?? 0;
        int audN = 0, subN = 0;
        for (long i = 0; i < count; i++)
        {
            string? type = e.GetPropertyString($"track-list/{i}/type");
            long id = e.GetPropertyLong($"track-list/{i}/id") ?? 0;
            bool selected = e.GetPropertyBool($"track-list/{i}/selected") ?? false;
            bool external = e.GetPropertyBool($"track-list/{i}/external") ?? false;
            bool deflt = e.GetPropertyBool($"track-list/{i}/default") ?? false;
            string? title = e.GetPropertyString($"track-list/{i}/title");
            string? lang = e.GetPropertyString($"track-list/{i}/lang");
            string? codec = e.GetPropertyString($"track-list/{i}/codec");

            if (type == "audio")
            {
                audN++;
                var detail = new List<string>();
                if (e.GetPropertyString($"track-list/{i}/demux-channel-count") is { } ch) detail.Add($"{ch} ch");
                else if (e.GetPropertyString($"track-list/{i}/audio-channels") is { } ac) detail.Add(ac);
                if (e.GetPropertyLong($"track-list/{i}/demux-samplerate") is long hz && hz > 0) detail.Add($"{hz / 1000.0:0.#} kHz");
                if (e.GetPropertyLong($"track-list/{i}/demux-bitrate") is long br && br > 0) detail.Add($"{br / 1000.0:0} kb/s");
                if (!string.IsNullOrEmpty(lang)) detail.Add(lang!);
                m.AudioSection.Tracks.Add(new TrackRow
                {
                    IdChip = external ? "ext" : $"#0:{id}",
                    Title = !string.IsNullOrEmpty(title) ? title! : FriendlyAudioCodec(codec),
                    Detail = string.Join(" · ", detail),
                    Highlight = deflt || selected,
                    Badge = (deflt || selected) ? "DEFAULT" : null,
                });
            }
            else if (type == "sub")
            {
                subN++;
                var detail = new List<string>();
                if (!string.IsNullOrEmpty(lang)) detail.Add(lang!);
                if (external) detail.Add("external");
                m.SubtitleSection.Tracks.Add(new TrackRow
                {
                    IdChip = external ? "ext" : $"#0:{id}",
                    Title = !string.IsNullOrEmpty(title) ? title! : FriendlySubCodec(codec),
                    Detail = string.Join(" · ", detail),
                    Highlight = selected,
                    Badge = external ? "EXT" : selected ? "ON" : null,
                });
            }
        }
        m.AudioSection.Eyebrow = $"AUDIO · {audN} TRACK{(audN == 1 ? "" : "S")}";
        m.SubtitleSection.Eyebrow = $"SUBTITLES · {subN} TRACK{(subN == 1 ? "" : "S")}";
    }

    private static void BuildStats(OkPlayer.Mpv.MpvContext e, MediaInfoViewModel m)
    {
        var dec = new InfoSection { Eyebrow = "DECODE · RENDER" };
        string hw = e.GetPropertyString("hwdec-current") is { } h && h != "no" ? h : "software";
        dec.Add("Hardware decoder", hw, mono: true);
        dec.Add("Renderer", e.GetPropertyString("current-vo"), mono: true);
        dec.Add("Scaler", e.GetPropertyString("scale"), mono: true);
        dec.Add("Tone-mapping", e.GetPropertyString("tone-mapping"), mono: true);
        if (dec.Count > 0) m.StatsSections.Add(dec);

        var live = new InfoSection { Eyebrow = "LIVE · PERFORMANCE" };
        if ((e.GetPropertyDouble("estimated-vf-fps") ?? e.GetPropertyDouble("container-fps")) is double fps && fps > 0)
            live.Add("Current FPS", $"{fps:0.00}", accent: true);
        if (e.GetPropertyDouble("avsync") is double av)
            live.Add("A–V sync", $"{av:+0.000;−0.000;0.000} s", accent: true);
        if (e.GetPropertyLong("frame-drop-count") is long fd)
            live.Add("Frames dropped", fd.ToString("N0", CultureInfo.InvariantCulture));
        if (e.GetPropertyDouble("demuxer-cache-duration") is double cd)
            live.Add("Container cache", $"{cd:0.0} s");
        if (live.Count > 0) m.StatsSections.Add(live);

        var disp = new InfoSection { Eyebrow = "DISPLAY · OUTPUT" };
        if (e.GetPropertyLong("display-width") is long dw && e.GetPropertyLong("display-height") is long dh)
        {
            string hz = e.GetPropertyDouble("display-fps") is double dfps ? $" @ {dfps:0.##} Hz" : "";
            disp.Add("Display mode", $"{dw} × {dh}{hz}");
        }
        disp.Add("Sync mode", e.GetPropertyString("video-sync"), mono: true);
        if (disp.Count > 0) m.StatsSections.Add(disp);
    }

    private bool CopyMediaInfo(MediaInfoViewModel m)
    {
        try
        {
            var sb = new System.Text.StringBuilder();
            sb.AppendLine($"Media information — {m.FileName}");
            if (!string.IsNullOrEmpty(m.DirectoryPath)) sb.AppendLine(m.DirectoryPath);
            void Section(InfoSection sec)
            {
                sb.AppendLine();
                sb.AppendLine(sec.Eyebrow + (sec.IdChip is { } c ? $" {c}" : "") + (sec.Badge is { } b ? $" [{b}]" : ""));
                foreach (var r in sec.Left.Concat(sec.Right)) sb.AppendLine($"  {r.Label,-18}{r.Value}");
            }
            foreach (var sec in m.StreamSections) Section(sec);
            foreach (var ts in new[] { m.AudioSection, m.SubtitleSection })
            {
                if (ts.Tracks.Count == 0) continue;
                sb.AppendLine();
                sb.AppendLine(ts.Eyebrow);
                foreach (var t in ts.Tracks) sb.AppendLine($"  {t.IdChip}  {t.Title}{(t.Badge is { } bb ? $" ({bb})" : "")}\n      {t.Detail}");
            }
            foreach (var sec in m.StatsSections) Section(sec);

            var dp = new Windows.ApplicationModel.DataTransfer.DataPackage();
            dp.SetText(sb.ToString());
            Windows.ApplicationModel.DataTransfer.Clipboard.SetContent(dp);
            return true;
        }
        catch { return false; }
    }

    private static string? FriendlyContainer(string? f)
        => f switch { null or "" => null, var x when x.Contains("matroska") => "Matroska · MKV", var x when x.Contains("mp4") || x.Contains("mov") => "MP4 · MOV", var x when x.Contains("webm") => "WebM", _ => f!.Split(',')[0].ToUpperInvariant() };

    private static string? FriendlyVideoCodec(string? c)
        => c switch { null or "" => null, "hevc" => "HEVC (H.265)", "h264" => "H.264 (AVC)", "av1" => "AV1", "vp9" => "VP9", _ => c!.ToUpperInvariant() };

    private static string FriendlyAudioCodec(string? c)
        => c switch { null or "" => "Audio", "truehd" => "Dolby TrueHD", "eac3" => "E-AC-3 (Dolby Digital+)", "ac3" => "AC-3 (Dolby Digital)", "aac" => "AAC", "dts" => "DTS", "flac" => "FLAC", "opus" => "Opus", _ => c!.ToUpperInvariant() };

    private static string FriendlySubCodec(string? c)
        => c switch { null or "" => "Subtitle", "hdmv_pgs_subtitle" => "PGS (HDMV)", "subrip" => "SubRip (SRT)", "ass" => "ASS", "dvd_subtitle" => "VobSub", _ => c!.ToUpperInvariant() };

    private static string? BitDepthFromPixfmt(string? p)
    {
        if (string.IsNullOrEmpty(p)) return null;
        string chroma = p.Contains("420") ? "4:2:0" : p.Contains("422") ? "4:2:2" : p.Contains("444") ? "4:4:4" : "";
        string bits = p.Contains("p10") ? "10-bit" : p.Contains("p12") ? "12-bit" : "8-bit";
        return string.IsNullOrEmpty(chroma) ? bits : $"{bits} · {chroma}";
    }

    private static string? SelectedTrackChip(OkPlayer.Mpv.MpvContext e, string type)
    {
        long count = e.GetPropertyLong("track-list/count") ?? 0;
        for (long i = 0; i < count; i++)
            if (e.GetPropertyString($"track-list/{i}/type") == type && (e.GetPropertyBool($"track-list/{i}/selected") ?? false))
                return $"#0:{e.GetPropertyLong($"track-list/{i}/id") ?? 0}";
        return null;
    }

    private static string? SelectedTrackProp(OkPlayer.Mpv.MpvContext e, string type, string prop)
    {
        long count = e.GetPropertyLong("track-list/count") ?? 0;
        for (long i = 0; i < count; i++)
            if (e.GetPropertyString($"track-list/{i}/type") == type && (e.GetPropertyBool($"track-list/{i}/selected") ?? false))
                return e.GetPropertyString($"track-list/{i}/{prop}");
        return null;
    }
    private void OnTrailingTimeTapped(object sender, TappedRoutedEventArgs e) => Vm.ToggleTimeLabel();

    // ---- switchers ----

    private void OnSpeedStepClick(object sender, RoutedEventArgs e)
    {
        if (sender is Button { Tag: string tag } &&
            double.TryParse(tag, NumberStyles.Any, CultureInfo.InvariantCulture, out double speed))
            Vm.SetSpeed(speed);
        SpeedFlyout.Hide();   // a speed pick is a one-shot choice — dismiss the popover
        RevealChrome();
    }

    private void OnSubtitleOffClick(object sender, RoutedEventArgs e) { Vm.SetSubtitleOff(); SubtitleFlyout.Hide(); RevealChrome(); }

    /// <summary>Raised when the user asks to load an external subtitle file; the owning window shows the
    /// picker (it holds the HWND) and calls <see cref="AddSubtitle"/> back.</summary>
    public event EventHandler? AddSubtitleRequested;

    private void OnAddSubtitleFile(object sender, RoutedEventArgs e)
    {
        SubtitleFlyout.Hide();
        if (!Vm.HasMedia)
        {
            ShowToast("Open a video first");
            return;
        }
        AddSubtitleRequested?.Invoke(this, EventArgs.Empty);
    }

    private readonly Queue<string> _subtitlePending = new(); // submitted sub-add filenames, in reply order

    /// <summary>Load an external subtitle file into the running engine and select it. mpv's sub-add with
    /// "select" flips <c>sid</c>, which re-reads the track list, so the new track appears in the switcher.
    /// Toast is deferred to the reply (<see cref="OnSubtitleAdded"/>) so a file mpv can't parse reports a
    /// failure instead of a false success.</summary>
    public void AddSubtitle(string path)
    {
        if (string.IsNullOrEmpty(path))
            return;
        if (Video.AddSubtitle(path))
            _subtitlePending.Enqueue(System.IO.Path.GetFileName(path));
        else
            ShowToast("Couldn't add subtitles");
        RevealChrome();
    }

    /// <summary>mpv finished a sub-add: <paramref name="ok"/> is whether it loaded. Dequeue regardless so the
    /// one-submit-one-reply pairing stays in sync; toast the real outcome.</summary>
    private void OnSubtitleAdded(bool ok)
    {
        string name = _subtitlePending.Count > 0 ? _subtitlePending.Dequeue() : "subtitles";
        ShowToast(ok ? $"Subtitles added: {name}" : "Couldn't add subtitles");
    }

    private void OnSubtitleTrackClick(object sender, RoutedEventArgs e)
    {
        if (sender is FrameworkElement { DataContext: TrackInfo track })
            Vm.SelectSubtitle(track);
        SubtitleFlyout.Hide();   // picking a track dismisses the switcher (the Delay/Size steppers don't)
        RevealChrome();
    }

    private void OnAudioTrackClick(object sender, RoutedEventArgs e)
    {
        if (sender is FrameworkElement { DataContext: TrackInfo track })
            Vm.SelectAudio(track);
        AudioFlyout.Hide();
        RevealChrome();
    }

    private void OnAudioFlyoutOpened(object? sender, object e) => Vm.RefreshAudioDevices();

    private void OnAudioDeviceClick(object sender, RoutedEventArgs e)
    {
        if (sender is FrameworkElement { DataContext: AudioDevice dev })
        {
            Vm.SelectAudioDevice(dev.Name);
            App.Settings.Current.AudioDevice = dev.Name; // remember the choice across launches
            App.Settings.Save();
            ShowToast($"Audio output: {dev.Label}");
        }
        AudioFlyout.Hide();
        RevealChrome();
    }

    private void OnSubDelayMinus(object sender, RoutedEventArgs e) => Vm.NudgeSubDelay(-50);
    private void OnSubDelayPlus(object sender, RoutedEventArgs e) => Vm.NudgeSubDelay(50);
    private void OnSubScaleMinus(object sender, RoutedEventArgs e) => Vm.NudgeSubScale(-0.1);
    private void OnSubScalePlus(object sender, RoutedEventArgs e) => Vm.NudgeSubScale(0.1);

    // ---- chapters panel ----

    private void OnChaptersClick(object sender, RoutedEventArgs e) => TogglePanel();

    private void TogglePanel()
    {
        _panelOpen = !_panelOpen;
        if (_panelOpen)
        {
            UpdateChaptersEmpty();
            LoadBookmarks();
            ChaptersPanel.Visibility = Visibility.Visible;
            PanelShowSb.Begin();
            RevealChrome(); // an open panel pins the chrome
            WarmChapterThumbnails(); // ensure previews are filling (usually already warmed on open)
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
    {
        int n = Vm.Chapters.Count;
        ChaptersEmpty.Visibility = n == 0 ? Visibility.Visible : Visibility.Collapsed;
        ChaptersSectionHeader.Visibility = n == 0 ? Visibility.Collapsed : Visibility.Visible;
        ChaptersSectionHeader.Text = $"CHAPTERS · {n}";
    }

    /// <summary>Request a background chapter-thumbnail warm for the current file. Coalesces: marks the
    /// chapter set dirty and starts a pass only if one isn't already running. Runs whether or not the panel
    /// is open, so the cache fills preemptively and a reopened file — or a freshly opened panel — shows its
    /// chapter previews instantly. Re-fired on open, on chapter changes, and on panel open.</summary>
    private void WarmChapterThumbnails()
    {
        _chapterWarmDirty = true;
        if (_chapterWarmBusy)
            return; // a pass is running; it will pick up the dirty flag and re-walk the list
        _ = RunChapterWarmAsync(_openGeneration);
    }

    private async System.Threading.Tasks.Task RunChapterWarmAsync(int gen)
    {
        _chapterWarmBusy = true;
        try
        {
            // Wait for THIS file's decode engine to finish loading: await the open task only while it's still
            // in flight (so a warm that starts mid-switch can't grab the previous file's frames), then gate on
            // the LIVE readiness flag — never on the task's one-shot result, which would pin a transient
            // failure and lock out every later retry for this file.
            var ready = _thumbReady;
            if (ready is { IsCompleted: false })
                await ready;
            if (gen != _openGeneration)
                return; // a different file is loading
            // If the engine didn't come up (transient open failure / timeout), re-arm it once so a stuck
            // not-ready state can't blank this file's previews for the whole session. Bounded: a warm only
            // starts on a trigger (open / panel open / chapter change), so this re-arms at most once per pass.
            if (!_thumbs.IsReady && _currentPath is { } filePath)
            {
                var rearm = _thumbs.OpenAsync(filePath);
                _thumbReady = rearm;
                await rearm;
                if (gen != _openGeneration)
                    return;
            }
            if (!_thumbs.IsReady)
                return; // still not ready — give up for now (a later trigger retries)

            // Re-walk while the chapter set keeps changing (embedded chapters land after user ones; edits) or a
            // transient miss wants a retry. Bounded by a pass cap so a frame that always fails can't spin.
            for (int pass = 0; _chapterWarmDirty && gen == _openGeneration && pass < 4; pass++)
            {
                _chapterWarmDirty = false;
                bool missed = false;
                foreach (var ch in Vm.Chapters.ToList())
                {
                    if (gen != _openGeneration)
                        return; // a different file is loading — its own pass takes over
                    if (ch.Thumbnail is not null)
                        continue;
                    // a hair past the boundary so the frame is the chapter's content, not the cut
                    string? path = await _thumbs.GetThumbnailAsync(ch.Time + 0.5, () => gen != _openGeneration);
                    if (gen != _openGeneration)
                        return;
                    if (path is null) { missed = true; continue; } // transient miss — retried below
                    ch.Thumbnail = new Microsoft.UI.Xaml.Media.Imaging.BitmapImage(new Uri(path));
                }
                if (missed && gen == _openGeneration)
                {
                    _chapterWarmDirty = true;                  // retry the frames that transiently failed
                    await System.Threading.Tasks.Task.Delay(400); // brief backoff so a hard failure can't spin
                }
            }
        }
        catch { /* transient — remaining thumbnails stay null (retried on next open / panel open) */ }
        finally
        {
            _chapterWarmBusy = false;
            // A newer file arrived while we were busy and our pass bailed on the generation check — hand off
            // to it. Guarded by the generation change so this can't loop on the same file's transient misses.
            if (_chapterWarmDirty && _openGeneration != gen)
                WarmChapterThumbnails();
        }
    }

    /// <summary>Preemptively warm a coarse, bounded grid of seek-preview frames across the whole timeline so
    /// scrubbing is instant (the hover preview shows the nearest cached frame immediately — see PeekNearestCached).
    /// Background + low priority: it shares the one decode engine + gate with chapter warming and on-demand
    /// hover requests, releasing the gate between frames so a live hover interleaves. Single-flight per open.</summary>
    private void WarmTimeline()
    {
        int gen = _openGeneration;
        if (_timelineWarmGen == gen)
            return;
        _timelineWarmGen = gen;
        _ = WarmTimelineAsync(gen);
    }

    private async System.Threading.Tasks.Task WarmTimelineAsync(int gen)
    {
        try
        {
            // Wait for THIS file's decode engine (await the open task while in flight, then gate on live state).
            var ready = _thumbReady;
            if (ready is { IsCompleted: false })
                await ready;
            if (gen != _openGeneration || !_thumbs.IsReady)
                return;
            // Let playback (and any resume seek + chapter warm) settle before pulling the CPU-decode engine
            // through the whole timeline, so this background work doesn't contend with a smooth start.
            await System.Threading.Tasks.Task.Delay(3000);
            if (gen != _openGeneration || !_thumbs.IsReady)
                return;
            // Read the duration here (not at claim time): the file is loaded, so this is the real, stable value
            // for THIS media — a stale duration notification from the previous file can't drive the grid.
            double duration = Vm.Duration;
            if (!double.IsFinite(duration) || duration <= 0)
            {
                if (gen == _openGeneration)
                    _timelineWarmGen = -1; // no usable duration yet — let a later Duration update retry
                return;
            }
            if (Vm.VideoWidth <= 0)
                return; // audio-only (no video plane): the engine can't produce frames — don't burn 140 seeks
            // ~140 frames evenly across the file, clamped so a long film stays coarse and a short clip isn't dense.
            double step = Math.Clamp(duration / 140.0, 10.0, 60.0);
            int consecutiveNull = 0;
            for (double t = 0; t < duration && gen == _openGeneration; t += step)
            {
                string? f = await _thumbs.GetThumbnailAsync(t, () => gen != _openGeneration); // caches; bails if superseded
                if (f is null && ++consecutiveNull >= 3)
                    return; // the engine isn't producing frames (no video / unseekable) — stop wasting seeks
                if (f is not null)
                    consecutiveNull = 0;
            }
        }
        catch { /* best effort — a partial grid still makes scrubbing faster */ }
    }

    // ---- overflow ----  (the volume control owns its own mute / drag / scroll / type interactions)

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

    private string? _pendingInitialPath; // a launch-time file held until the engine is ready

    /// <summary>Apply the user's default subtitle size/position (Settings -> Subtitles) to the engine. Live —
    /// safe to call any time; a no-op when no engine/file is up.</summary>
    public void ApplySubtitleDefaults()
    {
        try
        {
            if (Video.Engine is { } e)
            {
                e.SetProperty("sub-scale", App.Settings.Current.SubtitleScale);
                e.SetProperty("sub-pos", (double)App.Settings.Current.SubtitlePosition);
            }
        }
        catch { /* setting a property never blocks startup/open */ }
    }

    /// <summary>Apply loudness normalization (Settings -> Audio) to the engine via an mpv audio filter.
    /// Live — safe to call any time; a no-op when no engine is up. dynaudnorm evens quiet dialogue and
    /// loud effects (night mode). Manages only our own labelled filter (<c>@okpnorm</c>) with af
    /// add/remove, so any filters the user set via raw mpv.conf (the escape hatch) are left intact.</summary>
    public void ApplyAudioDefaults()
    {
        try
        {
            if (Video.Engine is { } e)
            {
                e.CommandAsync("af", "remove", "@okpnorm"); // drop our prior instance (no-op if absent)
                if (App.Settings.Current.AudioNormalization)
                    e.CommandAsync("af", "add", "@okpnorm:dynaudnorm");
            }
        }
        catch { /* an af command never blocks startup/open */ }
    }

    /// <summary>Open a file given on the command line ("Open with"). If the engine isn't up yet, hold it
    /// and open on EngineReady.</summary>
    public void QueueInitialFile(string path)
    {
        if (Video.Engine is not null)
            OpenMedia(path);
        else
            _pendingInitialPath = path;
    }

    /// <summary>Load a local path or URL into the engine. Never throws to the caller — a failed open
    /// surfaces a toast (a genuine decode/format failure later arrives as an EndFile(Error) toast).</summary>
    public void OpenMedia(string pathOrUrl)
    {
        if (!pathOrUrl.Contains("://") &&
            (pathOrUrl.EndsWith(".m3u", StringComparison.OrdinalIgnoreCase) ||
             pathOrUrl.EndsWith(".m3u8", StringComparison.OrdinalIgnoreCase)))
        {
            OpenM3u(pathOrUrl); // a LOCAL .m3u playlist file (an HLS .m3u8 URL is a live stream — mpv plays it)
            return;
        }
        try
        {
            SaveProgress();        // persist the outgoing file's position before we replace it
            Video.Open(pathOrUrl); // may throw on engine-init failure — do this before mutating UI state
            Vm.OnOpening();        // load accepted: clear the prior file's playhead/duration/chapter/HasMedia
            _currentPath = pathOrUrl;
            _openGeneration++;     // invalidate any in-flight chapter-warm pass for the previous file
            // resume only when the user keeps that on (Settings -> Playback); applied on the first Duration
            _resumeTarget = (App.Settings.Current.ResumePlayback ? _history.Get(pathOrUrl)?.Position : null) ?? -1;
            Vm.SetSpeed(App.Settings.Current.DefaultSpeed); // every file starts at the default speed, incl. 1x
                                                            // (so a manual speed change doesn't carry over)
            ApplySubtitleDefaults(); // default sub size/position (Settings -> Subtitles)
            ApplyAudioDefaults();    // loudness normalization (Settings -> Audio)
            LoadBookmarks();       // refresh the panel's bookmarks for the new file (panel may be open)
            LoadUserChapters();    // feed the file's user-added chapters in (merge with the file's own)
            RevealChrome();        // show the controls when a file opens (drag-drop / picker)
            _thumbReady = _thumbs.OpenAsync(pathOrUrl); // arm the seek-preview engine; the warm awaits this task
            WarmChapterThumbnails();           // preemptively fill the chapter-thumbnail cache in the background
            UpdatePlaylist(pathOrUrl);        // (re)build the folder-as-playlist around this file
        }
        catch (Exception)
        {
            ShowToast("Couldn't open this file");
        }
    }

    // ---- folder-as-playlist (PRD 10.3): opening a file makes its folder the active playlist ----

    /// <summary>Keep the playlist pointed at the opened file. Navigating to a file already in the list just
    /// moves the cursor; opening a file elsewhere rebuilds the list from its folder. Streams get no list.</summary>
    /// <summary>The folder playlist projected into bound rows for the Up-Next panel (newest cursor state).</summary>
    public System.Collections.ObjectModel.ObservableCollection<ViewModels.PlaylistRow> UpNext { get; } = new();

    private void UpdatePlaylist(string pathOrUrl)
    {
        SetPlaylistFor(pathOrUrl);
        RebuildUpNext();
    }

    private void SetPlaylistFor(string pathOrUrl)
    {
        bool isUrl = pathOrUrl.Contains("://");
        string key = pathOrUrl; // URLs match by the raw string; local files by their absolute path
        if (!isUrl)
        {
            try { key = System.IO.Path.GetFullPath(pathOrUrl); } // EnumerateFiles yields absolute paths, so the
            catch { _playlist = null; return; }                  // cursor only matches if `current` is absolute too
        }
        // An entry we already have — a folder sibling, or a file/URL from a loaded .m3u — keeps the list and
        // just moves the cursor. Crucially this runs BEFORE the URL bail-out, so a URL entry of an .m3u
        // playlist doesn't wipe the playlist.
        if (_playlist?.SetCurrent(key) == true)
            return;
        if (isUrl)
        {
            _playlist = null; // a lone URL with no playlist context — single stream
            return;
        }
        try
        {
            string? dir = System.IO.Path.GetDirectoryName(key);
            if (dir is null)
            {
                _playlist = null;
                return;
            }
            var siblings = new System.Collections.Generic.List<string>();
            foreach (var f in System.IO.Directory.EnumerateFiles(dir))
                if (OkPlayer.Core.MediaFormats.IsMedia(f))
                    siblings.Add(f);
            _playlist = new OkPlayer.Core.Playlist(siblings, key) { Repeat = _repeat, Shuffle = _shuffle };
        }
        catch
        {
            _playlist = null; // unreadable folder — fall back to single-file playback
        }
    }

    /// <summary>Project the folder playlist into the Up-Next rows and refresh the panel's folder header /
    /// empty state. Called whenever the playlist or its cursor changes.</summary>
    private void RebuildUpNext()
    {
        UpNext.Clear();
        int cur = _playlist?.CurrentIndex ?? -1;
        int count = _playlist?.Count ?? 0;
        string? nextPath = _playlist?.PeekNext; // the up-next item in play order (handles shuffle + wrap)
        for (int i = 0; i < count; i++)
        {
            string p = _playlist!.Items[i];
            UpNext.Add(new ViewModels.PlaylistRow
            {
                Path = p,
                Title = System.IO.Path.GetFileNameWithoutExtension(p),
                IsCurrent = i == cur,
                IsNext = string.Equals(p, nextPath, StringComparison.OrdinalIgnoreCase),
                IsWatched = _history.Get(p) is { Position: > 60 }, // seen at least a minute in
            });
        }
        bool hasFolder = count > 1;
        UpNextFolderHeader.Text = hasFolder ? $"FROM THIS FOLDER · {count}" : string.Empty;
        UpNextFolderHeader.Visibility = hasFolder ? Visibility.Visible : Visibility.Collapsed;
        UpNextList.Visibility = hasFolder ? Visibility.Visible : Visibility.Collapsed;
        UpNextEmpty.Visibility = hasFolder ? Visibility.Collapsed : Visibility.Visible;
        RefreshModeButtons();
    }

    /// <summary>Reflect the active play-modes on the footer toggle buttons (glyph + accent vs. dimmed).</summary>
    private void RefreshModeButtons()
    {
        var accent = PanelBrush("OkAccentTextBrush", Windows.UI.Color.FromArgb(0xFF, 0x28, 0xB3, 0xAA));
        var dim = PanelBrush("OkTextSecondaryBrush", Windows.UI.Color.FromArgb(0xB3, 0xFF, 0xFF, 0xFF));
        var tint = new Microsoft.UI.Xaml.Media.SolidColorBrush(Windows.UI.Color.FromArgb(0x24, 0x10, 0x93, 0x8A));

        var rep = _playlist?.Repeat ?? _repeat;
        RepeatIcon.Glyph = rep == OkPlayer.Core.RepeatMode.One ? "" : ""; // RepeatOne vs RepeatAll
        RepeatIcon.Foreground = rep == OkPlayer.Core.RepeatMode.Off ? dim : accent;
        RepeatButton.Background = rep == OkPlayer.Core.RepeatMode.Off ? null : tint;

        bool sh = _playlist?.Shuffle ?? _shuffle;
        ShuffleIcon.Foreground = sh ? accent : dim;
        ShuffleButton.Background = sh ? tint : null;

        AutoAdvanceIcon.Foreground = _autoAdvance ? accent : dim;
        AutoAdvanceButton.Background = _autoAdvance ? tint : null;
    }

    private void OnRepeatClick(object sender, RoutedEventArgs e)
    {
        _repeat = _repeat switch
        {
            OkPlayer.Core.RepeatMode.Off => OkPlayer.Core.RepeatMode.All,
            OkPlayer.Core.RepeatMode.All => OkPlayer.Core.RepeatMode.One,
            _ => OkPlayer.Core.RepeatMode.Off,
        };
        if (_playlist is not null) _playlist.Repeat = _repeat;
        RebuildUpNext(); // the up-next item can change (wrap), so refresh the NEXT badge + the buttons
    }

    private void OnShuffleClick(object sender, RoutedEventArgs e)
    {
        _shuffle = !_shuffle;
        if (_playlist is not null) _playlist.Shuffle = _shuffle;
        RebuildUpNext();
    }

    private void OnAutoAdvanceClick(object sender, RoutedEventArgs e)
    {
        _autoAdvance = !_autoAdvance;
        RefreshModeButtons();
    }

    /// <summary>Raised with the playlist's `.m3u` text when the user taps Save; MainWindow runs the save picker.</summary>
    public event EventHandler<string>? SavePlaylistRequested;

    private void OnSavePlaylistClick(object sender, RoutedEventArgs e)
    {
        if (_playlist is { Count: > 0 })
            SavePlaylistRequested?.Invoke(this, OkPlayer.Core.M3u.Write(_playlist.Items));
    }

    /// <summary>Open a `.m3u` as the active playlist: parse it (order preserved), keep the entries that exist
    /// or are URLs, and play the first.</summary>
    private void OpenM3u(string m3uPath)
    {
        try
        {
            var entries = OkPlayer.Core.M3u.Parse(System.IO.File.ReadAllText(m3uPath), System.IO.Path.GetDirectoryName(m3uPath));
            var valid = new System.Collections.Generic.List<string>();
            foreach (var entry in entries)
                if (entry.Contains("://") || System.IO.File.Exists(entry))
                    valid.Add(entry);
            if (valid.Count == 0)
            {
                ShowToast("Empty playlist");
                return;
            }
            _shuffle = false; // an .m3u defines its own order — honor it rather than shuffle it away
            _playlist = new OkPlayer.Core.Playlist(valid, valid[0], sort: false) { Repeat = _repeat };
            OpenMedia(valid[0]); // plays; UpdatePlaylist's SetCurrent keeps this list rather than the folder
            Vm.Play();
        }
        catch
        {
            ShowToast("Couldn't open this playlist");
        }
    }

    private void OnChaptersTab(object sender, TappedRoutedEventArgs e) => SetPanelTab(false);
    private void OnUpNextTab(object sender, TappedRoutedEventArgs e) => SetPanelTab(true);

    /// <summary>Switch the right panel between its Chapters and Up-Next tabs (one panel, two views).</summary>
    private void SetPanelTab(bool upNext)
    {
        UpNextView.Visibility = upNext ? Visibility.Visible : Visibility.Collapsed;
        ChaptersSectionHeader.Visibility = upNext ? Visibility.Collapsed : Visibility.Visible;
        ChapterList.Visibility = upNext ? Visibility.Collapsed : Visibility.Visible;
        ChaptersFooter.Visibility = upNext ? Visibility.Collapsed : Visibility.Visible;

        var accent = PanelBrush("OkAccentTextBrush", Windows.UI.Color.FromArgb(0xFF, 0x28, 0xB3, 0xAA));
        var secondary = PanelBrush("OkTextSecondaryBrush", Windows.UI.Color.FromArgb(0xB3, 0xFF, 0xFF, 0xFF));
        var pill = PanelBrush("OkPopoverBrush", Windows.UI.Color.FromArgb(0xF7, 0x1F, 0x1F, 0x1F));
        ChaptersTab.Background = upNext ? null : pill;
        ChaptersTabText.Foreground = upNext ? secondary : accent;
        UpNextTab.Background = upNext ? pill : null;
        UpNextTabText.Foreground = upNext ? accent : secondary;
    }

    private static Microsoft.UI.Xaml.Media.Brush PanelBrush(string key, Windows.UI.Color fallback) =>
        Microsoft.UI.Xaml.Application.Current.Resources.TryGetValue(key, out var v) && v is Microsoft.UI.Xaml.Media.Brush b
            ? b : new Microsoft.UI.Xaml.Media.SolidColorBrush(fallback);

    private void OnUpNextRowClick(object sender, RoutedEventArgs e)
    {
        if (sender is FrameworkElement { Tag: string path } && !string.Equals(path, _currentPath, StringComparison.OrdinalIgnoreCase))
        {
            OpenMedia(path);
            Vm.Play();
        }
    }

    /// <summary>Open the next file in the folder playlist (no-op at the end / without a playlist).</summary>
    public void PlayNext()
    {
        // Peek, don't advance: OpenMedia moves the cursor (SetCurrent) atomically with the row rebuild, so a
        // failed open can't leave the cursor ahead of the Up-Next rows.
        if (_playlist?.PeekNext is string next)
        {
            OpenMedia(next);
            Vm.Play(); // a hop from a played-out (keep-open paused) file must not inherit that pause
        }
    }

    /// <summary>Open the previous file in the folder playlist (no-op at the start / without a playlist).</summary>
    public void PlayPrevious()
    {
        if (_playlist?.PeekPrev is string prev)
        {
            OpenMedia(prev);
            Vm.Play();
        }
    }

    private void OnEndReached()
    {
        // Only advance if the current file is genuinely at its end. A queued eof-reached can arrive after a
        // manual hop (PageDown / opening another file) loaded a fresh file at position 0 — that stale event
        // must not skip a file. A real EOF leaves position at (≈) duration.
        if (_autoAdvance && Vm.Duration > 0 && Vm.Position >= Vm.Duration - 1.0 && _playlist?.AutoAdvanceTarget is string next)
        {
            if (string.Equals(next, _currentPath, StringComparison.OrdinalIgnoreCase))
            {
                Vm.SeekToFraction(0); // Repeat One: restart the loaded file, not reload+resume into an EOF loop
                Vm.Play();
            }
            else
            {
                ShowToast("Up next… " + System.IO.Path.GetFileNameWithoutExtension(next));
                OpenMedia(next);
                Vm.Play(); // the just-ended file left pause=yes (keep-open); play the next one through
            }
        }
    }

    private void OnOpenAccelerator(KeyboardAccelerator sender, KeyboardAcceleratorInvokedEventArgs args)
    {
        OpenFileRequested?.Invoke(this, EventArgs.Empty);
        args.Handled = true;
    }

    private bool _dragNameLoaded; // DragOver fires continuously — read the dragged name only once per drag

    private async void OnDragOver(object sender, DragEventArgs e)
    {
        if (!e.DataView.Contains(StandardDataFormats.StorageItems))
            return;
        e.AcceptedOperation = DataPackageOperation.Copy;
        try
        {
            // The custom DragOverlay already shows the accent drop-zone + "Drop to play"; suppress the OS
            // caption/glyph so the two don't double up.
            e.DragUIOverride.IsCaptionVisible = false;
            e.DragUIOverride.IsGlyphVisible = false;
        }
        catch { /* override not available on every shell drag — non-fatal */ }
        DragOverlay.Visibility = Visibility.Visible;
        if (_dragNameLoaded)
            return;
        _dragNameLoaded = true;
        var deferral = e.GetDeferral();
        try
        {
            var items = await e.DataView.GetStorageItemsAsync();
            var file = items.OfType<StorageFile>().FirstOrDefault();
            DragFileName.Text = file is not null ? System.IO.Path.GetFileName(file.Path) : string.Empty;
        }
        catch { DragFileName.Text = string.Empty; }
        finally { deferral.Complete(); }
    }

    private void OnDragLeave(object sender, DragEventArgs e) => HideDragOverlay();

    private void HideDragOverlay()
    {
        DragOverlay.Visibility = Visibility.Collapsed;
        _dragNameLoaded = false;
    }

    private async void OnDrop(object sender, DragEventArgs e)
    {
        HideDragOverlay();
        if (!e.DataView.Contains(StandardDataFormats.StorageItems))
            return;
        // async void: a transient first-time DataView access can throw — never let it escape to the UI thread.
        var deferral = e.GetDeferral();
        try
        {
            var items = await e.DataView.GetStorageItemsAsync();
            var file = items.OfType<StorageFile>().FirstOrDefault();
            if (file is not null)
            {
                // A subtitle dropped onto a playing video loads as a track rather than replacing the media.
                if (Vm.HasMedia && OkPlayer.Core.MediaFormats.IsSubtitle(file.Path))
                    AddSubtitle(file.Path);
                else
                    OpenMedia(file.Path);
            }
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
