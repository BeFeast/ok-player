using System.Text.Json;
using System.Threading;
using OkPlayer.App.Services;
using OkPlayer.Core;

namespace OkPlayer.Tests;

public class SettingsServiceTests : IDisposable
{
    private readonly string _path = Path.Combine(Path.GetTempPath(), $"okplayer-settings-{Guid.NewGuid():N}.json");

    private SettingsService New() => new(_path); // internal test ctor

    public void Dispose()
    {
        try { File.Delete(_path); } catch { }
    }

    [Fact]
    public void FreshService_HasSmartDefaults()
    {
        var s = New().Current;
        Assert.Equal("Auto", s.Theme);
        Assert.Equal(100, s.DefaultVolume);
        Assert.True(s.ResumePlayback);
        Assert.Equal(1.0, s.DefaultSpeed);
        Assert.True(s.HardwareDecoding);
        Assert.Equal(0.0, s.Brightness);
        Assert.Equal(0.0, s.Contrast);
        Assert.Equal(0.0, s.Saturation);
        Assert.Equal(0.0, s.Gamma);
        Assert.False(s.AudioNormalization);   // off by default
        Assert.Equal("", s.AudioDevice);      // empty = mpv's default output
        Assert.Equal(0, s.HistoryRetentionDays); // keep forever by default
        Assert.Equal("", s.Keybindings);
    }

    [Fact]
    public void Save_ThenReload_PersistsChangedValues()
    {
        var a = New();
        a.Current.Theme = "Light";
        a.Current.DefaultVolume = 75;
        a.Current.SubtitleScale = 1.4;
        a.Current.AudioNormalization = true;
        a.Current.AudioDevice = "wasapi/{headphones}";
        a.Current.HistoryRetentionDays = 30;
        a.Current.Keybindings = "play-pause=P\nplay-pause=Space";
        a.Current.Brightness = 18;
        a.Current.Contrast = -12;
        a.Current.Saturation = 24;
        a.Current.Gamma = -6;
        a.Save();

        var reloaded = New().Current; // a fresh service reads the same file
        Assert.Equal("Light", reloaded.Theme);
        Assert.Equal(75, reloaded.DefaultVolume);
        Assert.Equal(1.4, reloaded.SubtitleScale);
        Assert.True(reloaded.AudioNormalization);
        Assert.Equal("wasapi/{headphones}", reloaded.AudioDevice);
        Assert.Equal(30, reloaded.HistoryRetentionDays);
        Assert.Equal("play-pause=P\nplay-pause=Space", reloaded.Keybindings);
        Assert.Equal(18.0, reloaded.Brightness);
        Assert.Equal(-12.0, reloaded.Contrast);
        Assert.Equal(24.0, reloaded.Saturation);
        Assert.Equal(-6.0, reloaded.Gamma);
    }

    [Fact]
    public void Save_ThenReload_PersistsAccentSource()
    {
        var a = New();
        a.Current.AccentSource = "System";
        a.Save();
        Assert.Equal("System", New().Current.AccentSource); // must survive a restart, not revert to teal
    }

    // Regression: %APPDATA% files take brief exclusive locks from Defender / the Search indexer. The save
    // used to swallow the resulting sharing violation and silently drop the write (the System accent
    // "reverting to teal after restart"). Save must retry across a transient lock and still land.
    [Fact]
    public void Save_RetriesAcrossATransientLock_AndStillPersists()
    {
        var a = New();
        a.Current.AccentSource = "System";
        a.Save();                              // create the file first so the next save is a replace
        Assert.True(File.Exists(_path));

        a.Current.DefaultVolume = 42;          // a change we expect to survive despite the lock
        var locker = new FileStream(_path, FileMode.Open, FileAccess.Read, FileShare.None);
        var release = Task.Run(() => { Thread.Sleep(60); locker.Dispose(); }); // free it within the retry budget
        a.Save();                              // retries until the lock clears instead of giving up
        release.Wait();

        var reloaded = New().Current;
        Assert.Equal(42, reloaded.DefaultVolume);
        Assert.Equal("System", reloaded.AccentSource);
    }

    [Fact]
    public void Save_RaisesChanged()
    {
        var s = New();
        int fired = 0;
        s.Changed += () => fired++;
        s.Save();
        Assert.Equal(1, fired);
    }

    [Fact]
    public void CorruptFile_FallsBackToDefaults()
    {
        File.WriteAllText(_path, "{ this is not valid json ");
        Assert.Equal("Auto", New().Current.Theme); // unreadable settings are non-fatal
    }

