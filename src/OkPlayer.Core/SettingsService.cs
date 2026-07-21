using System;
using System.IO;
using System.Text.Json;
using System.Text.Json.Nodes;
using System.Text.Json.Serialization;
using OkPlayer.Core;

namespace OkPlayer.App.Services;

/// <summary>User preferences. Only the keys whose controls are rendered in the design ship now;
/// later panels add their keys as they land. Defaults are the design's "smart defaults".</summary>
public sealed class AppSettings
{
    // Appearance (design band 9 — fully rendered)
    public string Theme { get; set; } = "Auto";        // "Light" | "Dark" | "Auto"
    public string AccentSource { get; set; } = "OkTeal"; // "System" | "OkTeal"

    // Playback (design band 9)
    public bool ResumePlayback { get; set; } = true;     // resume from the last position on open
    public bool HideControlsWhenPaused { get; set; } = true; // also auto-hide the OSC when paused, after the same idle timeout
    public double DefaultSpeed { get; set; } = 1.0;      // speed a newly opened file starts at
    public int SkipStep { get; set; } = 5;               // seconds the Left/Right arrows seek

    // Video
    public bool HardwareDecoding { get; set; } = true;   // hwdec auto-safe vs software (applied at engine init)
    public double Brightness { get; set; }               // global mpv picture controls, neutral = 0
    public double Contrast { get; set; }
    public double Saturation { get; set; }
    public double Gamma { get; set; }

    // Audio
    public int DefaultVolume { get; set; } = 100;        // 0..130, the volume the engine starts at
    public bool AudioNormalization { get; set; }         // loudness normalization (night mode) via an mpv audio filter
    public string AudioDevice { get; set; } = "";        // remembered output device (mpv id); "" = mpv's default (auto)

    // Subtitles
    public double SubtitleScale { get; set; } = 1.0;     // default sub-scale (size multiplier)
    public int SubtitlePosition { get; set; } = 100;     // default sub-pos: 100 = bottom, lower = higher
    public string SubtitleStyle { get; set; } = "Default"; // appearance preset key (OkPlayer.Core.SubtitleStyle)

    // Privacy (Integration panel)
    public int HistoryRetentionDays { get; set; } = 0;   // auto-prune history older than N days; 0 = keep forever

    // Updates (About panel) — Velopack auto-update. On by default for the beta program.
    public bool AutoCheckUpdates { get; set; } = true;   // check GitHub for a newer release on launch

    /// <summary>The shared <c>advanced.keybindings</c> config text; blank means all defaults.</summary>
    public string Keybindings { get; set; } = "";

    public int SchemaVersion { get; set; } = SettingsService.SchemaVersion;

    public double VideoAdjustment(VideoAdjustmentKind kind) => kind switch
    {
        VideoAdjustmentKind.Brightness => Brightness,
        VideoAdjustmentKind.Contrast => Contrast,
        VideoAdjustmentKind.Saturation => Saturation,
        VideoAdjustmentKind.Gamma => Gamma,
        _ => throw new ArgumentOutOfRangeException(nameof(kind), kind, null),
    };

    public void SetVideoAdjustment(VideoAdjustmentKind kind, double value)
    {
        value = VideoAdjustments.Normalize(value);
        switch (kind)
        {
            case VideoAdjustmentKind.Brightness: Brightness = value; break;
            case VideoAdjustmentKind.Contrast: Contrast = value; break;
            case VideoAdjustmentKind.Saturation: Saturation = value; break;
            case VideoAdjustmentKind.Gamma: Gamma = value; break;
            default: throw new ArgumentOutOfRangeException(nameof(kind), kind, null);
        }
    }
}

/// <summary>
/// App settings persisted as the shared, human-readable canonical JSON document at
/// %APPDATA%/OkPlayer/settings.json (no database). The public <see cref="AppSettings"/> facade keeps
/// existing Windows call sites simple while this service reads/writes the same sectioned v2 schema as
/// Linux and preserves fields the Windows shell does not yet understand.
/// </summary>
public sealed class SettingsService
{
    internal const int SchemaVersion = 2;

    private static readonly JsonSerializerOptions JsonOpts = new()
    {
        WriteIndented = true,
        DefaultIgnoreCondition = JsonIgnoreCondition.WhenWritingNull,
    };

    private readonly string? _path; // null when AppData is unreachable — runs without persistence
    private readonly object _lock = new();
    private JsonObject _document = NewCanonicalDocument();

    public AppSettings Current { get; private set; } = new();

    /// <summary>Raised after a save so listeners (player, windows) re-apply appearance changes.</summary>
    public event Action? Changed;

