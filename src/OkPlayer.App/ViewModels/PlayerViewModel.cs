using System.Collections.Generic;
using System.Collections.ObjectModel;
using System.Globalization;
using System.Linq;
using CommunityToolkit.Mvvm.ComponentModel;
using Microsoft.UI.Dispatching;
using OkPlayer.Core;
using OkPlayer.Mpv;
using OkPlayer.Mpv.Interop;

namespace OkPlayer.App.ViewModels;

/// <summary>
/// The Main Player state model. Observes libmpv properties (values are read from the event payload,
/// never via Get*Property on the UI thread — that can deadlock a briefly-busy core). Track/chapter
/// enumeration reads are done on the pump thread and only the finished lists are marshalled to the UI.
/// </summary>
public partial class PlayerViewModel : ObservableObject
{
    private MpvContext? _engine;
    private DispatcherQueue? _dispatcher;
    private bool _awaitingSeek; // drop stale time-pos until the post-seek PlaybackRestart

    /// <summary>True while the user is scrubbing — suppresses time-pos echo so the thumb doesn't fight the drag.</summary>
    public bool IsScrubbing { get; set; }

    /// <summary>Emitted for transient OSD toasts (volume, mute, speed, screenshot, seek readout, errors).</summary>
    public event System.Action<string>? ToastRequested;

    [ObservableProperty] private double _position;          // seconds
    [ObservableProperty] private double _duration;          // seconds
    [ObservableProperty] private bool _isPaused = true;
    [ObservableProperty] private double _volume = 100;      // 0..130
    [ObservableProperty] private bool _isMuted;
    [ObservableProperty] private double _speed = 1.0;
    [ObservableProperty] private bool _showRemaining;       // total vs remaining
    [ObservableProperty] private string _mediaTitle = string.Empty;
    [ObservableProperty] private bool _hasMedia;
    [ObservableProperty] private bool _subtitleOff = true;
    [ObservableProperty] private bool _secondarySubtitleOff = true;
    [ObservableProperty] private bool _canUseSecondarySubtitle; // ≥2 sub tracks: gates the secondary picker
    [ObservableProperty] private int _subDelayMs;
    [ObservableProperty] private double _subScale = 1.0;
    public string SubScaleText => $"{SubScale * 100:0}%";
    [ObservableProperty] private int _currentChapterIndex = -1;
    [ObservableProperty] private int _videoWidth;   // mpv dwidth (display resolution)
    [ObservableProperty] private int _videoHeight;  // mpv dheight
    [ObservableProperty] private double _bufferedFraction; // demuxer cache extent, 0..1

    public ObservableCollection<TrackInfo> SubtitleTracks { get; } = new();
    /// <summary>The same subtitle tracks as <see cref="SubtitleTracks"/>, but with selection reflecting the
    /// SECONDARY slot (mpv <c>secondary-sid</c>) — a second caption shown at the same time, at the top.</summary>
    public ObservableCollection<TrackInfo> SecondarySubtitleTracks { get; } = new();
    public ObservableCollection<TrackInfo> AudioTracks { get; } = new();
    public ObservableCollection<AudioDevice> AudioDevices { get; } = new();
    public ObservableCollection<ChapterInfo> Chapters { get; } = new();

    /// <summary>The currently selected PRIMARY subtitle / audio track id, cached on the UI thread from the
    /// last <see cref="ApplyTracks"/> so the save path can read the user's choice without an mpv read on the
    /// UI thread (which can deadlock a busy core). Convention matches <c>FileRecord</c>: <c>-1</c> = off/none,
    /// <c>&gt;= 1</c> = the mpv track id. Set once a file has loaded; defaults to off until then.</summary>
    public int? CurrentSubtitleId { get; private set; }
    public int? CurrentAudioId { get; private set; }

    public bool IsPlaying => !IsPaused;
    public double PositionFraction => Duration > 0 ? Position / Duration : 0;
    public string CurrentTimeText => FormatTime(Position);
    public string DurationText => FormatTime(Duration);
    public string TrailingTimeText => ShowRemaining ? "-" + FormatTime(System.Math.Max(0, Duration - Position)) : FormatTime(Duration);
    public string SpeedText => Speed.ToString("0.00", CultureInfo.InvariantCulture) + Glyph(0x00D7);
    public string SubDelayText => $"{SubDelayMs:+0;-0;0} ms";
    public string VolumeText => $"{Volume:0}%";
    public double VolumeFillWidth => System.Math.Clamp(Volume / 130.0, 0, 1) * 54; // inline 54px OSC volume bar
    public string PlayPauseGlyph => IsPaused ? Glyph(0xE768) : Glyph(0xE769);
    public string VolumeGlyph => IsMuted ? Glyph(0xE74F) : Glyph(0xE767);

    // Volume UX (design "Volume"): boost above 100% reads amber; muted dims and shows a "Muted" readout.
    public bool VolumeIsBoost => Volume > 100 && !IsMuted;
    public string VolumeReadout => IsMuted ? "Muted" : $"{Volume:0}%";
    public Microsoft.UI.Xaml.Media.Brush VolumeFillBrush => IsMuted ? VolMutedBrush : VolumeIsBoost ? VolBoostBrush : VolWhiteBrush;
    public Microsoft.UI.Xaml.Media.Brush VolumeGlyphBrush => IsMuted ? VolMutedBrush : VolumeIsBoost ? VolBoostBrush : VolWhiteBrush;
    public Microsoft.UI.Xaml.Media.Brush VolumeReadoutBrush => IsMuted ? VolMutedBrush : VolumeIsBoost ? VolBoostBrush : VolReadoutBrush;

