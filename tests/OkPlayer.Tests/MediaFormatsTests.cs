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

    [Theory]
    [InlineData(@"C:\music\song.flac")]
    [InlineData(@"C:\music\track.MP3")]  // case-insensitive
    [InlineData("podcast.opus")]
    [InlineData(@"C:\music\album.mka")]
    public void IsAudio_TrueForAudioOnlyContainers(string path) => Assert.True(MediaFormats.IsAudio(path));

    [Theory]
    [InlineData(@"C:\v\movie.mkv")]
    [InlineData(@"C:\v\clip.mp4")]
    [InlineData(@"C:\v\subs.srt")]
    [InlineData(@"C:\v\notes.txt")]
    public void IsAudio_FalseForVideoSubtitleAndOther(string path) => Assert.False(MediaFormats.IsAudio(path));

    [Fact]
    public void AudioExtensions_AreAllRecognizedMedia() // the audio subset must never drift out of Extensions
    {
        foreach (string ext in MediaFormats.AudioExtensions)
            Assert.True(MediaFormats.IsMedia("x" + ext));
    }

    [Theory]
    [InlineData("https://example.com/video.mkv")]
    [InlineData("http://host:8080/stream")]
    [InlineData("smb://nas/share/movie.mkv")]
    [InlineData("rtsp://host/live")]
    [InlineData("  https://example.com/v.mp4  ")] // surrounding whitespace is trimmed
    public void IsPlayableUrl_TrueForAbsoluteStreamUrls(string text) => Assert.True(MediaFormats.IsPlayableUrl(text));

    [Theory]
    [InlineData("check out https://example.com/v.mkv it's great")] // a paragraph that merely contains a URL
    [InlineData("file:///C:/media/movie.mkv")]                     // an explicit file: URI is a local file, not a stream
    [InlineData("movie.mkv")]                                       // relative, not absolute
    [InlineData("not a url at all")]
    [InlineData("")]
    [InlineData("   ")]
    [InlineData(null)]
    public void IsPlayableUrl_FalseForPathsParagraphsAndJunk(string? text) => Assert.False(MediaFormats.IsPlayableUrl(text));
}
