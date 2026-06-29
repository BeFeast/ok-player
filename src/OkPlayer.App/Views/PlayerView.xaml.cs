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
    private readonly Microsoft.UI.Dispatching.DispatcherQueueTimer _loadWatchdog; // backstop: never let the spinner hang forever
    private bool _chromeVisible; // starts false to match the chrome's initial Opacity=0, so the first RevealChrome actually animates it in
    private bool _panelOpen;
    private bool _panelTabUserChosen; // the user tapped a panel tab for this file -> stop auto-defaulting it
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
    private bool _reachedEnd; // latched when the current file plays through to a natural EOF; resets on open
    private double? _explicitResume; // exact resume from a library launch (PRD §13.1): overrides history, skips the heuristic
    private int? _resumeSubId, _resumeAudioId; // remembered per-file track choice from history, applied right after open (a launch --sub/--audio still wins)

    /// <summary>The full resumable continue-watching pool (most-recent first).</summary>
    public ObservableCollection<RecentEntry> Recents { get; } = new();

    /// <summary>The leading slice of <see cref="Recents"/> actually shown on the welcome shelf — as many cards
    /// as fit the row width, so the shelf never needs a horizontal scrollbar. Recomputed in
    /// <see cref="RebuildVisibleRecents"/> on load and on resize.</summary>
    public ObservableCollection<RecentEntry> VisibleRecents { get; } = new();

    /// <summary>The remainder of <see cref="Recents"/> that didn't fit the row, listed in the "+N more" flyout
    /// so every resumable file stays reachable without a horizontal scrollbar.</summary>
    public ObservableCollection<RecentEntry> OverflowRecents { get; } = new();

    // Continue-watching card geometry, matched to the DataTemplate (194px card, 14px inter-card spacing), used
    // to work out how many fit the current row width.
    private const double RecentCardWidth = 194;
    private const double RecentCardSpacing = 14;
    // Welcome-shelf geometry, matched to the XAML (StackPanel MaxWidth 920, Padding 44 each side). The visible
    // count is computed from the available *viewport* width, not the content-sized RecentsRow — see
    // RebuildVisibleRecents.
    private const double RecentsShelfMaxWidth = 920;
    private const double RecentsShelfPadding = 44;

    /// <summary>Bookmarks for the current file, shown in the Chapters panel (bound from XAML).</summary>
    public ObservableCollection<BookmarkEntry> Bookmarks { get; } = new();
    private int _previewToken; // ignores stale async thumbnail results
    private bool _viewUnloaded; // guards against duplicate Unloaded disposing the thumbnail engine twice
    private int _openGeneration;      // bumps per file open; a stale chapter-warm pass bails on mismatch
    private bool _chapterWarmBusy;     // a chapter-thumbnail warm pass is running (single-flight)
    private bool _chapterWarmDirty;    // the chapter set changed (or a retry is wanted) — re-walk it
    private int _timelineWarmGen = -1; // the open generation a coarse seek-preview warm is already running for
    private System.Threading.Tasks.Task<bool>? _thumbReady; // resolves when the decode engine has the current file

    // ---- synced lyrics (audio karaoke overlay) ----
    private readonly ObservableCollection<LyricRow> _lyrics = new();
    private System.Collections.Generic.IReadOnlyList<OkPlayer.Core.LrcLine> _lyricLines = System.Array.Empty<OkPlayer.Core.LrcLine>();
    private int _lyricsGen;               // bumps per fetch / file change; a stale lyrics fetch bails on mismatch
    private int _lyricsActiveIndex = -1;  // the currently highlighted line, or -1
    private bool _lyricsTimed;            // the loaded sheet is synced (drives the highlight + click-to-seek)
    private string? _lyricsForPath;       // the file the loaded/loading lyrics belong to (set at load start)
    private bool _lyricsResolved;         // we have actual lyrics for _lyricsForPath (vs. a miss we may retry on tags)
    private System.Threading.CancellationTokenSource? _lyricsCts; // aborts a superseded in-flight fetch

    public PlayerViewModel Vm { get; } = new();

    /// <summary>The window's title-bar drag region — the top bar minus the right caption strip, so the custom
    /// caption buttons sit outside the drag element and reliably receive clicks.</summary>
    public FrameworkElement TitleBarElement => TitleDragRegion;

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
    /// <summary>Custom caption buttons (the native min/max/close are hidden so they can auto-hide over video).
    /// The host owns the AppWindow/presenter that actually minimizes, maximizes/restores, and closes.</summary>
    public event EventHandler? CaptionMinimizeRequested;
    public event EventHandler? CaptionMaximizeRestoreRequested;
    public event EventHandler? CaptionCloseRequested;

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

        _loadWatchdog = DispatcherQueue.CreateTimer();
        _loadWatchdog.Interval = TimeSpan.FromSeconds(30); // generous: a real load (even a slow stream) starts well within this
        _loadWatchdog.IsRepeating = false;
        _loadWatchdog.Tick += OnLoadWatchdogTick;

        _saveTimer = DispatcherQueue.CreateTimer();
        _saveTimer.Interval = TimeSpan.FromSeconds(10); // periodically persist the resume position
        _saveTimer.IsRepeating = true;
        _saveTimer.Tick += (_, _) => SaveProgress();
        _saveTimer.Start();

        Video.EngineReady += OnEngineReady;
        Video.ScreenshotSaved += (_, _) => DispatcherQueue.TryEnqueue(() => ShowToast("Screenshot saved"));
        Video.ScreenshotForClipboard += (_, ok) => DispatcherQueue.TryEnqueue(() => OnClipboardFrameReady(ok));
        Video.SubtitleAdded += (_, ok) => DispatcherQueue.TryEnqueue(() => OnSubtitleAdded(ok));
        HistorySurface.OpenRequested += OnHistoryOpenRequested;
        HistorySurface.CloseRequested += (_, _) => CloseHistory();
        HistorySurface.SettingsRequested += (_, _) => SettingsRequested?.Invoke(this, EventArgs.Empty);
        HistorySurface.ToastRequested += (_, msg) => ShowToast(msg);
        VolumeCtl.Vm = Vm;
        Seek.SeekRequested += OnSeekRequested;
        Seek.ScrubStateChanged += scrubbing => Vm.IsScrubbing = scrubbing;
        Seek.HoverChanged += OnSeekHover;
        Seek.HoverEnded += OnSeekHoverEnded;
        App.Settings.Changed += OnSettingsChanged; // re-evaluate pause auto-hide when its toggle changes mid-pause
        App.Updates.Changed += OnUpdateStateChanged; // surface a ready update once, as an unobtrusive toast
        OnUpdateStateChanged(); // also catch an update already staged before we subscribed (a prior-session download)
        Unloaded += (_, _) =>
        {
            if (_viewUnloaded) return;
            _viewUnloaded = true;
            _saveTimer.Stop();
            _history.Changed -= OnHistoryChanged; // shared instance outlives the view — don't leak the handler
            App.Settings.Changed -= OnSettingsChanged; // shared instance outlives the view — don't leak the handler
            App.Updates.Changed -= OnUpdateStateChanged;
            _mediaInfoWindow?.Close(); // don't leave the inspector window orphaned when the player tears down
            _lyricsCts?.Cancel();   // abort + release a lyrics fetch still in flight when the view tears down
            _lyricsCts?.Dispose();
            _lyricsCts = null;
            SaveProgress();
            System.Threading.Tasks.Task.Run(() => { _thumbs.Dispose(); _posterThumbs.Dispose(); });
        };
        Vm.PropertyChanged += OnVmPropertyChanged;
        Vm.ToastRequested += ShowToast;
        // Recolour the caption glyphs when the welcome shell flips light<->dark (no effect over video — white wins).
        ActualThemeChanged += (_, _) => { if (!Vm.HasMedia) ApplyCaptionPalette(false); };
        ApplyCaptionPalette(false); // start on the light welcome shell
        LyricsList.ItemsSource = _lyrics;
        // "Clear history" / retention prune can fire from the Settings window — refresh when it does.
        _history.Changed += OnHistoryChanged;
        Vm.EndReached += OnEndReached; // auto-advance the folder playlist when a file plays out
        Vm.LoadFailed += OnLoadFailed; // tear down the loading spinner if an open fails (e.g. a dead URL)
        Vm.MetadataChanged += () => ReloadOpenLyricsIf(metadataChanged: true); // late tags may now resolve lyrics
        SetPanelTab(false);            // initial visual; RefreshPanelTabs picks the real default per file on open
        Vm.Chapters.CollectionChanged += (_, _) =>
        {
            UpdateChaptersEmpty(); // seek-bar ticks bind Vm.ChapterFractions
            RefreshPanelTabs();    // chapters arriving/clearing changes whether the Chapters tab is offered at all
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
    private bool _historyOpen; // the History surface is showing (idle-only; mutually exclusive with playback)
    private bool _loading;     // an open is in flight (load accepted, awaiting the first frame or a load error)
    private string? _audioArtForPath; // the audio file we've resolved the now-playing cover art for

    private void ApplyMediaPresence()
    {
        bool has = Vm.HasMedia;
        if (has)
        {
            _loading = false;       // the file is ready — drop the loading spinner
            _loadWatchdog.Stop();   // and disarm the never-hang backstop
        }
        if (has && _historyOpen)
            _historyOpen = false; // opening a file from History (or anywhere) takes over the canvas
        // While a load is in flight the spinner owns the canvas, so suppress the welcome/History idle surfaces.
        bool idle = !has && !_loading;
        WelcomeCard.Visibility = (idle && !_historyOpen) ? Visibility.Visible : Visibility.Collapsed;
        HistorySurface.Visibility = (idle && _historyOpen) ? Visibility.Visible : Visibility.Collapsed;
        VideoBackdrop.Visibility = has ? Visibility.Visible : Visibility.Collapsed;
        Video.Visibility = has ? Visibility.Visible : Visibility.Collapsed;
        LoadingOverlay.Visibility = _loading ? Visibility.Visible : Visibility.Collapsed;
        ApplyAudioSurface(has);
        ApplyCaptionPalette(has); // white caption glyphs over video; theme-aware on the welcome shell
        MediaPresenceChanged?.Invoke(this, has);
        if (has)
        {
            RevealChrome();
        }
        else
        {
            _idleTimer.Stop();
            _chromeVisible = false;
            // Keep the title bar hit-testable on the welcome shell so the custom caption buttons stay clickable
            // (the scrim/title behind them is non-interactive and faded to 0); only the OSC drops out.
            TitleChrome.IsHitTestVisible = true;
            BottomChrome.IsHitTestVisible = false;
            CaptionBar.IsHitTestVisible = true;
            ChromeHideSb.Begin();
            CaptionShowSb.Begin(); // caption buttons are always available on the idle welcome/History surface
            if (idle && !_historyOpen)
                LoadRecents(); // refresh the welcome shelf (skip while History owns the canvas or a load is in flight)
        }
    }

    /// <summary>Audio-only media (flac/mp3/…) renders no video frames (the playback engine runs with
    /// audio-display=no, so embedded cover art is never a video track). Show a now-playing card over the black
    /// plane instead of looking broken — with the file's cover art when it has any. Gated on the audio extension
    /// so a real video file never flashes the card.</summary>
    private void ApplyAudioSurface(bool has)
    {
        bool audioOnly = has && Vm.VideoWidth <= 0
            && _currentPath is { } p && OkPlayer.Core.MediaFormats.IsAudio(p);
        AudioNowPlaying.Visibility = audioOnly ? Visibility.Visible : Visibility.Collapsed;
        if (audioOnly)
        {
            AudioTitle.Text = !string.IsNullOrWhiteSpace(Vm.MediaTitle)
                ? Vm.MediaTitle // the file's metadata title (mpv media-title); fall back to the bare file name
                : (_currentPath is { } cp ? System.IO.Path.GetFileNameWithoutExtension(cp) : string.Empty);
            if (_audioArtForPath != _currentPath) // kick the extraction once per file, not on every refresh
            {
                _audioArtForPath = _currentPath;
                ShowAudioArtFallback();             // music-note tile until the art (if any) lands
                _ = LoadCoverArtAsync(_currentPath!);
            }
        }
        else
        {
            _audioArtForPath = null; // next audio file re-resolves its art
            CloseLyrics();           // lyrics are audio-only — hide the overlay when we switch to video/welcome
        }
    }

    private void ShowAudioArtFallback()
    {
        AudioArtHost.Visibility = Visibility.Collapsed;
        AudioArtFallback.Visibility = Visibility.Visible;
        AudioArtBrush.ImageSource = null;
    }

    private async Task LoadCoverArtAsync(string path)
    {
        string? png = await OkPlayer.App.Services.CoverArtService.GetAsync(path);
        if (png is null || _currentPath != path || _audioArtForPath != path)
            return; // no embedded art, or the file changed while we were extracting
        try
        {
            AudioArtBrush.ImageSource = new Microsoft.UI.Xaml.Media.Imaging.BitmapImage(new Uri(png));
            AudioArtHost.Visibility = Visibility.Visible;
            AudioArtFallback.Visibility = Visibility.Collapsed;
        }
        catch { /* leave the fallback tile up if the bitmap can't be loaded */ }
    }

    // ===== synced lyrics (audio karaoke overlay) =====

    private void OnLyricsToggle(object sender, RoutedEventArgs e)
    {
        if (LyricsOverlay.Visibility == Visibility.Visible)
            CloseLyrics();
        else
            OpenLyrics();
    }

    private void OnLyricsClose(object sender, RoutedEventArgs e) => CloseLyrics();

    private void CloseLyrics() => LyricsOverlay.Visibility = Visibility.Collapsed;

    /// <summary>Show the overlay; fetch lyrics for the current track if we don't already have them, else just
    /// re-sync the highlight to the playhead.</summary>
    private void OpenLyrics()
    {
        LyricsOverlay.Visibility = Visibility.Visible;
        if (_lyricsForPath != _currentPath)
            _ = LoadLyricsAsync(_currentPath);
        else
            UpdateLyricHighlight(force: true);
    }

    /// <summary>Resolve lyrics for <paramref name="path"/> (sidecar → cache → LRCLIB) off the UI thread, behind a
    /// generation guard AND a per-load cancellation token so a track change supersedes — and actually aborts the
    /// in-flight network request of — a slow fetch. In a private session, don't cache to disk.</summary>
    private async System.Threading.Tasks.Task LoadLyricsAsync(string? path)
    {
        int gen = ++_lyricsGen;
        _lyricsCts?.Cancel();   // abort any prior in-flight fetch before it can land on the overlay
        _lyricsCts?.Dispose();
        _lyricsCts = new System.Threading.CancellationTokenSource();
        System.Threading.CancellationToken ct = _lyricsCts.Token;
        _lyrics.Clear();
        _lyricLines = System.Array.Empty<OkPlayer.Core.LrcLine>();
        _lyricsActiveIndex = -1;
        _lyricsTimed = false;
        _lyricsForPath = path;
        _lyricsResolved = false; // a miss stays retryable until the tags it needs actually arrive (MetadataChanged)
        ShowLyricsStatus("Searching for lyrics…");
        if (string.IsNullOrEmpty(path))
        {
            ShowLyricsStatus("No lyrics");
            return;
        }

        try
        {
            TrackMetadata meta = await Vm.ReadMetadataAsync(ct);
            if (gen != _lyricsGen)
                return;
            var (artist, track) = OkPlayer.Core.TrackTags.Resolve(
                meta.Artist, meta.Title, Vm.MediaTitle, System.IO.Path.GetFileNameWithoutExtension(path));
            bool privateSession = _history.Private;
            var query = new OkPlayer.App.Services.LyricsQuery(
                MediaPath: path, Artist: artist, Track: track, Album: meta.Album,
                DurationSeconds: meta.DurationSeconds,
                // A private session stays fully local — sidecar/cache only; no request (not even metadata) leaves.
                AllowNetwork: !privateSession, AllowCacheWrite: !privateSession);

            OkPlayer.Core.LrcDocument doc = await OkPlayer.App.Services.LyricsService.GetAsync(query, ct);
            if (gen != _lyricsGen)
                return; // a newer file / fetch superseded this one

            if (doc.IsEmpty)
            {
                // A miss leaves _lyricsResolved false (set above), so a later MetadataChanged — late-arriving tags
                // that may now resolve — re-attempts via ReloadOpenLyricsIf, even mid-fetch. A genuinely tag-less
                // track fires no further metadata change, so it settles on "No lyrics found" without thrashing.
                ShowLyricsStatus("No lyrics found for this track");
                return;
            }
            _lyricLines = doc.Lines;
            _lyricsTimed = doc.HasTimings;
            _lyricsResolved = true; // we have lyrics for this track — later tag changes shouldn't re-fetch
            // Untimed (plain) lyrics carry no per-line timestamps — tapping a line can't seek, so drop the click
            // affordance and mark the sheet "not synced" so it reads as the track's lyrics, just unsynced, rather
            // than looking broken when a tap does nothing.
            LyricsList.IsItemClickEnabled = _lyricsTimed;
            LyricsHeader.Text = _lyricsTimed ? "LYRICS" : "LYRICS · NOT SYNCED";
            foreach (OkPlayer.Core.LrcLine line in doc.Lines)
                _lyrics.Add(new LyricRow(line.Text, line.Time.TotalSeconds));
            HideLyricsStatus();
            UpdateLyricHighlight(force: true);
        }
        catch (System.OperationCanceledException)
        {
            // superseded by a newer load (or a file change) which cancelled this token — that load owns the overlay
        }
        catch when (gen != _lyricsGen)
        {
            // a newer load owns the overlay now — leave its state alone
        }
        catch
        {
            // fire-and-forget: a fault (e.g. a malformed sidecar, a network hiccup) must never strand the
            // overlay on the "Searching…" spinner — fall back to the no-lyrics state for this track.
            ShowLyricsStatus("No lyrics found for this track");
        }
    }

    /// <summary>If the lyrics overlay is open, (re)resolve for the current file. A <b>track change</b> always reloads
    /// (driven off media-title/duration — a playlist advance raises Duration even when two tracks share a title). A
    /// <b>metadata change</b> reloads only while we're still showing a miss (<see cref="_lyricsResolved"/> is false):
    /// that's the signal that the artist/title/album a lookup needs has arrived late — and it fires on mpv's tag
    /// dictionary, not the display title, so a missing-artist-but-same-title fill-in still re-resolves even mid-fetch
    /// (the generation guard + cancellation supersede the in-flight load). A genuinely tag-less track has no metadata
    /// change after load, so it settles on "No lyrics found" instead of thrashing on every Duration tick.</summary>
    private void ReloadOpenLyricsIf(bool metadataChanged)
    {
        if (LyricsOverlay.Visibility != Visibility.Visible)
            return;
        if (_lyricsForPath != _currentPath || (metadataChanged && !_lyricsResolved))
            _ = LoadLyricsAsync(_currentPath);
    }

    private void ShowLyricsStatus(string text)
    {
        LyricsStatus.Text = text;
        LyricsStatus.Visibility = Visibility.Visible;
        LyricsList.Visibility = Visibility.Collapsed;
    }

    private void HideLyricsStatus()
    {
        LyricsStatus.Visibility = Visibility.Collapsed;
        LyricsList.Visibility = Visibility.Visible;
    }

    /// <summary>Highlight the lyric line the playhead is on and scroll it into view. Cheap (a binary search and
    /// at most two flag flips); only touches the UI when the active line actually changes.</summary>
    private void UpdateLyricHighlight(bool force = false)
    {
        if (LyricsOverlay.Visibility != Visibility.Visible || !_lyricsTimed || _lyrics.Count == 0)
            return;
        int idx = OkPlayer.Core.LyricSync.ActiveIndex(_lyricLines, Vm.Position);
        if (idx == _lyricsActiveIndex && !force)
            return;
        if (_lyricsActiveIndex >= 0 && _lyricsActiveIndex < _lyrics.Count)
            _lyrics[_lyricsActiveIndex].IsActive = false;
        _lyricsActiveIndex = idx;
        if (idx >= 0 && idx < _lyrics.Count)
        {
            _lyrics[idx].IsActive = true;
            LyricsList.ScrollIntoView(_lyrics[idx]);
        }
    }

    private void OnLyricLineClick(object sender, Microsoft.UI.Xaml.Controls.ItemClickEventArgs e)
    {
        if (!_lyricsTimed || e.ClickedItem is not LyricRow row)
            return; // plain (untimed) lyrics carry no per-line position to seek to
        Vm.SeekToSeconds(row.Time);
        RevealChrome();
    }

    /// <summary>Drop any loaded lyrics (the file is changing/closing) so the next open re-resolves. Leaves the
    /// overlay's visibility alone — a playlist advance keeps it open and reloads when the new track's tags
    /// arrive (via the MediaTitle handler).</summary>
    private void ResetLyricsData()
    {
        _lyricsGen++; // supersede any in-flight fetch
        _lyricsCts?.Cancel(); // and abort its in-flight network request rather than let it run to completion
        _lyricsCts?.Dispose();
        _lyricsCts = null;
        _lyricsForPath = null;
        _lyricsResolved = false;
        _lyrics.Clear();
        _lyricLines = System.Array.Empty<OkPlayer.Core.LrcLine>();
        _lyricsActiveIndex = -1;
        _lyricsTimed = false;
        if (LyricsOverlay.Visibility == Visibility.Visible)
            ShowLyricsStatus("Searching for lyrics…");
    }

    private void OnLoadFailed()
    {
        _loading = false;      // async open/decode failure — tear down the spinner (the VM also toasts the error)
        _loadWatchdog.Stop();
        ApplyMediaPresence();  // back to the idle welcome surface
    }

    /// <summary>The open never produced a first frame or a load error within the timeout (e.g. mpv stalled on a
    /// dead network mount and emitted nothing) — give up rather than spin forever.</summary>
    private void OnLoadWatchdogTick(Microsoft.UI.Dispatching.DispatcherQueueTimer sender, object args)
    {
        sender.Stop();
        if (!_loading || Vm.HasMedia)
            return; // already resolved between the tick firing and now
        _loading = false;
        ApplyMediaPresence();
        ShowToast("Couldn't play this file");
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
        // Pre-fill from the clipboard when it holds a link, so "Open URL" doubles as paste-a-URL: copy a link in
        // the browser, click here, press Enter. Fire-and-forget so a slow/large clipboard provider can't delay
        // the dialog appearing — the field populates a moment later if the text is a URL.
        _ = PrefillUrlFromClipboardAsync(input);
        try
        {
            if (await dialog.ShowAsync() == ContentDialogResult.Primary && !string.IsNullOrWhiteSpace(input.Text))
                OpenMedia(input.Text.Trim());
        }
        catch { /* another content dialog is already open — ignore the concurrent open */ }
    }

    private static async Task PrefillUrlFromClipboardAsync(TextBox input)
    {
        try
        {
            var clip = Windows.ApplicationModel.DataTransfer.Clipboard.GetContent();
            if (clip.Contains(StandardDataFormats.Text))
            {
                string clipped = (await clip.GetTextAsync() ?? string.Empty).Trim();
                if (input.Text.Length == 0 && OkPlayer.Core.MediaFormats.IsPlayableUrl(clipped)) // don't clobber typing
                {
                    input.Text = clipped;
                    input.SelectAll();
                }
            }
        }
        catch { /* clipboard unavailable / slow — leave the field empty */ }
    }

    private void OnHistoryClick(object sender, RoutedEventArgs e) => OpenHistory();

    private void OnHistoryOpenRequested(object? sender, HistoryView.OpenRequest req)
    {
        // Fall back to the welcome shelf, then open — matching open-from-welcome, where the shelf stays
        // up until the video is ready and remains if the file fails to load (async decode errors leave
        // HasMedia false and never re-run ApplyMediaPresence, so collapsing to a bare canvas would strand it).
        CloseHistory();
        OpenMedia(req.Path, req.FromStart);
    }

    /// <summary>Open the full History surface — a third idle-canvas state alongside Welcome and playback.
    /// Idle-only: a file must be closed first (the OSC's Close file / X), so History never sits over video.</summary>
    private void OpenHistory()
    {
        if (Vm.HasMedia || _historyOpen)
            return;
        _historyOpen = true;
        HistorySurface.Load();
        ApplyMediaPresence();
        HistorySurface.Focus(FocusState.Programmatic); // History owns the keyboard (search / Esc) while open
    }

    private void CloseHistory()
    {
        if (!_historyOpen)
            return;
        _historyOpen = false;
        ApplyMediaPresence();   // back to the welcome shelf (and refresh it for any removals)
        Focus(FocusState.Programmatic);
    }

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
            _explicitResume = _pendingInitialResume; // after OpenMedia (it resets per-open state)
            _pendingInitialResume = null;
            ApplyLaunchTracks(_pendingInitialSub, _pendingInitialAudio);
            _pendingInitialSub = _pendingInitialAudio = null;
        }
        RevealChrome();
    }

    private void OnVmPropertyChanged(object? sender, System.ComponentModel.PropertyChangedEventArgs e)
    {
        if (e.PropertyName == nameof(PlayerViewModel.IsPaused))
        {
            if (Vm.IsPaused)
                RevealChrome();     // paused: reveal now; "hide when paused" (Settings) lets the idle timer hide it
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
            if (!Vm.HasMedia)
                CloseMediaInfo(); // the file closed — its media info is now stale
            ApplyMediaPresence();
        }
        else if (e.PropertyName == nameof(PlayerViewModel.VideoWidth))
        {
            // dwidth can arrive after file-loaded (or flip when cover art decodes), so re-decide the audio card.
            ApplyAudioSurface(Vm.HasMedia);
        }
        else if (e.PropertyName == nameof(PlayerViewModel.MediaTitle))
        {
            if (AudioNowPlaying.Visibility == Visibility.Visible)
                ApplyAudioSurface(Vm.HasMedia); // the metadata title arrives after file-loaded — refresh the card
            ReloadOpenLyricsIf(metadataChanged: false); // title change marks a track change for the open overlay
        }
        else if (e.PropertyName == nameof(PlayerViewModel.Duration))
        {
            TryResume(); // seek-bar chapter ticks update via the Vm.ChapterFractions binding
            WarmTimeline(); // preemptively warm a coarse grid of seek-preview frames for instant scrubbing
            ReloadOpenLyricsIf(metadataChanged: false); // a fresh Duration marks a track change (same-title advances)
        }
        else if (e.PropertyName == nameof(PlayerViewModel.Position))
        {
            UpdateLyricHighlight(); // advance the synced-lyrics highlight (no-op unless the overlay is open + timed)
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
            CaptionBar.IsHitTestVisible = true;
            ChromeShowSb.Begin();
            CaptionShowSb.Begin(); // caption buttons fade in with the chrome over video
            Vm.ApplySubtitlePosition(App.Settings.Current.SubtitlePosition, ComputeOscLift()); // lift subtitles above the OSC
        }
        ResetIdleTimer();
    }

    /// <summary>The sub-pos lift (percentage points) that clears the OSC for the current surface size. sub-pos
    /// is a percentage of render height but the OSC pill is a fixed device-independent height, so a constant
    /// percentage is too few pixels on a small surface (mini-player, a tiny window). Convert the OSC's fixed
    /// DIP clearance into the needed percentage, floored at the tuned large-window value. See
    /// <see cref="OkPlayer.Core.SubtitleLift"/>.</summary>
    private double ComputeOscLift()
        => OkPlayer.Core.SubtitleLift.ForSurface(ActualHeight, OscClearanceDip, PlayerViewModel.OscSubtitleLift);

    // The OSC pill is anchored to the bottom of the player surface at a fixed device-independent height
    // (Margin 16,0,16,18 + Padding 18,11 + the ~32px control row → its top is ~72 DIP up); lift captions a
    // little past that so they never touch the controls.
    private const double OscClearanceDip = 88;

    // True when the OSC should auto-hide on pause as well as during playback (Settings -> Playback). The same
    // 2.5s idle timeout applies; any pointer move re-reveals it.
    private bool PauseHideEnabled => Vm.IsPaused && App.Settings.Current.HideControlsWhenPaused;

    private void HideChrome()
    {
        // no media / panel-open / already-hidden keep the chrome up; so does pause UNLESS "hide when paused" is on.
        if (!_chromeVisible || !Vm.HasMedia || _panelOpen || (!Vm.IsPlaying && !PauseHideEnabled))
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
        CaptionBar.IsHitTestVisible = false; // hidden + non-hittable so a top-right click over video can't trigger it
        ChromeHideSb.Begin();
        CaptionHideSb.Begin(); // caption buttons fade out with the chrome over video
        Vm.ApplySubtitlePosition(App.Settings.Current.SubtitlePosition, 0); // drop subtitles back to the user's position
    }

    private void ResetIdleTimer()
    {
        _idleTimer.Stop();
        if (Vm.HasMedia && !_panelOpen && (Vm.IsPlaying || PauseHideEnabled))
            _idleTimer.Start();
    }

    // ---- custom caption buttons ----

    private void OnCaptionMinimizeClick(object sender, RoutedEventArgs e) => CaptionMinimizeRequested?.Invoke(this, EventArgs.Empty);
    private void OnCaptionMaximizeClick(object sender, RoutedEventArgs e) => CaptionMaximizeRestoreRequested?.Invoke(this, EventArgs.Empty);
    private void OnCaptionCloseClick(object sender, RoutedEventArgs e) => CaptionCloseRequested?.Invoke(this, EventArgs.Empty);

    /// <summary>Swap the maximize/restore glyph + tooltip to match the window state. The host window calls this
    /// when the presenter/size changes (maximize, restore, snap, drag-to-top).</summary>
    public void SetMaximizedGlyph(bool maximized)
    {
        MaximizeButton.Content = maximized ? "" : ""; // ChromeRestore : ChromeMaximize
        Microsoft.UI.Xaml.Controls.ToolTipService.SetToolTip(MaximizeButton, maximized ? "Restore" : "Maximize");
    }

    /// <summary>Recolour the custom caption glyphs for the surface behind them: white over video, theme-aware on
    /// the light/dark welcome shell. Mutates the shared brushes referenced by the caption button templates.</summary>
    private void ApplyCaptionPalette(bool overVideo)
    {
        var glyph = (Microsoft.UI.Xaml.Media.SolidColorBrush)Resources["OkCaptionGlyphBrush"];
        var hover = (Microsoft.UI.Xaml.Media.SolidColorBrush)Resources["OkCaptionHoverBrush"];
        var pressed = (Microsoft.UI.Xaml.Media.SolidColorBrush)Resources["OkCaptionPressedBrush"];
        if (overVideo || ActualTheme == ElementTheme.Dark)
        {
            glyph.Color = Windows.UI.Color.FromArgb(0xF2, 0xFF, 0xFF, 0xFF);
            hover.Color = Windows.UI.Color.FromArgb(0x1F, 0xFF, 0xFF, 0xFF);
            pressed.Color = Windows.UI.Color.FromArgb(0x2E, 0xFF, 0xFF, 0xFF);
        }
        else
        {
            glyph.Color = Windows.UI.Color.FromArgb(0xE6, 0x1A, 0x1A, 0x1A);
            hover.Color = Windows.UI.Color.FromArgb(0x14, 0x00, 0x00, 0x00);
            pressed.Color = Windows.UI.Color.FromArgb(0x24, 0x00, 0x00, 0x00);
        }
    }

    // Toggling "Hide controls when paused" (Settings) while a file is already paused must take effect now, not
    // on the next pointer move. RevealChrome shows the controls and (via ResetIdleTimer) arms the idle timer
    // when the setting is on, or leaves them up when it's off. Fires on the shared UI thread (both windows).
    private void OnSettingsChanged()
    {
        if (Vm.IsPaused)
            RevealChrome();
    }

    private bool _updateBannerShown; // session guard: surface a ready update once, not on every Changed tick

    /// <summary>When a background-downloaded update becomes ready, surface it once per session as an actionable
    /// banner with an in-place "Restart now" — no trip to Settings. <see cref="UpdateService.Changed"/> can fire
    /// off the UI thread (the check completes on a worker), so marshal before touching the UI.</summary>
    private void OnUpdateStateChanged() => DispatcherQueue.TryEnqueue(() =>
    {
        if (_viewUnloaded || _updateBannerShown || !App.Updates.UpdateReady)
            return; // skip a callback that landed after the view tore down (Changed can fire from a worker thread)
        _updateBannerShown = true;
        string ver = App.Updates.PendingVersion is { } v ? $"Update ready · {v}" : "Update ready";
        UpdateBannerText.Text = ver;
        UpdateBanner.Visibility = Visibility.Visible;
        UpdateBannerShowSb.Begin();
    });

    /// <summary>Apply the staged update and relaunch — the banner's primary action. Tears the process down.</summary>
    private void OnUpdateRestartClick(object sender, RoutedEventArgs e) => App.Updates.ApplyAndRestart();

    /// <summary>Dismiss the banner for this session; the update stays staged and is re-offered on the next launch
    /// (UpdateReady persists), and Settings → About still has the restart action.</summary>
    private void OnUpdateLaterClick(object sender, RoutedEventArgs e)
    {
        UpdateBannerHideSb.Completed += HideUpdateBannerOnCompleted;
        UpdateBannerHideSb.Begin();
    }

    private void HideUpdateBannerOnCompleted(object? sender, object e)
    {
        UpdateBannerHideSb.Completed -= HideUpdateBannerOnCompleted;
        UpdateBanner.Visibility = Visibility.Collapsed;
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
        if (_historyOpen)
            return; // History owns the keyboard while open (its own search box / Esc handling)
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
            case (VirtualKey)0x58: CloseFile(); break;            // X — close the current file, back to Welcome
            case (VirtualKey)0x48: if (!Vm.HasMedia) OpenHistory(); else handled = false; break; // H — open History (idle)
            case VirtualKey.PageDown: PlayNext(); break;          // next file in the folder playlist
            case VirtualKey.PageUp:   PlayPrevious(); break;      // previous file
            case VirtualKey.Escape:
                if (_mediaInfoWindow is not null) CloseMediaInfo();
                else if (LyricsOverlay.Visibility == Visibility.Visible) CloseLyrics(); // dismiss the lyrics sheet first
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
    private void OnCloseFileClick(object sender, RoutedEventArgs e) => CloseFile();
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

    /// <summary>Toggle the compact-overlay mini-player (native Windows PiP). The owning window applies it
    /// (it holds the AppWindow and tracks the mode); raised as a plain toggle, like fullscreen.</summary>
    public event EventHandler? MiniPlayerRequested;

    private void OnMiniPlayerClick(object sender, RoutedEventArgs e)
    {
        if (!Vm.HasMedia)
            return; // the menu items are IsEnabled-bound to HasMedia, but guard here too: never enter PiP on the welcome screen
        MiniPlayerRequested?.Invoke(this, EventArgs.Empty);
    }

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

    /// <summary>Copy the current playhead position as a timecode to the clipboard (pairs with Go to time —
    /// share or note a moment).</summary>
    private void OnCopyTimeClick(object sender, RoutedEventArgs e)
    {
        if (!Vm.HasMedia)
            return;
        string tc = OkPlayer.Core.TimeCode.Format(Vm.Position);
        try
        {
            var data = new DataPackage { RequestedOperation = DataPackageOperation.Copy };
            data.SetText(tc);
            Clipboard.SetContent(data);
            ShowToast($"Copied {tc}");
        }
        catch { ShowToast("Couldn't copy the time"); }
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
            // "Finished" latches only when the file played through to a natural EOF (_reachedEnd), never from a
            // sampled position — so seeking into the final stretch, or seeking back after the credits, can't flip
            // the watched flag, and a sub-30s clip isn't marked finished the instant it opens at position 0.
            // Independently, parking the stored position at 0 once the playhead is in the final stretch keeps
            // resume/continue-watching clean. "Final stretch" = the last 30s, but never more than the final 5%.
            double completeAt = Math.Max(Vm.Duration * 0.95, Vm.Duration - 30);
            double position = (_reachedEnd || Vm.Position >= completeAt) ? 0 : Vm.Position;
            // Also remember the user's current subtitle/audio track choice so reopening the file restores it.
            _history.Record(path, position, Vm.Duration, _reachedEnd, Vm.CurrentSubtitleId, Vm.CurrentAudioId);
        }
    }

    /// <summary>Stop playback, unload the current file, and fall back to the Welcome card. Persists the resume
    /// position first (same as EOF), then drops all per-file state so the next open starts clean. No-op without
    /// media. Surfaced on the OSC overflow menu, the right-click menu, and the X key.</summary>
    private void CloseFile()
    {
        if (!Vm.HasMedia)
            return;
        SaveProgress();          // persist the outgoing file's position before we unload it
        if (_panelOpen)
            TogglePanel();       // collapse the side panel — its chapters/bookmarks/up-next are now empty
        Vm.CloseFile();          // stop mpv + clear title/tracks/chapters; flips HasMedia → ApplyMediaPresence → Welcome
        _currentPath = null;
        _reachedEnd = false;
        _explicitResume = null;
        _resumeTarget = -1;
        _resumeSubId = _resumeAudioId = null;
        _openGeneration++;       // invalidate any in-flight chapter/thumbnail warm for the closed file
        CloseLyrics();           // back to the welcome surface — drop the lyrics overlay…
        ResetLyricsData();       // …and the loaded sheet
        _loading = false;        // a close during a slow playlist/file open owns the loading state: drop the spinner…
        _loadWatchdog.Stop();    // …and disarm the watchdog so a superseded open can't later fire a false toast
        _playlist = null;        // drop the folder-as-playlist…
        UpNext.Clear();          // …and its projected rows
    }

    private void TryResume()
    {
        if (Vm.Duration <= 0)
            return;
        // A companion-library launch (PRD §13.1) carries an exact position: honour it verbatim — overriding any
        // remembered position and bypassing the auto-resume heuristic, since the library, not the player, decides
        // where to start. mpv can report a provisional (small) duration before the final one for progressive /
        // network media, so wait until the known duration actually covers the target before seeking — otherwise
        // we'd land at the wrong early spot and then skip the real seek. (A target of 0, "from the start", and a
        // file genuinely shorter than the target both fall through gracefully: start from 0.) Seek by absolute
        // seconds, clamped just shy of the end so a value at/over the end can't land on EOF and latch "finished".
        if (_explicitResume is { } exact)
        {
            if (exact > Vm.Duration)
                return; // keep _explicitResume queued; a later, larger Duration will apply it (or the next open clears it)
            _explicitResume = null;
            _resumeTarget = -1; // the explicit position wins; drop any history target queued for this open
            double seekTo = Math.Min(exact, Math.Max(0, Vm.Duration - 0.5));
            Vm.SeekToSeconds(seekTo);
            ShowToast($"Resumed at {FormatPreviewTime(seekTo)}");
            return;
        }
        if (_resumeTarget <= 0)
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
                entry.Poster = PosterImage.Load(rec.PosterPath!);
            Recents.Add(entry);
            if (Recents.Count >= 10) // a bounded pool; the shelf shows what fits, the rest live in History
                break;
        }
        // Two welcome layouts: recents-forward "Continue watching" when there is resumable history,
        // else the centred first-run hero.
        bool hasRecents = Recents.Count > 0;
        // You don't "watch" audio: when every resumable item is a track, call the shelf "Continue listening".
        // Mixed or any video keeps "Continue watching" (there's something to watch).
        bool allAudio = hasRecents && Recents.All(r => OkPlayer.Core.MediaFormats.IsAudio(r.Path));
        ContinueHeader.Text = allAudio ? "Continue listening" : "Continue watching";
        WelcomeVariationA.Visibility = hasRecents ? Visibility.Visible : Visibility.Collapsed;
        WelcomeFirstRun.Visibility = hasRecents ? Visibility.Collapsed : Visibility.Visible;
        RebuildVisibleRecents(); // pick the leading cards that fit + the "+N more" hint
        _ = GeneratePostersAsync(); // fill any missing posters in the background
    }

    /// <summary>Split <see cref="Recents"/> into <see cref="VisibleRecents"/> (as many cards as fit the row
    /// width — never overflowing into a horizontal scrollbar) and <see cref="OverflowRecents"/> (the rest,
    /// reached through the "+N more" flyout so nothing becomes unreachable). Idempotent and flicker-free: each
    /// collection is only mutated when its slice actually differs.</summary>
    private void RebuildVisibleRecents()
    {
        // Measure the available width from the *viewport*, not RecentsRow.ActualWidth. The row is a Grid inside
        // a centred, content-sized StackPanel, so its width tracks the cards it currently holds — once the
        // window shrinks it down to a single card it can never grow back (it would only ever measure one card
        // wide). The viewport width is the true space the shelf may use, capped at the StackPanel's MaxWidth
        // and inset by its horizontal padding. 0 before first layout → VisibleCount falls back to its default.
        double viewport = VariationAScroll?.ViewportWidth ?? 0;
        double avail = viewport <= 0 ? 0 : Math.Min(RecentsShelfMaxWidth, viewport) - 2 * RecentsShelfPadding;
        int want = OkPlayer.Core.RecentsShelf.VisibleCount(
            avail, Recents.Count, RecentCardWidth, RecentCardSpacing);

        SyncSlice(VisibleRecents, 0, want);
        SyncSlice(OverflowRecents, want, Recents.Count);

        int more = Recents.Count - want;
        MoreRecentsText.Text = more > 0 ? $"+{more} more" : string.Empty;
        MoreRecentsLink.Visibility = more > 0 ? Visibility.Visible : Visibility.Collapsed;
    }

    /// <summary>Make <paramref name="target"/> equal Recents[start..end), touching it only if it differs.</summary>
    private void SyncSlice(ObservableCollection<RecentEntry> target, int start, int end)
    {
        int count = end - start;
        bool same = target.Count == count;
        for (int i = 0; same && i < count; i++)
            if (!ReferenceEquals(target[i], Recents[start + i])) same = false;
        if (same)
            return;
        target.Clear();
        for (int i = start; i < end; i++)
            target.Add(Recents[i]);
    }

    private void OnRecentsViewportSizeChanged(object sender, SizeChangedEventArgs e) => RebuildVisibleRecents();

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

    // Sentinel stored as PosterPath when a file has no usable (non-black) frame, so we don't re-derive it
    // every welcome visit. Treated as "no poster" by consumers (File.Exists is false), so the gradient shows.
    private const string NoUsablePoster = "(none)";

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
            const double minUsableLuma = 22; // below this a thumbnail reads as a black block — show the gradient
            foreach (var entry in Recents.ToList())
            {
                if (Vm.HasMedia) // playback started — stop the background pass
                    break;
                var rec = _history.Get(entry.Path);
                string poster = System.IO.Path.Combine(dir, PosterHash(entry.Path) + ".png");
                if (OkPlayer.Core.MediaFormats.IsAudio(entry.Path))
                {
                    // Before the no-art gate: a sidecar cover can be dropped in after the track was first seen,
                    // so EnsureAudioPosterAsync re-checks it cheaply and only the embedded extraction is skipped.
                    await EnsureAudioPosterAsync(entry, poster);
                    continue;
                }
                if (rec?.PosterPath == NoUsablePoster)
                    continue; // video: already decided this file has no usable (non-black) frame — keep the gradient
                // Keep an existing poster only if it isn't (near-)black: earlier builds cached black frames and
                // a fixed grab can land on a dark scene, so re-validate rather than trusting the cache forever.
                if (System.IO.File.Exists(poster) && await MeanLumaAsync(poster) is double cached && cached >= minUsableLuma)
                {
                    if (entry.Poster is null)
                        DispatcherQueue.TryEnqueue(() => entry.Poster = PosterImage.Load(poster));
                    continue;
                }

                if (!await _posterThumbs.OpenAsync(entry.Path))
                    continue;
                var (frame, frameLuma) = await PickRepresentativeFrameAsync(rec is { Duration: > 0 } ? rec.Duration : 0);
                if (frame is null)
                    continue; // nothing decoded (transient) — retry on a later pass, don't give up
                if (frameLuma < minUsableLuma)
                {
                    // Even the brightest sampled frame is basically black — a clean light gradient beats a black
                    // block. Drop any stale poster and mark the file so we don't re-derive it every visit.
                    try { if (System.IO.File.Exists(poster)) System.IO.File.Delete(poster); } catch { /* best effort */ }
                    _history.SetPoster(entry.Path, NoUsablePoster);
                    DispatcherQueue.TryEnqueue(() => entry.Poster = null); // show the gradient, not a stale black image
                    continue;
                }
                try { System.IO.File.Copy(frame, poster, overwrite: true); } catch { continue; }
                _history.SetPoster(entry.Path, poster);
                DispatcherQueue.TryEnqueue(() => entry.Poster = PosterImage.Load(poster));
            }
        }
        catch { /* best effort */ }
        finally { _generatingPosters = false; }
    }

    /// <summary>Fill an audio recent's poster from a sidecar cover image or, failing that, its embedded album art
    /// (there's no video frame to grab). Caches the art into the persistent posters dir and records it on the
    /// history entry so later visits load instantly; marks the file posterless (gradient) only when it has
    /// neither, so we don't re-extract embedded art every visit. A cheap sidecar re-check still runs even for a
    /// posterless entry — a cover dropped in next to the track later overrides the verdict. Best-effort.</summary>
    private async System.Threading.Tasks.Task EnsureAudioPosterAsync(RecentEntry entry, string posterPath)
    {
        if (entry.Poster is not null)
            return; // already shown (loaded from the cached poster path in LoadRecents)
        // 1) Cheap sidecar-only check first — runs even when the entry is marked NoUsablePoster, so art added
        //    after the track was first seen is picked up (the media file's mtime/size don't change).
        // GetSidecarAsync already found this file by enumerating off-thread; the re-stat just guards a TOCTOU
        // delete. Skip it for network paths so a flaky share can't block the resumed continuation (UI thread).
        if (await Services.CoverArtService.GetSidecarAsync(entry.Path) is { } sidecar
            && (OkPlayer.Core.NetworkPath.IsNetwork(sidecar) || System.IO.File.Exists(sidecar)))
        {
            await SetAudioPosterAsync(entry, sidecar, posterPath);
            return;
        }
        // 2) No sidecar. If we already determined there's no embedded picture, don't re-run the costly extractor.
        if (_history.Get(entry.Path)?.PosterPath == NoUsablePoster)
            return;
        var (art, definitelyNoArt) = await Services.CoverArtService.GetWithStatusAsync(entry.Path);
        if (art is not null && System.IO.File.Exists(art))
            await SetAudioPosterAsync(entry, art, posterPath);
        else if (definitelyNoArt)
            _history.SetPoster(entry.Path, NoUsablePoster); // neither sidecar nor embedded — keep the gradient
        // else: a transient failure (timeout/locked/unreadable) — leave the gradient and retry on a later pass
    }

    /// <summary>Copy a resolved cover image into the persistent posters dir, record it on the history entry, and
    /// show it on the card. The copy makes the poster self-contained (a sidecar the user later moves won't break
    /// it); the copied file is content-sniffed on load, so a .png-named JPEG/WebP still renders. The copy runs
    /// OFF the UI thread — the source can be a sidecar next to the media on a (possibly network) share, and a
    /// synchronous File.Copy of a stalled SMB file would freeze the dispatcher.</summary>
    private async System.Threading.Tasks.Task SetAudioPosterAsync(RecentEntry entry, string sourceImage, string posterPath)
    {
        try { await System.Threading.Tasks.Task.Run(() => System.IO.File.Copy(sourceImage, posterPath, overwrite: true)); }
        catch { return; }
        _history.SetPoster(entry.Path, posterPath);
        DispatcherQueue.TryEnqueue(() => entry.Poster = PosterImage.Load(posterPath));
    }

    /// <summary>Pick a non-black poster frame. A single fixed 20% grab often lands on a fade/dark scene (studio
    /// logos, dark openings) → a black poster; instead sample a few positions across the file and keep the
    /// brightest. Stops early once a clearly-lit frame is found. Falls back to the brightest sampled frame when
    /// the whole film is dark, and to a single fixed grab when the duration is unknown.</summary>
    private async System.Threading.Tasks.Task<(string? Frame, double Luma)> PickRepresentativeFrameAsync(double duration)
    {
        const double litEnough = 48; // mean luma (0–255); a clearly-lit scene, well clear of black/fade frames
        // Sample widely — many films open dark and only brighten mid-reel, so cover 15%–82% of the runtime.
        double[] fractions = { 0.15, 0.25, 0.38, 0.50, 0.62, 0.75, 0.82 };
        string? best = null;
        double bestLuma = -1;
        foreach (double f in fractions)
        {
            double when = duration > 0 ? Math.Max(3, duration * f) : 30;
            string? frame = await _posterThumbs.GetThumbnailAsync(when);
            if (frame is { } && System.IO.File.Exists(frame)
                && await MeanLumaAsync(frame) is double luma && luma > bestLuma)
            {
                // Only a frame that actually decoded can win — an unreadable/partial PNG (null) must never
                // become the poster, even though the file "exists".
                bestLuma = luma;
                best = frame;
            }
            if (duration <= 0 || bestLuma >= litEnough)
                break; // unknown duration → one grab; otherwise stop as soon as a lit frame is in hand
        }
        return (best, bestLuma);
    }

    /// <summary>Mean luma (0–255) of a PNG via the platform codec, scored by <see cref="OkPlayer.Core.ImageLuma"/>.
    /// Returns null when the file can't be read/decoded, so the caller skips it rather than letting a broken PNG
    /// (which would otherwise score 0) win as the poster.</summary>
    private static async System.Threading.Tasks.Task<double?> MeanLumaAsync(string pngPath)
    {
        try
        {
            var file = await Windows.Storage.StorageFile.GetFileFromPathAsync(pngPath);
            using var stream = await file.OpenAsync(Windows.Storage.FileAccessMode.Read);
            var decoder = await Windows.Graphics.Imaging.BitmapDecoder.CreateAsync(stream);
            var pixels = await decoder.GetPixelDataAsync(
                Windows.Graphics.Imaging.BitmapPixelFormat.Bgra8,
                Windows.Graphics.Imaging.BitmapAlphaMode.Ignore,
                new Windows.Graphics.Imaging.BitmapTransform(),
                Windows.Graphics.Imaging.ExifOrientationMode.IgnoreExifOrientation,
                Windows.Graphics.Imaging.ColorManagementMode.DoNotColorManage);
            return OkPlayer.Core.ImageLuma.MeanBgra(pixels.DetachPixelData());
        }
        catch { return null; }
    }

    private static string PosterHash(string path)
    {
        byte[] hash = System.Security.Cryptography.SHA1.HashData(System.Text.Encoding.UTF8.GetBytes(path));
        return System.Convert.ToHexString(hash);
    }

    // ---- media info (design band 13: Streams) ----

    private MediaInfoWindow? _mediaInfoWindow; // the open inspector window (single instance), or null
    private bool _mediaInfoBuilding;           // in-flight guard: one off-thread property read at a time

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
        if (_mediaInfoWindow is not null) // toggle: a second press closes the open inspector window
        {
            CloseMediaInfo();
            return;
        }
        if (!Vm.HasMedia || Video.Engine is not { } engine || _mediaInfoBuilding)
            return;
        _mediaInfoBuilding = true;
        string? path = _currentPath; // pin the file we're reading so a mid-read switch can't show stale info
        MediaInfoViewModel model;
        try { model = await System.Threading.Tasks.Task.Run(() => BuildMediaInfo(engine, path)); }
        catch { return; } // engine torn down mid-read — just don't show the card
        finally { _mediaInfoBuilding = false; }
        if (!Vm.HasMedia || _mediaInfoWindow is not null || _currentPath != path || _viewUnloaded)
            return; // the file changed, a window opened, or the view tore down while we were reading
        _mediaInfoModel = model;
        var win = new MediaInfoWindow(model);
        win.CopyRequested += (_, _) => OnMediaInfoCopy();
        win.Closed += (_, _) => { _mediaInfoWindow = null; _mediaInfoModel = null; };
        _mediaInfoWindow = win;
        win.Activate();
    }

    private void CloseMediaInfo() => _mediaInfoWindow?.Close(); // the Closed handler clears the field

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

    private void OnSecondarySubtitleOffClick(object sender, RoutedEventArgs e) { Vm.SetSecondarySubtitleOff(); SubtitleFlyout.Hide(); RevealChrome(); }

    private void OnSecondarySubtitleTrackClick(object sender, RoutedEventArgs e)
    {
        if (sender is FrameworkElement { DataContext: TrackInfo track })
            Vm.SelectSecondarySubtitle(track);
        SubtitleFlyout.Hide();
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
            RefreshPanelTabs();      // land on the right default tab (audio/no-chapters -> Up Next) and hide an empty Chapters tab
            LoadBookmarks();
            ChaptersPanel.Visibility = Visibility.Visible;
            PanelBackdrop.Visibility = Visibility.Visible; // arm light-dismiss: a click outside the panel closes it
            PanelShowSb.Begin();
            RevealChrome(); // an open panel pins the chrome
            WarmChapterThumbnails(); // ensure previews are filling (usually already warmed on open)
        }
        else
        {
            PanelBackdrop.Visibility = Visibility.Collapsed;
            PanelHideSb.Begin(); // the Completed handler collapses it
            ResetIdleTimer();
        }
    }

    private void OnPanelBackdropPressed(object sender, PointerRoutedEventArgs e)
    {
        if (_panelOpen)
            TogglePanel(); // click outside the panel dismisses it (light-dismiss)
        e.Handled = true;  // consume the click so it doesn't also reach the OSC underneath
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

    /// <summary>Reveal the current file in Explorer (selected). Local files only — URLs/streams have no
    /// folder; a moved/deleted file is reported rather than opening an empty window.</summary>
    // Gate "Open file location" on having a revealable local path rather than on HasMedia: a load that failed
    // still leaves _currentPath set (and that local file is worth revealing), while the welcome screen has no
    // path at all — so HasMedia would both wrongly disable the former and offer a no-op on the latter. URLs and
    // streams aren't on disk, so exclude them here too (Greptile P2).
    private void OnContextMenuOpening(object sender, object e)
    {
        OpenFileLocationItem.IsEnabled =
            _currentPath is { } path && !path.Contains("://", StringComparison.Ordinal);
    }

    private void OnOpenFileLocationClick(object sender, RoutedEventArgs e)
    {
        if (_currentPath is not { } path || path.Contains("://", StringComparison.Ordinal))
        {
            ShowToast("Not a local file");
            return;
        }
        // Skip the existence check for network paths — statting a dead SMB mount on the UI thread would freeze
        // the window; Explorer handles a missing target on its own. Only stat genuinely local files here.
        if (!OkPlayer.Core.NetworkPath.IsNetwork(path) && !System.IO.File.Exists(path))
        {
            ShowToast("File not found");
            return;
        }
        try
        {
            // /select highlights the file in its folder; quote the path so spaces are preserved.
            System.Diagnostics.Process.Start(new System.Diagnostics.ProcessStartInfo
            {
                FileName = "explorer.exe",
                Arguments = $"/select,\"{path}\"",
                UseShellExecute = true,
            });
        }
        catch { ShowToast("Couldn't open the folder"); }
    }

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
    private double? _pendingInitialResume; // explicit resume paired with _pendingInitialPath
    private int? _pendingInitialSub, _pendingInitialAudio; // launch-time track preselection paired with _pendingInitialPath

    /// <summary>Apply the user's default subtitle size/position (Settings -> Subtitles) to the engine. Live —
    /// safe to call any time; a no-op when no engine/file is up.</summary>
    public void ApplySubtitleDefaults()
    {
        try
        {
            if (Video.Engine is { } e)
            {
                e.SetProperty("sub-scale", App.Settings.Current.SubtitleScale);
                // Appearance preset (Settings -> Subtitles -> STYLE): a fixed set of sub-* style options.
                // Every preset writes the same options, so switching fully overrides the previous look.
                // These style mpv's own text-sub renderer; ASS subs keep their embedded styling by design.
                foreach (var (name, value) in OkPlayer.Core.SubtitleStyle.FromKey(App.Settings.Current.SubtitleStyle).Options)
                    e.SetProperty(name, value);
                // sub-pos is owned by the OSC-lift (it both positions and lifts subtitles); re-apply it for
                // the current chrome state so a live position change keeps the lift consistent.
                Vm.ApplySubtitlePosition(App.Settings.Current.SubtitlePosition, _chromeVisible ? ComputeOscLift() : 0);
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
    public void QueueInitialFile(string path, double? resumeSeconds = null, int? subTrack = null, int? audioTrack = null)
    {
        if (Video.Engine is not null)
        {
            OpenMedia(path);
            _explicitResume = resumeSeconds; // set after OpenMedia, which resets per-open state; applied on first Duration
            ApplyLaunchTracks(subTrack, audioTrack);
        }
        else
        {
            _pendingInitialPath = path;
            _pendingInitialResume = resumeSeconds;
            _pendingInitialSub = subTrack;
            _pendingInitialAudio = audioTrack;
        }
    }

    /// <summary>Apply a launch-time subtitle/audio track preselection (PRD §13.1). mpv honours sid/aid as the
    /// file loads, so this is set once right after open — no per-open latch needed. -1 means "none"/off.</summary>
    private void ApplyLaunchTracks(int? subTrack, int? audioTrack)
    {
        // -1 = explicit off; >= 1 = a real (1-based) mpv track. 0 would mean "auto" to mpv, so ignore it —
        // LaunchArgs already rejects 0, this just keeps the apply site self-defending.
        if (subTrack is int s)
        {
            if (s < 0) Vm.SetSubtitleOff();
            else if (s >= 1) Vm.SelectSubtitleId(s);
        }
        if (audioTrack is int a)
        {
            if (a < 0) Vm.SetAudioOff();
            else if (a >= 1) Vm.SelectAudioId(a);
        }
    }

    /// <summary>Restore the subtitle/audio track the user last chose for this file (captured into
    /// <see cref="_resumeSubId"/>/<see cref="_resumeAudioId"/> from history in <see cref="OpenMedia"/>). Same
    /// shape and lifecycle as <see cref="ApplyLaunchTracks"/> — set once right after open, since mpv applies
    /// sid/aid as the file loads. Same value convention: -1 = off/none, &gt;= 1 = a real mpv track, null =
    /// nothing remembered -> leave mpv's default. A launch preselect runs after this and overrides it.</summary>
    private void ApplyRememberedTracks()
    {
        if (_resumeSubId is int s)
        {
            if (s < 0) Vm.SetSubtitleOff();
            else if (s >= 1) Vm.SelectSubtitleId(s);
        }
        if (_resumeAudioId is int a)
        {
            if (a < 0) Vm.SetAudioOff();
            else if (a >= 1) Vm.SelectAudioId(a);
        }
        _resumeSubId = _resumeAudioId = null; // apply once per open
    }

    /// <summary>Load a local path or URL into the engine. Never throws to the caller — a failed open
    /// surfaces a toast (a genuine decode/format failure later arrives as an EndFile(Error) toast).</summary>
    public void OpenMedia(string pathOrUrl, bool fromStart = false)
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
            // Show the loading spinner until the first frame (or a load error) arrives — vital for slow network
            // sources, which would otherwise sit on the welcome screen with no feedback. HasMedia was already
            // false, so OnOpening's reset doesn't re-fire ApplyMediaPresence; call it here to reveal the overlay.
            _loading = true;
            LoadingName.Text = DisplayNameFor(pathOrUrl);
            _loadWatchdog.Stop();
            _loadWatchdog.Start(); // (re)arm the never-hang backstop for this open
            ApplyMediaPresence();
            _reachedEnd = false;   // fresh file: not finished until it plays through to its own EOF
            _explicitResume = null; // a launch resume belongs only to its launch file (the two launch paths set it
                                    // again right after this call); clearing here drops a stale value if that file
                                    // never reported a Duration, so the next normal open isn't force-seeked.
            _openGeneration++;     // invalidate any in-flight chapter-warm pass for the previous file
            _panelTabUserChosen = false; // a new file re-defaults the panel tab (audio -> Up Next, video -> Chapters)
            ResetLyricsData();     // drop the prior file's lyrics; an open overlay reloads when the new tags arrive
            // resume only when the user keeps that on (Settings -> Playback) and didn't ask to start over
            // (History's "Play from start"); applied on the first Duration
            var record = _history.Get(pathOrUrl);
            _resumeTarget = (!fromStart && App.Settings.Current.ResumePlayback ? record?.Position : null) ?? -1;
            // Remember the per-file subtitle/audio track choice from a previous viewing. Independent of the
            // resume position (and of ResumePlayback / "Play from start"): null = none recorded -> leave mpv's
            // default. Applied right after open, mirroring the launch preselect; a launch --sub/--audio override
            // (ApplyLaunchTracks, called after OpenMedia returns) still wins by running last.
            _resumeSubId = record?.SubtitleId;
            _resumeAudioId = record?.AudioId;
            Vm.SetSpeed(App.Settings.Current.DefaultSpeed); // every file starts at the default speed, incl. 1x
                                                            // (so a manual speed change doesn't carry over)
            ApplySubtitleDefaults(); // default sub size/position (Settings -> Subtitles)
            ApplyAudioDefaults();    // loudness normalization (Settings -> Audio)
            ApplyRememberedTracks(); // reselect the remembered sub/audio track (mpv honours sid/aid as the file loads)
            LoadBookmarks();       // refresh the panel's bookmarks for the new file (panel may be open)
            LoadUserChapters();    // feed the file's user-added chapters in (merge with the file's own)
            RevealChrome();        // show the controls when a file opens (drag-drop / picker)
            _thumbReady = _thumbs.OpenAsync(pathOrUrl); // arm the seek-preview engine; the warm awaits this task
            WarmChapterThumbnails();           // preemptively fill the chapter-thumbnail cache in the background
            UpdatePlaylist(pathOrUrl);        // (re)build the folder-as-playlist around this file
        }
        catch (Exception)
        {
            _loading = false;     // synchronous open failure — drop the spinner with the idle surface
            _loadWatchdog.Stop();
            ShowToast("Couldn't open this file");
            ApplyMediaPresence(); // restore the idle surface (e.g. the welcome shelf after a failed History resume)
        }
    }

    /// <summary>The label shown under the loading spinner: a stream URL in full (trimming handles overflow),
    /// or just the file name for a local path.</summary>
    private static string DisplayNameFor(string pathOrUrl)
    {
        if (pathOrUrl.Contains("://", StringComparison.Ordinal))
            return pathOrUrl;
        try { return System.IO.Path.GetFileName(pathOrUrl); }
        catch { return pathOrUrl; }
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
        // A fresh folder is needed. Enumerating it can block on a slow/dead network mount (NFS/SMB), and doing
        // that on the UI thread would freeze the dispatcher — the marshaled file-loaded event could never run,
        // so the file would never actually start (the loading spinner would hang forever). Scan off the UI
        // thread instead; the playlist fills in a moment later. Until then, treat playback as single-file.
        _playlist = null;
        _ = BuildFolderPlaylistAsync(key, _openGeneration);
    }

    /// <summary>Enumerate the file's folder off the UI thread and build the folder-as-playlist around it, then
    /// marshal the result back. A generation guard drops a stale scan if a newer open superseded it; the
    /// null-playlist guard yields to a playlist another path (e.g. a recursive folder drop) set meanwhile.</summary>
    private async Task BuildFolderPlaylistAsync(string key, int gen)
    {
        string? dir = System.IO.Path.GetDirectoryName(key);
        if (dir is null)
            return;
        System.Collections.Generic.List<string>? siblings = await Task.Run(() =>
        {
            try
            {
                var list = new System.Collections.Generic.List<string>();
                foreach (var f in System.IO.Directory.EnumerateFiles(dir))
                    if (OkPlayer.Core.MediaFormats.IsMedia(f))
                        list.Add(f);
                return list;
            }
            catch { return (System.Collections.Generic.List<string>?)null; } // unreadable folder — stay single-file
        });
        if (gen != _openGeneration || siblings is null || _playlist is not null)
            return; // superseded by a newer open, unreadable, or another path already set the playlist
        _playlist = new OkPlayer.Core.Playlist(siblings, key) { Repeat = _repeat, Shuffle = _shuffle };
        RebuildUpNext();
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
                IsWatched = _history.Get(p) is { Finished: true } or { Position: > 60 }, // watched to end, or seen a minute in
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
    /// or are URLs, and play the first. The file read and the per-entry <c>File.Exists</c> checks run off the
    /// UI thread: a playlist of network (NFS/SMB) paths would otherwise block the dispatcher on each stat and
    /// freeze the app — the exact hang reported when opening a saved .m3u of network files. While the entries
    /// are validated we show the loading spinner so the click gives feedback.</summary>
    private async void OpenM3u(string m3uPath)
    {
        // Feedback during the off-thread read/validate gap (can be seconds on a slow mount). OpenMedia re-arms
        // its own spinner + watchdog once we hand it the first entry.
        _loading = true;
        LoadingName.Text = DisplayNameFor(m3uPath);
        _loadWatchdog.Stop();
        _loadWatchdog.Start();
        ApplyMediaPresence();
        int gen = ++_openGeneration; // a newer open (file/folder/another playlist) supersedes this load

        System.Collections.Generic.List<string> valid;
        try
        {
            valid = await Task.Run(() =>
            {
                var entries = OkPlayer.Core.M3u.Parse(System.IO.File.ReadAllText(m3uPath), System.IO.Path.GetDirectoryName(m3uPath));
                var v = new System.Collections.Generic.List<string>();
                foreach (var entry in entries)
                    if (entry.Contains("://") || System.IO.File.Exists(entry))
                        v.Add(entry);
                return v;
            }); // resumes on the UI thread (DispatcherQueue sync-context) to touch the playlist + XAML
        }
        catch
        {
            if (gen == _openGeneration) FailPlaylistOpen("Couldn't open this playlist");
            return;
        }

        if (gen != _openGeneration)
            return; // superseded by a newer open/close while we were validating — that generation now owns the
                    // loading state (OpenMedia re-armed it, CloseFile cleared it), so we must NOT touch it here

        if (valid.Count == 0)
        {
            FailPlaylistOpen("Empty playlist");
            return;
        }

        _shuffle = false; // an .m3u defines its own order — honor it rather than shuffle it away
        _playlist = new OkPlayer.Core.Playlist(valid, valid[0], sort: false) { Repeat = _repeat };
        // A playlist launch (OkPlayer.exe playlist.m3u --resume <s>) parks its exact resume in _explicitResume
        // after the outer OpenMedia(m3u) returned — but that resume targets the playlist's FIRST entry, and the
        // inner OpenMedia below resets per-open state (clearing _explicitResume). Capture it across the open so
        // the first entry still honours the launch resume. Null in every non-launch open, so this is a no-op there.
        double? launchResume = _explicitResume;
        OpenMedia(valid[0]); // plays; UpdatePlaylist's SetCurrent keeps this list rather than the folder
        _explicitResume = launchResume;
        Vm.Play();
    }

    /// <summary>Tear down the loading spinner and surface a toast when a playlist open can't proceed.</summary>
    private void FailPlaylistOpen(string message)
    {
        _loading = false;
        _loadWatchdog.Stop();
        ShowToast(message);
        ApplyMediaPresence();
    }

    private void OnChaptersTab(object sender, TappedRoutedEventArgs e) { _panelTabUserChosen = true; SetPanelTab(false); }
    private void OnUpNextTab(object sender, TappedRoutedEventArgs e) { _panelTabUserChosen = true; SetPanelTab(true); }

    /// <summary>True when the open file is audio (by extension) — used to default the side panel to Up Next,
    /// since chapters are meaningless for music.</summary>
    private bool IsCurrentAudio() => _currentPath is { } p && OkPlayer.Core.MediaFormats.IsAudio(p);

    /// <summary>Pick the default panel tab for the current file: Up Next for audio or any file without chapters
    /// (chapters are meaningless there), the Chapters tab for video that has them. Both tabs stay available — the
    /// Chapters tab also hosts the file's bookmarks and the "Bookmark here" action, which are independent of
    /// chapters, so it must never be hidden. Honors a manual tab tap for the current file
    /// (<see cref="_panelTabUserChosen"/>, reset on each open).</summary>
    private void RefreshPanelTabs()
    {
        if (_panelTabUserChosen)
            return; // respect the user's explicit pick for this file
        SetPanelTab(IsCurrentAudio() || Vm.Chapters.Count == 0); // audio / no chapters -> Up Next; else Chapters
    }

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
        // Only act on a genuine end-of-file. A queued eof-reached can arrive after a manual hop (PageDown /
        // opening another file) loaded a fresh file at position 0 — that stale event must neither skip a file
        // nor mark anything watched. A real EOF leaves position at (≈) duration.
        bool atRealEof = Vm.Duration > 0 && Vm.Position >= Vm.Duration - 1.0;
        if (atRealEof)
        {
            _reachedEnd = true;
            SaveProgress(); // latch Finished now — before any auto-advance swaps the current file out
        }
        if (atRealEof && _autoAdvance && _playlist?.AutoAdvanceTarget is string next)
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

    private void OnDragOver(object sender, DragEventArgs e)
    {
        var data = e.DataView;
        // Accept files/folders and links (a URL/text dragged from a browser).
        if (!data.Contains(StandardDataFormats.StorageItems)
            && !data.Contains(StandardDataFormats.WebLink)
            && !data.Contains(StandardDataFormats.Text))
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
        // Deliberately NO GetStorageItemsAsync() here. A link dragged from a browser is also offered as a
        // VIRTUAL ".url" file (FileGroupDescriptor), and materializing that inside the modal drag loop
        // deadlocks — the app hangs for the whole drag. The filename preview isn't worth that; "Drop to play"
        // is enough. The dropped item is resolved in OnDrop, after the drag loop ends.
    }

    private void OnDragLeave(object sender, DragEventArgs e) => HideDragOverlay();

    private void HideDragOverlay()
    {
        DragOverlay.Visibility = Visibility.Collapsed;
    }

    /// <summary>Drop-a-folder → playlist: scan the folder for media (recursively, depth-bounded — see
    /// <see cref="OkPlayer.Core.FolderScan"/>), build the playlist, and start the first file. The scan runs off
    /// the UI thread because a deep or network folder can take a moment.</summary>
    private async Task OpenFolderAsPlaylist(string folderPath)
    {
        // A large or network folder can take a while to scan off-thread. If the user opens or drops something
        // else meanwhile, that newer action must win — claim a generation up front and bail if it's superseded,
        // so a slow scan can't silently replace whatever is playing by the time it finishes. (_openGeneration is
        // UI-thread-only and bumped by OpenMedia/CloseFile; the continuation resumes on the UI thread.)
        int gen = ++_openGeneration;
        var media = await Task.Run(() => OkPlayer.Core.FolderScan.MediaFiles(folderPath));
        if (gen != _openGeneration)
            return; // a newer open/drop took over while we were scanning — don't clobber it
        if (media.Count == 0)
        {
            ShowToast("No media in that folder");
            return;
        }
        OpenMedia(media[0]); // builds the immediate-folder playlist and handles a failed open via its own catch
        if (_currentPath != media[0])
            return; // the first file wouldn't open (OpenMedia toasted + restored the idle surface) — don't leave
                    // the player on a playlist rooted on a file that never played
        // Override the immediate-folder playlist OpenMedia just built with the full recursive scan (subfolders
        // included) and surface it in the Up-Next panel.
        _shuffle = false; // a folder defines a natural order — honor it rather than shuffle it away
        _playlist = new OkPlayer.Core.Playlist(media, media[0], sort: false) { Repeat = _repeat };
        RebuildUpNext();
        Vm.Play();
    }

    private async void OnDrop(object sender, DragEventArgs e)
    {
        HideDragOverlay();
        var data = e.DataView;
        // async void: a transient DataView access can throw — never let it escape to the UI thread.
        var deferral = e.GetDeferral();
        // Resolve WHAT was dropped first, release the OLE drop, and only THEN open it. Opening can do slow work
        // (a network folder/file), and anything still running inside the deferral keeps the source's drag loop
        // alive — which freezes the drag ghost on screen system-wide. Decide here; act after Complete().
        string? openUrl = null, openFile = null, openFolder = null, addSub = null;
        try
        {
            // Resolve a dragged LINK first. A browser link drag also exposes a virtual ".url" file via
            // StorageItems, so reaching for StorageItems would open the shortcut file (or stall materializing
            // it) instead of the actual address — resolve the URL up front and open it as a stream.
            openUrl = await TryGetDroppedUrlAsync(data);
            if (openUrl is null && data.Contains(StandardDataFormats.StorageItems))
            {
                var items = await data.GetStorageItemsAsync();
                var file = items.OfType<StorageFile>().FirstOrDefault();
                if (file is not null)
                {
                    // A subtitle dropped onto a playing video loads as a track rather than replacing the media.
                    if (Vm.HasMedia && OkPlayer.Core.MediaFormats.IsSubtitle(file.Path))
                        addSub = file.Path;
                    else
                        openFile = file.Path;
                }
                else if (items.OfType<StorageFolder>().FirstOrDefault() is { } folder)
                    openFolder = folder.Path;
                else if (items.Count > 0)
                    ShowToast("Drop a media file or folder");
            }
            else if (openUrl is null)
            {
                ShowToast("Drop a media file, folder, or link");
            }
        }
        catch (Exception)
        {
            ShowToast("Couldn't open dropped item");
        }
        finally
        {
            deferral.Complete(); // release the OLE drop now — the drag ghost can't linger while the open runs
        }

        // Open AFTER the deferral completes, so a slow (network) open never holds the source's drag loop open.
        try
        {
            if (openUrl is not null) OpenMedia(openUrl);
            else if (addSub is not null) AddSubtitle(addSub);
            else if (openFile is not null) OpenMedia(openFile);
            else if (openFolder is not null) await OpenFolderAsPlaylist(openFolder); // recursive playlist, play first
        }
        catch (Exception)
        {
            ShowToast("Couldn't open dropped item");
        }
    }

    /// <summary>Resolve a dragged link to a URL string: prefer an explicit WebLink, else URL-like text (a browser
    /// link drag carries the address as text too). Returns null when there is no link — a plain file/folder drop
    /// or arbitrary non-URL text — so the caller falls through to file handling. Never throws.</summary>
    private static async Task<string?> TryGetDroppedUrlAsync(DataPackageView data)
    {
        try
        {
            if (data.Contains(StandardDataFormats.WebLink))
            {
                var uri = await data.GetWebLinkAsync();
                if (uri is not null)
                    return uri.ToString();
            }
        }
        catch { /* fall through to text */ }
        try
        {
            if (data.Contains(StandardDataFormats.Text))
            {
                string text = (await data.GetTextAsync() ?? string.Empty).Trim();
                if (OkPlayer.Core.MediaFormats.IsPlayableUrl(text)) // a single absolute URL, not a paragraph
                    return text;
            }
        }
        catch { /* not a link */ }
        return null;
    }

    /// <summary>Ctrl+V: open a URL or local file path from the clipboard ("paste a URL"/path). A no-op with a
    /// gentle toast when the clipboard holds neither, so a stray paste doesn't disrupt playback.</summary>
    private async void OnPasteAccelerator(KeyboardAccelerator sender, KeyboardAcceleratorInvokedEventArgs args)
    {
        // Don't hijack Ctrl+V from a focused text field — the History search box, or any TextBox — let it paste
        // there. Only the bare player surface turns a pasted URL/path into an open. Return BEFORE marking the
        // accelerator handled, or the focused field never receives the paste.
        if (_historyOpen || (XamlRoot is not null && FocusManager.GetFocusedElement(XamlRoot) is TextBox))
            return;
        args.Handled = true;
        try
        {
            var data = Windows.ApplicationModel.DataTransfer.Clipboard.GetContent();
            if (data.Contains(StandardDataFormats.Text))
            {
                string text = (await data.GetTextAsync() ?? string.Empty).Trim().Trim('"');
                // A URL opens immediately. A filesystem path is stat'd OFF the UI thread — a dead SMB mount would
                // freeze the dispatcher on File.Exists — and opened only if it resolves, so a typo'd/dead path
                // toasts instead of becoming the current media.
                if (text.Length > 0 && OkPlayer.Core.MediaFormats.IsPlayableUrl(text))
                {
                    OpenMedia(text);
                    return;
                }
                if (text.Length > 0 && await System.Threading.Tasks.Task.Run(() => System.IO.File.Exists(text)))
                {
                    OpenMedia(text);
                    return;
                }
            }
            ShowToast("Clipboard has no link or file path");
        }
        catch { ShowToast("Couldn't read the clipboard"); }
    }
}