    private static readonly Microsoft.UI.Xaml.Media.SolidColorBrush VolWhiteBrush = new(Microsoft.UI.Colors.White);
    private static readonly Microsoft.UI.Xaml.Media.SolidColorBrush VolBoostBrush = new(Windows.UI.Color.FromArgb(0xFF, 0xF0, 0xB8, 0x40));
    private static readonly Microsoft.UI.Xaml.Media.SolidColorBrush VolMutedBrush = new(Windows.UI.Color.FromArgb(0x73, 0xFF, 0xFF, 0xFF));
    private static readonly Microsoft.UI.Xaml.Media.SolidColorBrush VolReadoutBrush = new(Windows.UI.Color.FromArgb(0xD9, 0xFF, 0xFF, 0xFF));

    private static string Glyph(int codePoint) => char.ConvertFromUtf32(codePoint);

    partial void OnPositionChanged(double value)
    {
        OnPropertyChanged(nameof(PositionFraction));
        OnPropertyChanged(nameof(CurrentTimeText));
        OnPropertyChanged(nameof(TrailingTimeText));
        UpdateCurrentChapter();
    }

    partial void OnDurationChanged(double value)
    {
        OnPropertyChanged(nameof(PositionFraction));
        OnPropertyChanged(nameof(DurationText));
        OnPropertyChanged(nameof(TrailingTimeText));
        OnPropertyChanged(nameof(ChapterFractions)); // fractions depend on duration
        OnPropertyChanged(nameof(AbLoopAFraction));  // A–B band positions depend on duration too
        OnPropertyChanged(nameof(AbLoopBFraction));
    }

    partial void OnIsPausedChanged(bool value)
    {
        OnPropertyChanged(nameof(IsPlaying));
        OnPropertyChanged(nameof(PlayPauseGlyph));
    }

    partial void OnIsMutedChanged(bool value)
    {
        OnPropertyChanged(nameof(VolumeGlyph));
        OnPropertyChanged(nameof(VolumeIsBoost));
        OnPropertyChanged(nameof(VolumeReadout));
        OnPropertyChanged(nameof(VolumeFillBrush));
        OnPropertyChanged(nameof(VolumeGlyphBrush));
        OnPropertyChanged(nameof(VolumeReadoutBrush));
    }
    partial void OnShowRemainingChanged(bool value) => OnPropertyChanged(nameof(TrailingTimeText));
    partial void OnSpeedChanged(double value) => OnPropertyChanged(nameof(SpeedText));
    partial void OnSubDelayMsChanged(int value) => OnPropertyChanged(nameof(SubDelayText));
    partial void OnSubScaleChanged(double value) => OnPropertyChanged(nameof(SubScaleText));
    partial void OnCanUseSecondarySubtitleChanged(bool value) => OnPropertyChanged(nameof(SecondarySubtitleVisibility));

    /// <summary>Collapses the secondary-subtitle picker unless the file has ≥2 subtitle tracks.</summary>
    public Microsoft.UI.Xaml.Visibility SecondarySubtitleVisibility =>
        CanUseSecondarySubtitle ? Microsoft.UI.Xaml.Visibility.Visible : Microsoft.UI.Xaml.Visibility.Collapsed;

    partial void OnCurrentChapterIndexChanged(int value) => ApplyCurrentChapterFlags();

    private void ApplyCurrentChapterFlags()
    {
        for (int i = 0; i < Chapters.Count; i++)
            Chapters[i].IsCurrent = i == CurrentChapterIndex;
    }
    partial void OnVolumeChanged(double value)
    {
        OnPropertyChanged(nameof(VolumeText));
        OnPropertyChanged(nameof(VolumeFillWidth));
        OnPropertyChanged(nameof(VolumeIsBoost));
        OnPropertyChanged(nameof(VolumeReadout));
        OnPropertyChanged(nameof(VolumeFillBrush));
        OnPropertyChanged(nameof(VolumeGlyphBrush));
        OnPropertyChanged(nameof(VolumeReadoutBrush));
    }

    public void Attach(MpvContext engine, DispatcherQueue dispatcher)
    {
        Detach(); // idempotent: never double-subscribe or strand a prior engine
        _engine = engine;
        _dispatcher = dispatcher;
        foreach (var (name, fmt) in new (string, MpvFormat)[]
        {
            ("time-pos", MpvFormat.Double), ("duration", MpvFormat.Double), ("pause", MpvFormat.Flag),
            ("volume", MpvFormat.Double), ("mute", MpvFormat.Flag), ("speed", MpvFormat.Double),
            ("media-title", MpvFormat.String), ("sid", MpvFormat.String), ("aid", MpvFormat.String),
            ("secondary-sid", MpvFormat.String),
            ("sub-delay", MpvFormat.Double), ("sub-scale", MpvFormat.Double), ("chapter", MpvFormat.Int64),
            ("dwidth", MpvFormat.Int64), ("dheight", MpvFormat.Int64),
            ("demuxer-cache-time", MpvFormat.Double), ("eof-reached", MpvFormat.Flag),
            ("ab-loop-a", MpvFormat.String), ("ab-loop-b", MpvFormat.String),
        })
        {
            engine.ObserveProperty(name, fmt);
        }
        engine.PropertyChanged += OnEngineProperty;
        engine.FileLoaded += OnFileLoaded;
        engine.EndFile += OnEndFile;
        engine.PlaybackRestart += OnPlaybackRestart;
        engine.CommandReply += OnVmCommandReply;

        // Capture libmpv's version once, off the UI thread (a constant property — safe to read off-thread,
        // and it dodges the render-thread blocking-call guard), for the Settings → Advanced "About" block.
        if (App.MpvVersion is null)
            System.Threading.Tasks.Task.Run(() =>
            {
                try
                {
                    string? v = engine.GetPropertyString("mpv-version");
                    if (!string.IsNullOrWhiteSpace(v))
                        App.MpvVersion = v;
                }
                catch { /* version is cosmetic — never fault attach over it */ }
            });
    }

