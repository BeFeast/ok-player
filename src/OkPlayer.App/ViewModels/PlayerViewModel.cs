using System.Collections.ObjectModel;
using System.Globalization;
using CommunityToolkit.Mvvm.ComponentModel;
using Microsoft.UI.Dispatching;
using OkPlayer.Mpv;
using OkPlayer.Mpv.Interop;

namespace OkPlayer.App.ViewModels;

/// <summary>
/// The Main Player state model (per the interaction handoff section 6). Observes libmpv properties
/// and surfaces them as bindable state; commands flow back to the engine. UI-thread affine: engine
/// property changes arrive on the pump thread and are marshalled onto the dispatcher.
/// </summary>
public partial class PlayerViewModel : ObservableObject
{
    private MpvContext? _engine;
    private DispatcherQueue? _dispatcher;

    /// <summary>True while the user is scrubbing — suppresses time-pos echo so the thumb doesn't fight the drag.</summary>
    public bool IsScrubbing { get; set; }

    /// <summary>Emitted for transient OSD toasts (volume, mute, speed, screenshot, seek readout).</summary>
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

    public ObservableCollection<TrackInfo> SubtitleTracks { get; } = new();
    public ObservableCollection<TrackInfo> AudioTracks { get; } = new();
    public ObservableCollection<ChapterInfo> Chapters { get; } = new();

    public bool IsPlaying => !IsPaused;
    public double PositionFraction => Duration > 0 ? Position / Duration : 0;
    public string CurrentTimeText => FormatTime(Position);
    public string DurationText => FormatTime(Duration);
    public string TrailingTimeText => ShowRemaining ? "-" + FormatTime(System.Math.Max(0, Duration - Position)) : FormatTime(Duration);
    public string SpeedText => Speed.ToString("0.00", CultureInfo.InvariantCulture) + Glyph(0x00D7); // "1.00×"

    // Segoe Fluent Icons glyphs that flip with state (built from code points to avoid source-encoding issues).
    public string PlayPauseGlyph => IsPaused ? Glyph(0xE768) : Glyph(0xE769); // Play / Pause
    public string VolumeGlyph => IsMuted ? Glyph(0xE74F) : Glyph(0xE767);     // Mute / Volume
    public string SubDelayText => $"{SubDelayMs:+0;-0;0} ms";

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

    /// <summary>Wire the VM to the engine: observe the properties the surface needs.</summary>
    public void Attach(MpvContext engine, DispatcherQueue dispatcher)
    {
        _engine = engine;
        _dispatcher = dispatcher;
        foreach (var (name, fmt) in new (string, MpvFormat)[]
        {
            ("time-pos", MpvFormat.Double), ("duration", MpvFormat.Double), ("pause", MpvFormat.Flag),
            ("volume", MpvFormat.Double), ("mute", MpvFormat.Flag), ("speed", MpvFormat.Double),
            ("media-title", MpvFormat.String), ("sid", MpvFormat.String), ("aid", MpvFormat.String),
            ("sub-delay", MpvFormat.Double), ("chapter", MpvFormat.Int64),
        })
        {
            engine.ObserveProperty(name, fmt);
        }
        engine.PropertyChanged += OnEngineProperty;
        engine.FileLoaded += OnFileLoaded;
    }

    private void OnFileLoaded(object? sender, System.EventArgs e)
        => _dispatcher?.TryEnqueue(() => { RefreshTracks(); RefreshChapters(); });

    private void OnEngineProperty(string name)
    {
        MpvContext? e = _engine;
        DispatcherQueue? d = _dispatcher;
        if (e is null || d is null)
            return;

        d.TryEnqueue(() =>
        {
            switch (name)
            {
                case "time-pos": if (!IsScrubbing) Position = e.GetPropertyDouble("time-pos") ?? 0; break;
                case "duration": Duration = e.GetPropertyDouble("duration") ?? 0; break;
                case "pause": IsPaused = e.GetPropertyBool("pause") ?? true; break;
                case "volume": Volume = e.GetPropertyDouble("volume") ?? 100; break;
                case "mute": IsMuted = e.GetPropertyBool("mute") ?? false; break;
                case "speed": Speed = e.GetPropertyDouble("speed") ?? 1.0; break;
                case "media-title":
                    MediaTitle = e.GetPropertyString("media-title") ?? string.Empty;
                    HasMedia = true;
                    break;
                case "sid":
                case "aid": RefreshTracks(); break;
                case "sub-delay": SubDelayMs = (int)System.Math.Round((e.GetPropertyDouble("sub-delay") ?? 0) * 1000); break;
                case "chapter": CurrentChapterIndex = (int)(e.GetPropertyLong("chapter") ?? -1); break;
            }
        });
    }

    // ---- commands (Main Player handoff sections 4-5) ----

    public void TogglePlay()
    {
        if (_engine is not { } e) return;
        e.SetProperty("pause", !(e.GetPropertyBool("pause") ?? false));
    }

    public void SeekToFraction(double fraction)
    {
        if (_engine is not { } e || Duration <= 0) return;
        double seconds = System.Math.Clamp(fraction, 0, 1) * Duration;
        Position = seconds;
        e.Command("seek", seconds.ToString(CultureInfo.InvariantCulture), "absolute");
    }

