using System;
using System.IO;
using System.Text.Json;
using System.Text.Json.Serialization;

namespace OkPlayer.App.Services;

/// <summary>User preferences. Only the keys whose controls are rendered in the design ship now;
/// later panels add their keys as they land. Defaults are the design's "smart defaults".</summary>
public sealed class AppSettings
{
    // Appearance (design band 9 — fully rendered)
    public string Theme { get; set; } = "Auto";        // "Light" | "Auto"
    public string AccentSource { get; set; } = "OkTeal"; // "System" | "OkTeal"
    public int MicaTitlebar { get; set; } = 55;          // 0..100
    public int MicaPanels { get; set; } = 70;            // 0..100
    public int MicaOverlays { get; set; } = 40;          // 0..100

    public int SchemaVersion { get; set; } = 1;          // forward-compat migration hook
}

/// <summary>
/// App settings persisted as human-readable JSON at %APPDATA%/OkPlayer/settings.json (no database),
/// mirroring <see cref="HistoryService"/>'s resilient pattern. A single shared instance is the one
/// source of truth; <see cref="Save"/> raises <see cref="Changed"/> so the player re-applies live.
/// </summary>
public sealed class SettingsService
{
    private static readonly JsonSerializerOptions JsonOpts = new()
    {
        WriteIndented = true,
        DefaultIgnoreCondition = JsonIgnoreCondition.WhenWritingNull,
    };

    private readonly string? _path; // null when AppData is unreachable — runs without persistence
    private readonly object _lock = new();

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

    private void Load()
    {
        try
        {
            if (_path is null || !File.Exists(_path))
                return;
            var data = JsonSerializer.Deserialize<AppSettings>(File.ReadAllText(_path), JsonOpts);
            if (data is not null)
                Current = data;
        }
        catch { /* corrupt/unreadable settings are non-fatal — keep defaults */ }
    }

    /// <summary>Persist the current settings and notify listeners to re-apply.</summary>
    public void Save()
    {
        if (_path is not null)
        {
            try
            {
                string json;
                lock (_lock)
                    json = JsonSerializer.Serialize(Current, JsonOpts);
                string tmp = _path + ".tmp";
                File.WriteAllText(tmp, json);
                File.Move(tmp, _path, overwrite: true); // replace in one step so a crash can't truncate
            }
            catch { /* best effort */ }
        }
        Changed?.Invoke();
    }
}