    public void Detach()
    {
        if (_engine is { } e)
        {
            e.PropertyChanged -= OnEngineProperty;
            e.FileLoaded -= OnFileLoaded;
            e.EndFile -= OnEndFile;
            e.PlaybackRestart -= OnPlaybackRestart;
            e.CommandReply -= OnVmCommandReply;
        }
        _engine = null;
        _dispatcher = null;
    }

    // ---- engine events (raised on the pump thread) ----

    private void OnFileLoaded(object? sender, System.EventArgs e)
    {
        // Read track/chapter metadata here (pump thread); marshal only the finished lists to the UI.
        var subs = new List<TrackInfo>();
        var auds = new List<TrackInfo>();
        var secondarySubs = new List<TrackInfo>();
        var (subOff, secondaryOff) = ReadTracks(subs, auds, secondarySubs);
        var chapters = ReadChapters();
        _dispatcher?.TryEnqueue(() =>
        {
            _awaitingSeek = false;
            HasMedia = true;
            ApplyTracks(subs, auds, subOff, secondarySubs, secondaryOff);
            ApplyChapters(chapters);
            if (CurrentChapterIndex >= Chapters.Count)
                CurrentChapterIndex = -1; // a shorter/empty new file must not keep the prior index
            OnPropertyChanged(nameof(CurrentChapterIndex)); // re-sync the highlight after repopulation
            ApplyCurrentChapterFlags(); // partial setter doesn't fire on the manual notify above
        });
    }

    /// <summary>Reset transient playback state at load-request time (before mpv reports the new file's
    /// duration/chapter), so the previous file's playhead can't bleed into the new file's first frames.
    /// Done here rather than on FileLoaded so it can't clobber a duration the pump already delivered.</summary>
    public void OnOpening()
    {
        Position = 0;
        Duration = 0;
        CurrentChapterIndex = -1;
        _fileChapters = new();
        _userChapters = new();
        Chapters.Clear();
        OnPropertyChanged(nameof(ChapterFractions));
        // Re-arm even when replacing an already-playing file, so the next FileLoaded flips HasMedia
        // false->true and re-fires the ready-time chrome reveal / idle countdown.
        HasMedia = false;
        _awaitingSeek = false;
        ResetVideoAdjustments(force: false); // a rotation/aspect/fill tweak shouldn't carry into the next file
    }

    /// <summary>Raised (on the UI thread) when playback reaches the natural end of the current file —
    /// signalled by the eof-reached property, since keep-open suppresses the end-file event at EOF. The
    /// view uses it to auto-advance the folder playlist; the last frame is otherwise held.</summary>
    public event Action? EndReached;

    private void OnEndFile(object? sender, MpvEndFileReason reason)
    {
        if (reason != MpvEndFileReason.Error)
            return; // EOF holds the last frame (keep-open); only a real failure clears to the empty state
        _dispatcher?.TryEnqueue(() =>
        {
            HasMedia = false;
            MediaTitle = string.Empty;
            ToastRequested?.Invoke("Couldn't play this file");
        });
    }

    private void OnPlaybackRestart(object? sender, System.EventArgs e)
        => _dispatcher?.TryEnqueue(() => _awaitingSeek = false);

    private void OnEngineProperty(string name, object? value)
    {
        DispatcherQueue? d = _dispatcher;
        if (d is null)
            return;

        if (name is "sid" or "aid" or "secondary-sid")
        {
            // selection changed: re-read the track list off the UI thread, then apply on it.
            var subs = new List<TrackInfo>();
            var auds = new List<TrackInfo>();
            var secondarySubs = new List<TrackInfo>();
            var (subOff, secondaryOff) = ReadTracks(subs, auds, secondarySubs);
            d.TryEnqueue(() => ApplyTracks(subs, auds, subOff, secondarySubs, secondaryOff));
            return;
        }

        // Scalars: the value is parsed from the event — never call Get*Property here.
        d.TryEnqueue(() =>
        {
            switch (name)
            {
                case "time-pos": if (!IsScrubbing && !_awaitingSeek && value is double tp) Position = tp; break;
                case "duration": if (value is double du) Duration = du; break;
                case "pause": if (value is bool pa) IsPaused = pa; break;
                case "eof-reached": if (value is bool eof && eof) EndReached?.Invoke(); break; // natural EOF (keep-open)
                case "volume": if (value is double vo) Volume = vo; break;
                case "mute": if (value is bool mu) IsMuted = mu; break;
                case "speed": if (value is double sp) Speed = sp; break;
                case "media-title": MediaTitle = value as string ?? string.Empty; break;
                case "sub-delay": if (value is double sd) SubDelayMs = (int)System.Math.Round(sd * 1000); break;
                case "sub-scale": if (value is double ss) SubScale = ss; break;
                // "chapter" is still observed, but CurrentChapterIndex is derived from playhead time so it
                // matches our merged (file + user) re-indexed list, where the engine's index no longer lines up.
                case "dwidth": if (value is long dw) VideoWidth = (int)dw; break;
                case "dheight": if (value is long dh) VideoHeight = (int)dh; break;
                case "demuxer-cache-time": if (value is double ct) BufferedFraction = Duration > 0 ? System.Math.Clamp(ct / Duration, 0, 1) : 0; break;
                case "ab-loop-a": _abA = ParseAbLoop(value as string); OnPropertyChanged(nameof(AbLoopAFraction)); ScheduleAbAnnounce(); break;
                case "ab-loop-b": _abB = ParseAbLoop(value as string); OnPropertyChanged(nameof(AbLoopBFraction)); ScheduleAbAnnounce(); break;
            }
        });
    }

