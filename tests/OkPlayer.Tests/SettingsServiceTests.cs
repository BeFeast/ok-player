using OkPlayer.App.Services;

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
        Assert.False(s.AudioNormalization);   // off by default
        Assert.Equal("", s.AudioDevice);      // empty = mpv's default output
        Assert.Equal(0, s.HistoryRetentionDays); // keep forever by default
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
        a.Save();

        var reloaded = New().Current; // a fresh service reads the same file
        Assert.Equal("Light", reloaded.Theme);
        Assert.Equal(75, reloaded.DefaultVolume);
        Assert.Equal(1.4, reloaded.SubtitleScale);
        Assert.True(reloaded.AudioNormalization);
        Assert.Equal("wasapi/{headphones}", reloaded.AudioDevice);
        Assert.Equal(30, reloaded.HistoryRetentionDays);
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
}
