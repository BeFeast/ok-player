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

    [ObservableProperty] private double _position;          // seconds
    [ObservableProperty] private double _duration;          // seconds
    [ObservableProperty] private bool _isPaused = true;
    [ObservableProperty] private double _volume = 100;      // 0..130
    [ObservableProperty] private bool _isMuted;
    [ObservableProperty] private double _speed = 1.0;
    [ObservableProperty] private bool _showRemaining;       // total vs remaining
    [ObservableProperty] private string _mediaTitle = string.Empty;
    [ObservableProperty] private bool _hasMedia;

    public bool IsPlaying => !IsPaused;
    public double PositionFraction => Duration > 0 ? Position / Duration : 0;
    public string CurrentTimeText => FormatTime(Position);
    public string DurationText => FormatTime(Duration);
    public string TrailingTimeText => ShowRemaining ? "-" + FormatTime(System.Math.Max(0, Duration - Position)) : FormatTime(Duration);
    public string SpeedText => Speed.ToString("0.00", CultureInfo.InvariantCulture) + Glyph(0x00D7); // "1.00×"

    // Segoe Fluent Icons glyphs that flip with state (built from code points to avoid source-encoding issues).
    public string PlayPauseGlyph => IsPaused ? Glyph(0xE768) : Glyph(0xE769); // Play / Pause
    public string VolumeGlyph => IsMuted ? Glyph(0xE74F) : Glyph(0xE767);     // Mute / Volume

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

    /// <summary>Wire the VM to the engine: observe the properties the surface needs.</summary>
    public void Attach(MpvContext engine, DispatcherQueue dispatcher)
    {
        _engine = engine;
        _dispatcher = dispatcher;
        foreach (var (name, fmt) in new (string, MpvFormat)[]
        {
            ("time-pos", MpvFormat.Double), ("duration", MpvFormat.Double), ("pause", MpvFormat.Flag),
            ("volume", MpvFormat.Double), ("mute", MpvFormat.Flag), ("speed", MpvFormat.Double),
            ("media-title", MpvFormat.String),
        })
        {
            engine.ObserveProperty(name, fmt);
        }
        engine.PropertyChanged += OnEngineProperty;
    }

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
        => _engine?.Command("seek", seconds.ToString(CultureInfo.InvariantCulture), "relative");

    public void FrameStep(bool forward) => _engine?.Command(forward ? "frame-step" : "frame-back-step");

    public void JumpChapter(int delta)
        => _engine?.Command("add", "chapter", delta.ToString(CultureInfo.InvariantCulture));

    public void NudgeVolume(double delta)
    {
        if (_engine is not { } e) return;
        e.SetProperty("volume", System.Math.Clamp((e.GetPropertyDouble("volume") ?? 100) + delta, 0, 130));
    }

    public void ToggleMute()
    {
        if (_engine is not { } e) return;
        e.SetProperty("mute", !(e.GetPropertyBool("mute") ?? false));
    }

    public void SetSpeed(double speed) => _engine?.SetProperty("speed", speed);

    public void CycleSpeed()
    {
        double[] steps = { 0.5, 0.75, 1.0, 1.25, 1.5, 2.0 };
        int i = System.Array.FindIndex(steps, s => s >= Speed - 0.001);
        SetSpeed(steps[(i + 1) % steps.Length]);
    }

    public void TakeScreenshot() => _engine?.Command("screenshot");

    public void ToggleTimeLabel() => ShowRemaining = !ShowRemaining;

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