    // ---- off-thread reads (libmpv's client API is thread-safe; we never touch the render context here) ----

    private (bool SubOff, bool SecondaryOff) ReadTracks(List<TrackInfo> subs, List<TrackInfo> auds, List<TrackInfo> secondarySubs)
    {
        if (_engine is not { } e)
            return (true, true);
        // The SECONDARY subtitle is matched by id against secondary-sid (it's set to a concrete id, never
        // "auto"). The PRIMARY uses track-list/selected — which also resolves an "auto"/default selection mpv
        // makes for us — minus the track that's the secondary (a secondary track also reports selected=yes,
        // and would otherwise show a stray checkmark in the primary list).
        string secondarySid = e.GetPropertyString("secondary-sid") ?? "no";
        bool anyPrimary = false;
        long count = e.GetPropertyLong("track-list/count") ?? 0;
        for (long i = 0; i < count; i++)
        {
            string? type = e.GetPropertyString($"track-list/{i}/type");
            long id = e.GetPropertyLong($"track-list/{i}/id") ?? 0;
            bool selected = e.GetPropertyBool($"track-list/{i}/selected") ?? false;
            bool external = e.GetPropertyBool($"track-list/{i}/external") ?? false;
            string? title = e.GetPropertyString($"track-list/{i}/title");
            string? lang = e.GetPropertyString($"track-list/{i}/lang");
            string baseName = !string.IsNullOrEmpty(title) ? title! : !string.IsNullOrEmpty(lang) ? lang! : $"Track {id}";

            if (type == "sub")
            {
                bool isSecondary = secondarySid == id.ToString(CultureInfo.InvariantCulture);
                bool isPrimary = selected && !isSecondary; // 'selected' covers an auto/default pick; exclude the secondary track
                if (isPrimary) anyPrimary = true;
                string ext = external ? $"   {Glyph(0x00B7)} EXT" : string.Empty;
                subs.Add(new TrackInfo { Id = id, Selected = isPrimary, External = external, Label = Check(isPrimary) + baseName + ext });
                secondarySubs.Add(new TrackInfo { Id = id, Selected = isSecondary, External = external, Label = Check(isSecondary) + baseName + ext });
            }
            else if (type == "audio")
            {
                var parts = new List<string> { baseName };
                string? channels = e.GetPropertyString($"track-list/{i}/audio-channels");
                string? codec = e.GetPropertyString($"track-list/{i}/codec");
                if (!string.IsNullOrEmpty(channels)) parts.Add(channels!);
                if (!string.IsNullOrEmpty(codec)) parts.Add(codec!.ToUpperInvariant());
                auds.Add(new TrackInfo { Id = id, Selected = selected, Label = Check(selected) + string.Join($" {Glyph(0x00B7)} ", parts) });
            }
        }
        return (!anyPrimary, secondarySid == "no");
    }

    private static string Check(bool on) => on ? Glyph(0x2713) + "  " : string.Empty;

    /// <summary>Refresh the audio output device list off the UI thread (a libmpv property read on the
    /// render/UI thread can deadlock a busy core), then marshal the result onto the dispatcher. Cheap and
    /// idempotent — call it when the audio flyout opens.</summary>
    public void RefreshAudioDevices()
    {
        MpvContext? e = _engine;
        DispatcherQueue? d = _dispatcher;
        if (e is null || d is null)
            return;
        System.Threading.Tasks.Task.Run(() =>
        {
            var list = ReadAudioDevices(e);
            d.TryEnqueue(() =>
            {
                AudioDevices.Clear();
                foreach (var dev in list)
                    AudioDevices.Add(dev);
            });
        });
    }

