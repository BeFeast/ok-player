using OkPlayer.Core;

namespace OkPlayer.Tests;

public class MediaFormatsTests
{
    [Theory]
    [InlineData(@"C:\v\movie.mkv")]
    [InlineData(@"C:\v\clip.MP4")]   // case-insensitive
    [InlineData(@"song.flac")]
    public void IsMedia_TrueForKnownMedia(string path) => Assert.True(MediaFormats.IsMedia(path));

    [Theory]
    [InlineData(@"C:\v\subs.srt")]
    [InlineData(@"C:\v\track.ass")]
    [InlineData(@"C:\v\track.VTT")]  // case-insensitive
    public void IsMedia_FalseForSubtitles(string path) => Assert.False(MediaFormats.IsMedia(path));

    [Theory]
    [InlineData(@"C:\v\subs.srt")]
    [InlineData(@"C:\v\track.ass")]
    [InlineData(@"C:\v\track.ssa")]
    [InlineData(@"C:\v\track.SUB")]  // case-insensitive
    [InlineData(@"C:\v\track.vtt")]
    public void IsSubtitle_TrueForSubtitleFiles(string path) => Assert.True(MediaFormats.IsSubtitle(path));

    [Theory]
    [InlineData(@"C:\v\movie.mkv")]
    [InlineData(@"C:\v\song.flac")]
    [InlineData(@"C:\v\notes.txt")]
    public void IsSubtitle_FalseForNonSubtitles(string path) => Assert.False(MediaFormats.IsSubtitle(path));

    [Fact]
    public void MediaAndSubtitleSets_DoNotOverlap()
    {
        foreach (string ext in MediaFormats.SubtitleExtensions)
            Assert.False(MediaFormats.IsMedia("x" + ext)); // a subtitle is never treated as playable media
    }
}