    public void SeekRelative(double seconds)
    {
        _engine?.Command("seek", seconds.ToString(CultureInfo.InvariantCulture), "relative");
        double target = Duration > 0 ? System.Math.Clamp(Position + seconds, 0, Duration) : Position + seconds;
        ToastRequested?.Invoke(FormatTime(target));
    }

    public void FrameStep(bool forward) => _engine?.Command(forward ? "frame-step" : "frame-back-step");

    public void JumpChapter(int delta)
        => _engine?.Command("add", "chapter", delta.ToString(CultureInfo.InvariantCulture));

    public void NudgeVolume(double delta)
    {
        if (_engine is not { } e) return;
        double v = System.Math.Clamp((e.GetPropertyDouble("volume") ?? 100) + delta, 0, 130);
        e.SetProperty("volume", v);
        ToastRequested?.Invoke($"Volume {v:0}%");
    }

    public void ToggleMute()
    {
        if (_engine is not { } e) return;
        bool m = !(e.GetPropertyBool("mute") ?? false);
        e.SetProperty("mute", m);
        ToastRequested?.Invoke(m ? "Muted" : "Unmuted");
    }

    public void SetSpeed(double speed) => _engine?.SetProperty("speed", speed);

    public void CycleSpeed()
    {
        double[] steps = { 0.5, 0.75, 1.0, 1.25, 1.5, 2.0 };
        int i = System.Array.FindIndex(steps, s => s >= Speed - 0.001);
        double next = steps[(i + 1) % steps.Length];
        SetSpeed(next);
        ToastRequested?.Invoke($"{next:0.00}{Glyph(0x00D7)}");
    }

    public void TakeScreenshot()
    {
        // Async: a vo=libmpv screenshot needs a render, so a synchronous command on the render thread
        // would deadlock.
        _engine?.CommandAsync("screenshot");
        ToastRequested?.Invoke("Screenshot saved");
    }

    public void ToggleTimeLabel() => ShowRemaining = !ShowRemaining;

    // ---- tracks & chapters ----

    private void RefreshTracks()
    {
        if (_engine is not { } e) return;
        SubtitleTracks.Clear();
        AudioTracks.Clear();
        long count = e.GetPropertyLong("track-list/count") ?? 0;
        for (long i = 0; i < count; i++)
        {
            string? type = e.GetPropertyString($"track-list/{i}/type");
            long id = e.GetPropertyLong($"track-list/{i}/id") ?? 0;
            bool selected = e.GetPropertyBool($"track-list/{i}/selected") ?? false;
            bool external = e.GetPropertyBool($"track-list/{i}/external") ?? false;
            string? title = e.GetPropertyString($"track-list/{i}/title");
            string? lang = e.GetPropertyString($"track-list/{i}/lang");
            string name = !string.IsNullOrEmpty(title) ? title! : !string.IsNullOrEmpty(lang) ? lang! : $"Track {id}";

            string check = selected ? Glyph(0x2713) + "  " : string.Empty; // leading check on the active track
            if (type == "sub")
            {
                string ext = external ? $"   {Glyph(0x00B7)} EXT" : string.Empty;
                SubtitleTracks.Add(new TrackInfo { Id = id, Selected = selected, External = external, Label = check + name + ext });
            }
            else if (type == "audio")
            {
                var parts = new System.Collections.Generic.List<string> { name };
                string? channels = e.GetPropertyString($"track-list/{i}/audio-channels");
                string? codec = e.GetPropertyString($"track-list/{i}/codec");
                if (!string.IsNullOrEmpty(channels)) parts.Add(channels!);
                if (!string.IsNullOrEmpty(codec)) parts.Add(codec!.ToUpperInvariant());
                AudioTracks.Add(new TrackInfo { Id = id, Selected = selected, Label = check + string.Join($" {Glyph(0x00B7)} ", parts) });
            }
        }
        SubtitleOff = (e.GetPropertyString("sid") ?? "no") == "no";
    }

    private void RefreshChapters()
    {
        if (_engine is not { } e) return;
        Chapters.Clear();
        long count = e.GetPropertyLong("chapter-list/count") ?? 0;
        for (int i = 0; i < count; i++)
        {
            double time = e.GetPropertyDouble($"chapter-list/{i}/time") ?? 0;
            string? title = e.GetPropertyString($"chapter-list/{i}/title");
            Chapters.Add(new ChapterInfo
            {
                Index = i,
                Time = time,
                TimeText = FormatTime(time),
                Title = string.IsNullOrEmpty(title) ? $"Chapter {i + 1}" : title!,
            });
        }
    }

    public void SetSubtitleOff() => _engine?.SetProperty("sid", "no");
    public void SelectSubtitle(TrackInfo track) => _engine?.SetProperty("sid", track.Id.ToString(CultureInfo.InvariantCulture));
    public void SelectAudio(TrackInfo track) => _engine?.SetProperty("aid", track.Id.ToString(CultureInfo.InvariantCulture));
    public void SeekToChapter(ChapterInfo chapter) => _engine?.SetProperty("chapter", chapter.Index.ToString(CultureInfo.InvariantCulture));

    public void NudgeSubDelay(int ms)
    {
        if (_engine is not { } e) return;
        double next = (e.GetPropertyDouble("sub-delay") ?? 0) + ms / 1000.0;
        e.SetProperty("sub-delay", next);
        ToastRequested?.Invoke($"Subtitle delay {next * 1000:+0;-0;0} ms");
    }

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