    private static List<AudioDevice> ReadAudioDevices(MpvContext e)
    {
        var result = new List<AudioDevice>();
        long count = e.GetPropertyLong("audio-device-list/count") ?? 0;
        string current = e.GetPropertyString("audio-device") ?? "auto";
        bool sawAuto = false;
        for (long i = 0; i < count; i++)
        {
            string? name = e.GetPropertyString($"audio-device-list/{i}/name");
            if (string.IsNullOrEmpty(name))
                continue;
            if (name == "auto")
                sawAuto = true;
            string? desc = e.GetPropertyString($"audio-device-list/{i}/description");
            bool selected = name == current;
            string label = (selected ? Glyph(0x2713) + "  " : string.Empty) + (string.IsNullOrEmpty(desc) ? name! : desc!);
            result.Add(new AudioDevice { Name = name!, Selected = selected, Label = label });
        }
        // Always offer an explicit Automatic entry so the user can return to the system default — and so a
        // remembered specific device can be cleared back to auto. (mpv usually lists "auto" itself, but not
        // on every AO build.)
        if (!sawAuto)
        {
            bool autoSel = current is "auto" or "";
            result.Insert(0, new AudioDevice { Name = "auto", Selected = autoSel, Label = (autoSel ? Glyph(0x2713) + "  " : string.Empty) + "Automatic" });
        }
        return result;
    }

    /// <summary>Switch the audio output device (session-scoped; mpv's <c>audio-device</c> property). The
    /// flyout closes on pick and re-reads on its next open, so the checkmark refreshes then — no immediate
    /// re-read here (it would race the async set and read the stale device).</summary>
    public void SelectAudioDevice(string name) => Set("audio-device", name);

    /// <summary>Restore a remembered output device at startup, but only if it still exists — a saved USB/
    /// Bluetooth/HDMI device that's since been unplugged would otherwise be force-selected and could leave
    /// playback silent. "auto"/empty is always valid (mpv's default). Validates the list off the UI thread.</summary>
    public void RestoreAudioDevice(string name)
    {
        if (string.IsNullOrEmpty(name) || name == "auto")
            return; // already mpv's default — nothing to restore
        MpvContext? e = _engine;
        DispatcherQueue? d = _dispatcher;
        if (e is null || d is null)
            return;
        System.Threading.Tasks.Task.Run(() =>
        {
            bool present = false;
            foreach (var dev in ReadAudioDevices(e))
                if (dev.Name == name) { present = true; break; }
            if (present)
                d.TryEnqueue(() => Set("audio-device", name)); // gone → leave mpv on its default, no silent output
        });
    }

    private List<ChapterInfo> ReadChapters()
    {
        var result = new List<ChapterInfo>();
        if (_engine is not { } e)
            return result;
        long count = e.GetPropertyLong("chapter-list/count") ?? 0;
        for (int i = 0; i < count; i++)
        {
            double time = e.GetPropertyDouble($"chapter-list/{i}/time") ?? 0;
            string? title = e.GetPropertyString($"chapter-list/{i}/title");
            result.Add(new ChapterInfo
            {
                Index = i,
                Time = time,
                TimeText = FormatTime(time),
                Title = string.IsNullOrEmpty(title) ? $"Chapter {i + 1}" : title!,
            });
        }
        return result;
    }

    private void ApplyTracks(List<TrackInfo> subs, List<TrackInfo> auds, bool subOff,
                             List<TrackInfo> secondarySubs, bool secondaryOff)
    {
        SubtitleTracks.Clear();
        foreach (var t in subs) SubtitleTracks.Add(t);
        SecondarySubtitleTracks.Clear();
        foreach (var t in secondarySubs) SecondarySubtitleTracks.Add(t);
        AudioTracks.Clear();
        foreach (var t in auds) AudioTracks.Add(t);
        SubtitleOff = subOff;
        SecondarySubtitleOff = secondaryOff;
        // Cache the current primary-sub / audio selection for the save path (history remembers it per file).
        // -1 == off/none; otherwise the selected track's id. Read from the freshly-built lists, no mpv call.
        CurrentSubtitleId = subOff ? -1 : (int?)(subs.FirstOrDefault(t => t.Selected)?.Id) ?? -1;
        CurrentAudioId = (int?)(auds.FirstOrDefault(t => t.Selected)?.Id) ?? -1;
        // A second simultaneous subtitle is only meaningful with at least two tracks (e.g. native + a
        // learning language); below that, hide the secondary picker to keep the flyout calm — UNLESS a
        // secondary is already active (mpv can carry secondary-sid into a 1-track file), so the user always
        // keeps an Off control to clear it.
        CanUseSecondarySubtitle = subs.Count >= 2 || !secondaryOff;
    }

    private List<ChapterInfo> _fileChapters = new();
    private List<(double Time, string Title)> _userChapters = new();

    private void ApplyChapters(List<ChapterInfo> chapters)
    {
        _fileChapters = chapters;
        RebuildChapters();
    }

    /// <summary>Replace the user-authored chapters (from the sidecar) and re-merge with the file's own.</summary>
    public void SetUserChapters(IEnumerable<(double Time, string Title)> chapters)
    {
        _userChapters = new List<(double, string)>(chapters);
        RebuildChapters();
    }

    /// <summary>Merge the file's chapters (read-only) and the user's into one time-sorted, re-indexed list,
    /// carrying over already-decoded thumbnails by time so an edit doesn't reload them.</summary>
    private void RebuildChapters()
    {
        var thumbs = new Dictionary<long, Microsoft.UI.Xaml.Media.ImageSource?>();
        foreach (var c in Chapters)
            thumbs[(long)System.Math.Round(c.Time * 10)] = c.Thumbnail;

        var fileTuples = new List<(double, string)>(_fileChapters.Count);
        foreach (var c in _fileChapters)
            fileTuples.Add((c.Time, c.Title));

        Chapters.Clear();
        foreach (var m in ChapterMath.Merge(fileTuples, _userChapters)) // pure merge/sort/reindex (Core, tested)
        {
            var entry = new ChapterInfo { Index = m.Index, Time = m.Time, Title = m.Title, TimeText = FormatTime(m.Time), IsUserDefined = m.IsUserDefined };
            if (thumbs.TryGetValue((long)System.Math.Round(m.Time * 10), out var th))
                entry.Thumbnail = th; // carry over an already-decoded thumbnail so an edit doesn't reload it
            Chapters.Add(entry);
        }
        OnPropertyChanged(nameof(ChapterFractions));
        UpdateCurrentChapter();
    }