    [Fact]
    public void Save_WritesTheSharedCanonicalVideoSchema_AndOmitsNeutralValues()
    {
        var service = New();
        service.Current.SetVideoAdjustment(VideoAdjustmentKind.Brightness, 17);
        service.Current.SetVideoAdjustment(VideoAdjustmentKind.Contrast, 0);
        service.Current.SetVideoAdjustment(VideoAdjustmentKind.Saturation, 125);
        service.Current.SetVideoAdjustment(VideoAdjustmentKind.Gamma, double.NaN);
        service.Save();

        using JsonDocument json = JsonDocument.Parse(File.ReadAllText(_path));
        JsonElement root = json.RootElement;
        Assert.Equal(2, root.GetProperty("version").GetInt32());
        Assert.False(root.TryGetProperty("SchemaVersion", out _));
        Assert.Equal("public", root.GetProperty("updates").GetProperty("channel").GetString());
        JsonElement video = root.GetProperty("video");
        Assert.Equal("auto-safe", video.GetProperty("hwdec").GetString());
        Assert.Equal(17.0, video.GetProperty("brightness").GetDouble());
        Assert.Equal(100.0, video.GetProperty("saturation").GetDouble());
        Assert.False(video.TryGetProperty("contrast", out _));
        Assert.False(video.TryGetProperty("gamma", out _));
    }

    [Fact]
    public void CanonicalLoadAndSave_PreservesLinuxOnlyFields()
    {
        File.WriteAllText(_path, """
        {
          "version": 2,
          "playback": { "auto_advance": false, "repeat": "all", "shuffle": true },
          "audio": { "downmix_surround_to_stereo": true },
          "video": { "brightness": 12, "contrast": -8 },
          "updates": { "auto_check": true, "channel": "candidate" },
          "advanced": { "keybindings": "space cycle pause" },
          "future_section": { "kept": true }
        }
        """);

        var service = New();
        Assert.Equal(12.0, service.Current.Brightness);
        Assert.Equal(-8.0, service.Current.Contrast);
        Assert.Equal("space cycle pause", service.Current.Keybindings);
        service.Current.Gamma = 9;
        service.Save();

        using JsonDocument json = JsonDocument.Parse(File.ReadAllText(_path));
        JsonElement root = json.RootElement;
        Assert.False(root.GetProperty("playback").GetProperty("auto_advance").GetBoolean());
        Assert.Equal("all", root.GetProperty("playback").GetProperty("repeat").GetString());
        Assert.True(root.GetProperty("playback").GetProperty("shuffle").GetBoolean());
        Assert.True(root.GetProperty("audio").GetProperty("downmix_surround_to_stereo").GetBoolean());
        Assert.Equal("candidate", root.GetProperty("updates").GetProperty("channel").GetString());
        Assert.Equal("space cycle pause", root.GetProperty("advanced").GetProperty("keybindings").GetString());
        Assert.True(root.GetProperty("future_section").GetProperty("kept").GetBoolean());
        Assert.Equal(9.0, root.GetProperty("video").GetProperty("gamma").GetDouble());
    }

    [Fact]
    public void LegacyWindowsDocument_MigratesToCanonicalOnSave()
    {
        File.WriteAllText(_path, """
        {
          "Theme": "Dark",
          "DefaultVolume": 75,
          "HardwareDecoding": false,
          "SchemaVersion": 1
        }
        """);

        var service = New();
        Assert.Equal("Dark", service.Current.Theme);
        Assert.Equal(75, service.Current.DefaultVolume);
        Assert.False(service.Current.HardwareDecoding);
        service.Current.Keybindings = "play-pause=P";
        service.Current.Brightness = 14;
        service.Save();

        using JsonDocument json = JsonDocument.Parse(File.ReadAllText(_path));
        JsonElement root = json.RootElement;
        Assert.Equal(2, root.GetProperty("version").GetInt32());
        Assert.False(root.TryGetProperty("Theme", out _));
        Assert.Equal("Dark", root.GetProperty("appearance").GetProperty("theme").GetString());
        Assert.Equal(75, root.GetProperty("playback").GetProperty("volume").GetInt32());
        Assert.Equal("public", root.GetProperty("updates").GetProperty("channel").GetString());
        Assert.Equal("no", root.GetProperty("video").GetProperty("hwdec").GetString());
        Assert.Equal(14.0, root.GetProperty("video").GetProperty("brightness").GetDouble());
        Assert.Equal("play-pause=P", root.GetProperty("advanced").GetProperty("keybindings").GetString());
    }
}