    public SettingsService()
    {
        try
        {
            string dir = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData), "OkPlayer");
            Directory.CreateDirectory(dir);
            _path = Path.Combine(dir, "settings.json");
        }
        catch { _path = null; } // can't reach AppData: run with in-memory defaults rather than failing
        Load();
    }

    /// <summary>Test seam: persist to a caller-supplied path instead of %APPDATA%.</summary>
    internal SettingsService(string? path)
    {
        _path = path;
        Load();
    }

    private void Load()
    {
        try
        {
            if (_path is null || !File.Exists(_path))
                return;
            string raw = File.ReadAllText(_path);
            JsonObject? document = JsonNode.Parse(raw) as JsonObject;
            if (document is null)
                return;

            if (document.ContainsKey("version"))
            {
                int version = ReadInt(document, "version") ?? 0;
                if (version is < 1 or > SchemaVersion)
                    return;
                _document = document;
                Current = ReadCanonical(document);
            }
            else if (document.ContainsKey("SchemaVersion"))
            {
                // Legacy Windows settings were flat PascalCase. Read them once, then Save writes the
                // canonical v2 shape so Linux and Windows can share this file without losing fields.
                var legacy = JsonSerializer.Deserialize<AppSettings>(raw, JsonOpts);
                if (legacy is not null)
                {
                    Current = legacy;
                    _document = NewCanonicalDocument();
                }
            }
        }
        catch { /* corrupt/unreadable settings are non-fatal — keep defaults */ }
    }

    /// <summary>Persist the current settings and notify listeners to re-apply.</summary>
    public void Save()
    {
        if (_path is not null)
        {
            string json;
            lock (_lock)
            {
                UpdateCanonical(_document, Current);
                json = _document.ToJsonString(JsonOpts);
            }
            string tmp = _path + ".tmp";
            // Files under %APPDATA% take brief shared locks from Defender and the Search indexer, which scan
            // each newly written file. That made the atomic replace throw a sharing violation and — because
            // the failure was swallowed — silently drop the save, which looked like "the System accent
            // reverts to teal after a restart". Retry across the transient lock so the save actually lands.
            for (int attempt = 0; ; attempt++)
            {
                try
                {
                    File.WriteAllText(tmp, json);
                    File.Move(tmp, _path, overwrite: true); // replace in one step so a crash can't truncate
                    break;
                }
                catch (Exception ex) when (attempt < 8 && ex is IOException or UnauthorizedAccessException)
                {
                    System.Threading.Thread.Sleep(30); // up to ~240ms; the scanner's lock clears well within this
                }
                catch
                {
                    break; // give up after the retries (or an unexpected error) — Save stays best-effort, never throws
                }
            }
        }
        Changed?.Invoke();
    }

    private static JsonObject NewCanonicalDocument() => new()
    {
        ["version"] = SchemaVersion,
        ["playback"] = new JsonObject(),
        ["audio"] = new JsonObject(),
        ["video"] = new JsonObject(),
        ["updates"] = new JsonObject(),
        ["advanced"] = new JsonObject(),
    };

    private static AppSettings ReadCanonical(JsonObject root)
    {
        var settings = new AppSettings { SchemaVersion = SchemaVersion };
        JsonObject playback = Section(root, "playback");
        JsonObject audio = Section(root, "audio");
        JsonObject video = Section(root, "video");
        JsonObject subtitles = Section(root, "subtitles");
        JsonObject appearance = Section(root, "appearance");
        JsonObject updates = Section(root, "updates");
        JsonObject advanced = Section(root, "advanced");
        JsonObject privacy = Section(root, "privacy");

        settings.DefaultVolume = (int)Math.Round(
            Math.Clamp(ReadDouble(playback, "volume") ?? settings.DefaultVolume, 0.0, 130.0));
        settings.ResumePlayback = ReadBool(playback, "resume") ?? settings.ResumePlayback;
        settings.DefaultSpeed = ReadDouble(playback, "default_speed") ?? settings.DefaultSpeed;
        settings.SkipStep = ReadInt(playback, "skip_step_seconds") ?? settings.SkipStep;
        settings.HideControlsWhenPaused =
            ReadBool(playback, "hide_controls_when_paused") ?? settings.HideControlsWhenPaused;

        settings.AudioNormalization = ReadBool(audio, "normalization") ?? settings.AudioNormalization;
        string audioDevice = (ReadString(audio, "device") ?? "").Trim();
        settings.AudioDevice = audioDevice is not ("" or "auto") ? audioDevice : "";

        string? hwdec = ReadString(video, "hwdec");
        settings.HardwareDecoding = hwdec switch
        {
            "no" => false,
            "auto-safe" => true,
            null => true,
            _ => false,
        };
        settings.Brightness = VideoAdjustments.Normalize(ReadDouble(video, "brightness") ?? 0.0);
        settings.Contrast = VideoAdjustments.Normalize(ReadDouble(video, "contrast") ?? 0.0);
        settings.Saturation = VideoAdjustments.Normalize(ReadDouble(video, "saturation") ?? 0.0);
        settings.Gamma = VideoAdjustments.Normalize(ReadDouble(video, "gamma") ?? 0.0);

        settings.SubtitleScale = ReadDouble(subtitles, "scale") ?? settings.SubtitleScale;
        settings.SubtitlePosition = ReadInt(subtitles, "position") ?? settings.SubtitlePosition;
        settings.SubtitleStyle = ReadString(subtitles, "style") ?? settings.SubtitleStyle;
        settings.Theme = ReadString(appearance, "theme") ?? settings.Theme;
        settings.AccentSource = ReadString(appearance, "accent_source") ?? settings.AccentSource;
        settings.AutoCheckUpdates = ReadBool(updates, "auto_check") ?? settings.AutoCheckUpdates;
        settings.Keybindings = ReadString(advanced, "keybindings") ?? "";
        settings.HistoryRetentionDays =
            ReadInt(privacy, "history_retention_days") ?? settings.HistoryRetentionDays;
        return settings;
    }

    private static void UpdateCanonical(JsonObject root, AppSettings settings)
    {
        root["version"] = SchemaVersion;
        JsonObject playback = EnsureSection(root, "playback");
        JsonObject audio = EnsureSection(root, "audio");
        JsonObject video = EnsureSection(root, "video");
        JsonObject subtitles = EnsureSection(root, "subtitles");
        JsonObject appearance = EnsureSection(root, "appearance");
        JsonObject updates = EnsureSection(root, "updates");
        JsonObject privacy = EnsureSection(root, "privacy");
        JsonObject advanced = EnsureSection(root, "advanced");

        playback["volume"] = Math.Clamp(settings.DefaultVolume, 0, 130);
        playback["resume"] = settings.ResumePlayback;
        playback["default_speed"] = settings.DefaultSpeed;
        playback["skip_step_seconds"] = settings.SkipStep;
        playback["hide_controls_when_paused"] = settings.HideControlsWhenPaused;

        audio["normalization"] = settings.AudioNormalization;
        SetOptional(
            audio,
            "device",
            string.IsNullOrWhiteSpace(settings.AudioDevice) ? null : settings.AudioDevice.Trim());

        video["hwdec"] = settings.HardwareDecoding ? "auto-safe" : "no";
        SetVideoAdjustment(video, "brightness", settings.Brightness);
        SetVideoAdjustment(video, "contrast", settings.Contrast);
        SetVideoAdjustment(video, "saturation", settings.Saturation);
        SetVideoAdjustment(video, "gamma", settings.Gamma);

        subtitles["scale"] = settings.SubtitleScale;
        subtitles["position"] = settings.SubtitlePosition;
        subtitles["style"] = settings.SubtitleStyle;
        appearance["theme"] = settings.Theme;
        appearance["accent_source"] = settings.AccentSource;
        updates["auto_check"] = settings.AutoCheckUpdates;
        updates["channel"] = ReadString(updates, "channel") == "candidate" ? "candidate" : "public";
        SetOptional(
            advanced,
            "keybindings",
            string.IsNullOrWhiteSpace(settings.Keybindings) ? null : settings.Keybindings.Trim());
        privacy["history_retention_days"] = settings.HistoryRetentionDays;
        settings.SchemaVersion = SchemaVersion;
    }

    private static void SetVideoAdjustment(JsonObject video, string name, double raw)
    {
        double value = VideoAdjustments.Normalize(raw);
        if (Math.Abs(value - VideoAdjustments.Neutral) < 0.005)
            video.Remove(name);
        else
            video[name] = value;
    }

    private static void SetOptional(JsonObject section, string name, string? value)
    {
        if (value is null)
            section.Remove(name);
        else
            section[name] = value;
    }

    private static JsonObject Section(JsonObject root, string name)
        => root[name] as JsonObject ?? new JsonObject();

    private static JsonObject EnsureSection(JsonObject root, string name)
    {
        if (root[name] is JsonObject section)
            return section;
        section = new JsonObject();
        root[name] = section;
        return section;
    }

    private static string? ReadString(JsonObject section, string name)
        => section[name] is JsonValue value && value.TryGetValue<string>(out string result) ? result : null;

    private static bool? ReadBool(JsonObject section, string name)
        => section[name] is JsonValue value && value.TryGetValue<bool>(out bool result) ? result : null;

    private static int? ReadInt(JsonObject section, string name)
    {
        if (section[name] is not JsonValue value)
            return null;
        if (value.TryGetValue<int>(out int integer))
            return integer;
        if (value.TryGetValue<long>(out long wide) && wide is >= int.MinValue and <= int.MaxValue)
            return (int)wide;
        return null;
    }

    private static double? ReadDouble(JsonObject section, string name)
    {
        if (section[name] is not JsonValue value)
            return null;
        if (value.TryGetValue<double>(out double number) && double.IsFinite(number))
            return number;
        if (value.TryGetValue<long>(out long integer))
            return integer;
        return null;
    }
}