    private List<double> ChapterTimes()
    {
        var times = new List<double>(Chapters.Count);
        foreach (var c in Chapters)
            times.Add(c.Time);
        return times;
    }

    /// <summary>Chapter start positions as 0..1 fractions, for the seek-bar tick markers.</summary>
    public IReadOnlyList<double> ChapterFractions => ChapterMath.Fractions(ChapterTimes(), Duration);

    /// <summary>Pick the current chapter by playhead time (works for merged file + user chapters, where the
    /// engine's own chapter index no longer matches our re-indexed list).</summary>
    private void UpdateCurrentChapter()
    {
        int idx = ChapterMath.CurrentIndex(ChapterTimes(), Position);
        if (idx != CurrentChapterIndex)
            CurrentChapterIndex = idx;
    }

    // ---- commands (guarded so an mpv rejection never escapes a keyboard/click handler) ----

    // All commands go out async (mpv_command_async): a synchronous mpv_command blocks the UI thread until
    // the core accepts it, which deadlocks while the core is briefly busy (e.g. an in-flight screenshot).
    // The UI reacts to observed property events, not a command's return, so fire-and-forget is correct.
    private void Cmd(params string[] args)
    {
        if (_engine is { } e)
        {
            try { e.CommandAsync(args); } catch (MpvException) { }
        }
    }

    private bool CmdOk(params string[] args)
    {
        if (_engine is not { } e)
            return false;
        try { e.CommandAsync(args); return true; } catch (MpvException) { return false; } // true == accepted for dispatch
    }

    // Video-plane adjustments (menu-driven). Tracked locally because reading the live mpv values on the
    // UI thread can deadlock a busy core — we own every change, so a local mirror stays in sync.
    private int _videoRotate;        // 0 / 90 / 180 / 270
    private bool _fillScreen;        // panscan: crop to fill, removing letterbox/pillar bars
    private const string AspectAuto = "no"; // mpv's value for "use the file's own aspect" (the default)
    private string _aspectOverride = AspectAuto;

    /// <summary>Rotate the video plane 90° clockwise (cycles 0 → 90 → 180 → 270 → 0).</summary>
    public void RotateVideo()
    {
        _videoRotate = (_videoRotate + 90) % 360;
        Set("video-rotate", _videoRotate.ToString(CultureInfo.InvariantCulture));
    }

    /// <summary>Force a display aspect ratio ("no" restores the file's own); e.g. "16:9", "4:3", "2.35:1".</summary>
    public void SetAspect(string ratio)
    {
        _aspectOverride = ratio;
        Set("video-aspect-override", ratio);
    }

    /// <summary>Toggle pan-and-scan fill — crop the video to fill the window, removing black bars. Returns
    /// the new state so the caller can phrase the toast.</summary>
    public bool ToggleFillScreen()
    {
        _fillScreen = !_fillScreen;
        Set("panscan", _fillScreen ? 1.0 : 0.0);
        return _fillScreen;
    }

    /// <summary>Undo every video-plane adjustment (rotation, aspect override, fill).</summary>
    public void ResetVideoAdjustments() => ResetVideoAdjustments(force: true);

    private void ResetVideoAdjustments(bool force)
    {
        if (!force && _videoRotate == 0 && !_fillScreen && _aspectOverride == AspectAuto)
            return; // nothing to undo — don't spam the engine on every file open
        _videoRotate = 0;
        _fillScreen = false;
        _aspectOverride = AspectAuto;
        Set("video-rotate", "0");
        Set("panscan", 0.0);
        Set("video-aspect-override", AspectAuto);
    }

    private void Set(string name, string value)
    {
        if (_engine is { } e)
        {
            try { e.SetProperty(name, value); } catch (MpvException) { }
        }
    }

    private void Set(string name, double value)
    {
        if (_engine is { } e)
        {
            try { e.SetProperty(name, value); } catch (MpvException) { }
        }
    }

    private static string Inv(double v) => v.ToString(CultureInfo.InvariantCulture);

    // mpv-side cycle: authoritative even before the first pause event seeds IsPaused.
    public void TogglePlay() => Cmd("cycle", "pause");

    /// <summary>Resume playback (pause=no). Used after a playlist hop, since keep-open leaves the previous
    /// file paused at its end and that pause would otherwise carry into the newly loaded file.</summary>
    public void Play() => Cmd("set", "pause", "no");

    /// <summary>Stop playback and unload the current file: clears mpv's playlist, returns the engine to idle,
    /// and resets to the empty state (no title/tracks/chapters) so the view falls back to the Welcome card.
    /// The caller persists the resume position (SaveProgress) before calling this.</summary>
    public void CloseFile()
    {
        Cmd("stop"); // halt playback + clear the internal playlist; the engine goes idle (keep-open holds no frame)
        HasMedia = false;
        MediaTitle = string.Empty;
        Position = 0;
        Duration = 0;
        CurrentChapterIndex = -1;
        SubtitleTracks.Clear();
        SecondarySubtitleTracks.Clear();
        AudioTracks.Clear();
        Chapters.Clear();
        OnPropertyChanged(nameof(ChapterFractions));
    }

