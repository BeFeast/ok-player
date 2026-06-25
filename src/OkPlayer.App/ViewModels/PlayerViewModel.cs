using System.Collections.Generic;
using System.Collections.ObjectModel;
using System.Globalization;
using CommunityToolkit.Mvvm.ComponentModel;
using Microsoft.UI.Dispatching;
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
    [ObservableProperty] private int _subDelayMs;
    [ObservableProperty] private int _currentChapterIndex = -1;
    [ObservableProperty] private int _videoWidth;   // mpv dwidth (display resolution)
    [ObservableProperty] private int _videoHeight;  // mpv dheight
    [ObservableProperty] private double _bufferedFraction; // demuxer cache extent, 0..1

    public ObservableCollection<TrackInfo> SubtitleTracks { get; } = new();
    public ObservableCollection<TrackInfo> AudioTracks { get; } = new();
    public ObservableCollection<ChapterInfo> Chapters { get; } = new();

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

    private static string Glyph(int codePoint) => char.ConvertFromUtf32(codePoint);

    partial void OnPositionChanged(double value)
    {
        OnPropertyChanged(nameof(PositionFraction));
        OnPropertyChanged(nameof(CurrentTimeText));
        OnPropertyChanged(nameof(TrailingTimeText));
    }

    partial void OnDurationChanged(double value)
    {
        OnPropertyChanged(nameof(PositionFraction));
        OnPropertyChanged(nameof(DurationText));
        OnPropertyChanged(nameof(TrailingTimeText));
    }

    partial void OnIsPausedChanged(bool value)
    {
        OnPropertyChanged(nameof(IsPlaying));
        OnPropertyChanged(nameof(PlayPauseGlyph));
    }

    partial void OnIsMutedChanged(bool value) => OnPropertyChanged(nameof(VolumeGlyph));
    partial void OnShowRemainingChanged(bool value) => OnPropertyChanged(nameof(TrailingTimeText));
    partial void OnSpeedChanged(double value) => OnPropertyChanged(nameof(SpeedText));
    partial void OnSubDelayMsChanged(int value) => OnPropertyChanged(nameof(SubDelayText));

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
            ("sub-delay", MpvFormat.Double), ("chapter", MpvFormat.Int64),
            ("dwidth", MpvFormat.Int64), ("dheight", MpvFormat.Int64),
            ("demuxer-cache-time", MpvFormat.Double),
        })
        {
            engine.ObserveProperty(name, fmt);
        }
        engine.PropertyChanged += OnEngineProperty;
        engine.FileLoaded += OnFileLoaded;
        engine.EndFile += OnEndFile;
        engine.PlaybackRestart += OnPlaybackRestart;
    }

    public void Detach()
    {
        if (_engine is { } e)
        {
            e.PropertyChanged -= OnEngineProperty;
            e.FileLoaded -= OnFileLoaded;
            e.EndFile -= OnEndFile;
            e.PlaybackRestart -= OnPlaybackRestart;
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
        bool subOff = ReadTracks(subs, auds);
        var chapters = ReadChapters();
        _dispatcher?.TryEnqueue(() =>
        {
            _awaitingSeek = false;
            HasMedia = true;
            ApplyTracks(subs, auds, subOff);
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
        // Re-arm even when replacing an already-playing file, so the next FileLoaded flips HasMedia
        // false->true and re-fires the ready-time chrome reveal / idle countdown.
        HasMedia = false;
        _awaitingSeek = false;
    }

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

        if (name is "sid" or "aid")
        {
            // selection changed: re-read the track list off the UI thread, then apply on it.
            var subs = new List<TrackInfo>();
            var auds = new List<TrackInfo>();
            bool subOff = ReadTracks(subs, auds);
            d.TryEnqueue(() => ApplyTracks(subs, auds, subOff));
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
                case "volume": if (value is double vo) Volume = vo; break;
                case "mute": if (value is bool mu) IsMuted = mu; break;
                case "speed": if (value is double sp) Speed = sp; break;
                case "media-title": MediaTitle = value as string ?? string.Empty; break;
                case "sub-delay": if (value is double sd) SubDelayMs = (int)System.Math.Round(sd * 1000); break;
                case "chapter": CurrentChapterIndex = value is long ch ? (int)ch : -1; break;
                case "dwidth": if (value is long dw) VideoWidth = (int)dw; break;
                case "dheight": if (value is long dh) VideoHeight = (int)dh; break;
                case "demuxer-cache-time": if (value is double ct) BufferedFraction = Duration > 0 ? System.Math.Clamp(ct / Duration, 0, 1) : 0; break;
            }
        });
    }

    // ---- off-thread reads (libmpv's client API is thread-safe; we never touch the render context here) ----

    private bool ReadTracks(List<TrackInfo> subs, List<TrackInfo> auds)
    {
        if (_engine is not { } e)
            return true;
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
            string check = selected ? Glyph(0x2713) + "  " : string.Empty;

            if (type == "sub")
            {
                string ext = external ? $"   {Glyph(0x00B7)} EXT" : string.Empty;
                subs.Add(new TrackInfo { Id = id, Selected = selected, External = external, Label = check + baseName + ext });
            }
            else if (type == "audio")
            {
                var parts = new List<string> { baseName };
                string? channels = e.GetPropertyString($"track-list/{i}/audio-channels");
                string? codec = e.GetPropertyString($"track-list/{i}/codec");
                if (!string.IsNullOrEmpty(channels)) parts.Add(channels!);
                if (!string.IsNullOrEmpty(codec)) parts.Add(codec!.ToUpperInvariant());
                auds.Add(new TrackInfo { Id = id, Selected = selected, Label = check + string.Join($" {Glyph(0x00B7)} ", parts) });
            }
        }
        return (e.GetPropertyString("sid") ?? "no") == "no";
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

    private void ApplyTracks(List<TrackInfo> subs, List<TrackInfo> auds, bool subOff)
    {
        SubtitleTracks.Clear();
        foreach (var t in subs) SubtitleTracks.Add(t);
        AudioTracks.Clear();
        foreach (var t in auds) AudioTracks.Add(t);
        SubtitleOff = subOff;
    }

    private void ApplyChapters(List<ChapterInfo> chapters)
    {
        Chapters.Clear();
        foreach (var c in chapters) Chapters.Add(c);
    }

    // ---- commands (guarded so an mpv rejection never escapes a keyboard/click handler) ----

    private void Cmd(params string[] args)
    {
        if (_engine is { } e)
        {
            try { e.Command(args); } catch (MpvException) { }
        }
    }

    private bool CmdOk(params string[] args)
    {
        if (_engine is not { } e)
            return false;
        try { e.Command(args); return true; } catch (MpvException) { return false; }
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

    public void SeekToFraction(double fraction)
    {
        if (_engine is null || Duration <= 0)
            return;
        double seconds = System.Math.Clamp(fraction, 0, 1) * Duration;
        Position = seconds;
        _awaitingSeek = true;
        if (!CmdOk("seek", Inv(seconds), "absolute"))
            _awaitingSeek = false; // a failed seek must not freeze the time-pos echo
    }

    public void SeekRelative(double seconds)
    {
        _awaitingSeek = true;
        if (!CmdOk("seek", Inv(seconds), "relative"))
        {
            _awaitingSeek = false;
            return;
        }
        double target = Duration > 0 ? System.Math.Clamp(Position + seconds, 0, Duration) : Position + seconds;
        ToastRequested?.Invoke(FormatTime(target));
    }

    public void FrameStep(bool forward) => Cmd(forward ? "frame-step" : "frame-back-step");

    public void JumpChapter(int delta) => Cmd("add", "chapter", Inv(delta));

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

    public void ToggleAbLoop()
    {
        if (CmdOk("ab-loop"))
            ToastRequested?.Invoke("A-B loop");
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
    public void SelectAudio(TrackInfo track) => Set("aid", track.Id.ToString(CultureInfo.InvariantCulture));
    public void SeekToChapter(ChapterInfo chapter) => Set("chapter", chapter.Index.ToString(CultureInfo.InvariantCulture));

    public void NudgeSubDelay(int ms)
    {
        if (CmdOk("add", "sub-delay", Inv(ms / 1000.0)))
            ToastRequested?.Invoke($"Subtitle delay {SubDelayMs + ms:+0;-0;0} ms");
    }

    public void ToggleTimeLabel() => ShowRemaining = !ShowRemaining;

    /// <summary>Lift subtitles above the OSC while the chrome is visible (design: 56px ↔ 128px baseline).</summary>
    public void SetSubtitleMargin(bool chromeVisible) => Set("sub-margin-y", chromeVisible ? "128" : "56");

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