    public void SeekToFraction(double fraction)
    {
        if (_engine is null || Duration <= 0)
            return;
        double seconds = System.Math.Clamp(fraction, 0, 1) * Duration;
        Position = seconds;
        _awaitingSeek = true;
        if (!SeekCmd("seek", Inv(seconds), "absolute"))
            _awaitingSeek = false; // submit rejected up front
    }

    /// <summary>Seek to an absolute time in seconds (mpv clamps to the media bounds). Unlike
    /// <see cref="SeekToFraction"/> this needs no known duration, so it's safe before/while duration settles.</summary>
    public void SeekToSeconds(double seconds)
    {
        if (_engine is null)
            return;
        double s = System.Math.Max(0, seconds);
        Position = s;
        _awaitingSeek = true;
        if (!SeekCmd("seek", Inv(s), "absolute"))
            _awaitingSeek = false; // submit rejected up front
    }

    public void SeekRelative(double seconds)
    {
        _awaitingSeek = true;
        if (!SeekCmd("seek", Inv(seconds), "relative"))
        {
            _awaitingSeek = false;
            return;
        }
        double target = Duration > 0 ? System.Math.Clamp(Position + seconds, 0, Duration) : Position + seconds;
        ToastRequested?.Invoke(FormatTime(target));
    }

    // Distinct from MpvVideoPanel.ScreenshotReply (1) — both subscribe to the engine's CommandReply, so a
    // shared id would make every seek reply look like a finished screenshot ("Screenshot saved" on any seek).
    private const ulong SeekReply = 2; // tags seek commands so a rejected seek (non-seekable stream) can be caught

    /// <summary>Dispatch a seek async, tagged so <see cref="OnVmCommandReply"/> can clear the time-pos
    /// suppression if mpv later rejects it (success arrives via PlaybackRestart). Returns false only if the
    /// submit itself is refused (no engine).</summary>
    private bool SeekCmd(params string[] args)
    {
        if (_engine is not { } e)
            return false;
        try { e.CommandAsync(SeekReply, args); return true; } catch (MpvException) { return false; }
    }

    private void OnVmCommandReply(ulong id, bool success)
    {
        // A rejected seek never fires PlaybackRestart, so it would otherwise leave _awaitingSeek stuck and
        // freeze the time-pos echo (e.g. seeking a non-seekable live stream). Clear it on failure.
        if (id == SeekReply && !success)
            _dispatcher?.TryEnqueue(() => _awaitingSeek = false);
    }

    public void FrameStep(bool forward) => Cmd(forward ? "frame-step" : "frame-back-step");

    public void JumpChapter(int delta)
    {
        if (ChapterMath.JumpTarget(CurrentChapterIndex, delta, Chapters.Count) is int target)
            SeekToChapter(Chapters[target]); // null at the first/last chapter -> no rewind
    }

    public void NudgeVolume(double delta)
    {
        // "add" lets mpv clamp to volume-max and stays correct under rapid presses (no stale cache).
        if (CmdOk("add", "volume", Inv(delta)))
            ToastRequested?.Invoke($"Volume {System.Math.Clamp(Volume + delta, 0, 130):0}%");
    }

    public void ToggleMute()
    {
        bool willMute = !IsMuted;
        if (CmdOk("cycle", "mute"))
            ToastRequested?.Invoke(willMute ? "Muted" : "Unmuted");
    }

    public void SetVolume(double value) => Set("volume", System.Math.Clamp(value, 0, 130));

    private double? _abA, _abB; // A–B loop points in seconds (null = unset), tracked from the ab-loop-a/b observes

    /// <summary>A–B loop start/end as 0..1 fractions for the seek bar, or NaN when unset.</summary>
    public double AbLoopAFraction => _abA is double a && Duration > 0 ? System.Math.Clamp(a / Duration, 0, 1) : double.NaN;
    public double AbLoopBFraction => _abB is double b && Duration > 0 ? System.Math.Clamp(b / Duration, 0, 1) : double.NaN;

    private static double? ParseAbLoop(string? s) =>
        s is null || s == "no" ? null
        : double.TryParse(s, System.Globalization.NumberStyles.Float, System.Globalization.CultureInfo.InvariantCulture, out double v) ? v
        : null;

    // ab-loop has no toast here: the ab-loop-a/b observes are authoritative (mpv sets each point at its real
    // playback position), so the toast + seek-bar region are driven from them. That avoids both a stale predicted
    // state on rapid re-toggles and a drifted time when a seek is still pending.
    public void ToggleAbLoop() => CmdOk("ab-loop");

    private int _abAnnounceVersion;  // coalesces the a/b observes of one toggle into a single toast
    private bool _abWasActive;       // suppresses a spurious "cleared" toast on load (both points start unset)

    private void ScheduleAbAnnounce()
    {
        int v = ++_abAnnounceVersion;
        // Defer so a clear (both a→no and b→no) collapses into one announce of the final state.
        _dispatcher?.TryEnqueue(() => { if (v == _abAnnounceVersion) AnnounceAbLoop(); });
    }

    private void AnnounceAbLoop()
    {
        bool active = _abA is not null || _abB is not null;
        string? msg = (_abA, _abB) switch
        {
            (not null, not null) => $"A–B loop: {FormatClock(_abA.Value)} – {FormatClock(_abB.Value)}",
            (not null, null)     => $"A–B loop: start at {FormatClock(_abA.Value)}",
            (null, not null)     => $"A–B loop: end at {FormatClock(_abB.Value)}",
            _                    => _abWasActive ? "A–B loop cleared" : null, // no toast on the initial unset state
        };
        _abWasActive = active;
        if (msg is not null)
            ToastRequested?.Invoke(msg);
    }

    private static string FormatClock(double seconds)
    {
        var ts = System.TimeSpan.FromSeconds(System.Math.Max(0, seconds));
        return ts.TotalHours >= 1 ? $"{(int)ts.TotalHours}:{ts.Minutes:00}:{ts.Seconds:00}" : $"{ts.Minutes}:{ts.Seconds:00}";
    }

    public void SetSpeed(double speed) => Set("speed", speed);

    public void CycleSpeed()
    {
        double[] steps = { 0.5, 0.75, 1.0, 1.25, 1.5, 2.0 };
        int i = System.Array.FindIndex(steps, s => s >= Speed - 0.001);
        double next = i < 0 ? steps[^1] : steps[(i + 1) % steps.Length]; // above-table speed -> top step, not slowest
        SetSpeed(next);
        ToastRequested?.Invoke($"{next:0.00}{Glyph(0x00D7)}");
    }

    public void TakeScreenshot()
    {
        if (_engine is not { } e)
            return;
        // "video" mode grabs the decoded frame directly — no render dependency, so it can't stall.
        try { e.CommandAsync("screenshot", "video"); } catch (MpvException) { return; }
        ToastRequested?.Invoke("Screenshot saved");
    }

    public void SetSubtitleOff() => Set("sid", "no");
    public void SelectSubtitle(TrackInfo track) => Set("sid", track.Id.ToString(CultureInfo.InvariantCulture));

    // Secondary subtitle (mpv secondary-sid): a second caption shown at once, rendered at the top by default.
    public void SetSecondarySubtitleOff() => Set("secondary-sid", "no");
    public void SelectSecondarySubtitle(TrackInfo track) => Set("secondary-sid", track.Id.ToString(CultureInfo.InvariantCulture));
    public void SelectAudio(TrackInfo track) => Set("aid", track.Id.ToString(CultureInfo.InvariantCulture));

    // Track selection by raw id — used for launch-time preselection (mpv applies sid/aid as the file loads).
    public void SelectSubtitleId(int id) => Set("sid", id.ToString(CultureInfo.InvariantCulture));
    public void SelectAudioId(int id) => Set("aid", id.ToString(CultureInfo.InvariantCulture));
    public void SetAudioOff() => Set("aid", "no");
    public void SeekToChapter(ChapterInfo chapter)
    {
        if (Duration > 0)
            SeekToFraction(chapter.Time / Duration); // seek by time so user-added chapters work too
    }

    public void NudgeSubDelay(int ms)
    {
        if (CmdOk("add", "sub-delay", Inv(ms / 1000.0)))
            ToastRequested?.Invoke($"Subtitle delay {SubDelayMs + ms:+0;-0;0} ms");
    }

    public void NudgeSubScale(double delta)
    {
        if (CmdOk("add", "sub-scale", Inv(delta)))
            ToastRequested?.Invoke($"Subtitle size {System.Math.Clamp(SubScale + delta, 0.2, 4.0) * 100:0}%");
    }

    public void ToggleTimeLabel() => ShowRemaining = !ShowRemaining;

    /// <summary>Minimum percentage points to raise subtitles by while the OSC chrome is up, so the controls
    /// never overlap captions (PRD P1-D9). Driven through <c>sub-pos</c>, not <c>sub-margin-y</c>: libass
    /// ignores the margin for ASS subtitles, so the old margin toggle silently failed on every ASS track (e.g.
    /// embedded SDH) — sub-pos moves every subtitle kind. This is the floor for large surfaces; the View
    /// raises it on small ones (where a fixed % is too few pixels) via <see cref="OkPlayer.Core.SubtitleLift"/>.
    /// Verified by SubtitleOscClearanceTests.</summary>
    public const double OscSubtitleLift = 16;

    /// <summary>Position subtitles via <c>sub-pos</c> (a percentage; 100 = bottom). <paramref name="basePos"/>
    /// is the user's configured position; <paramref name="lift"/> is how far to raise it (0 when the chrome is
    /// hidden, else the OSC-clearance lift the View computes for the current surface size). Works for ASS, text
    /// and bitmap subtitles alike (unlike <c>sub-margin-y</c>, which libass ignores for ASS).</summary>
    public void ApplySubtitlePosition(double basePos, double lift)
        => Set("sub-pos", System.Math.Max(0, basePos - lift));

    private static string FormatTime(double seconds)
    {
        if (double.IsNaN(seconds) || seconds < 0)
            seconds = 0;
        var ts = System.TimeSpan.FromSeconds(seconds);
        return ts.TotalHours >= 1
            ? $"{(int)ts.TotalHours}:{ts.Minutes:00}:{ts.Seconds:00}"
            : $"{ts.Minutes}:{ts.Seconds:00}";
    }
}
