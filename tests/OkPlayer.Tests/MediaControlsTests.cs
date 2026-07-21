using OkPlayer.Core;

namespace OkPlayer.Tests;

public class MediaControlsTests
{
    [Theory]
    [InlineData("Play", MediaControlCommand.Play)]
    [InlineData("Pause", MediaControlCommand.Pause)]
    [InlineData("Stop", MediaControlCommand.Stop)]
    [InlineData("Next", MediaControlCommand.Next)]
    [InlineData("Previous", MediaControlCommand.Previous)]
    public void CommandFromButtonName_MapsSupportedTransportButtons(string name, MediaControlCommand expected)
        => Assert.Equal(expected, MediaControls.CommandFromButtonName(name));

    [Theory]
    [InlineData("FastForward")]
    [InlineData("Rewind")]
    [InlineData("")]
    [InlineData(null)]
    public void CommandFromButtonName_RejectsUnsupportedButtons(string? name)
        => Assert.Null(MediaControls.CommandFromButtonName(name));

    [Fact]
    public void Project_NoMedia_ClosesAndDisablesTheSession()
    {
        var projected = MediaControls.Project(new MediaControlSnapshot(
            HasMedia: false, IsPaused: false, PositionSeconds: 15, DurationSeconds: 100,
            PlaybackRate: 2, CanGoNext: true, CanGoPrevious: true));

        Assert.Equal(MediaControlPlaybackStatus.Closed, projected.PlaybackStatus);
        Assert.Equal(TimeSpan.Zero, projected.Position);
        Assert.Equal(TimeSpan.Zero, projected.Duration);
        Assert.Equal(1, projected.PlaybackRate);
        Assert.False(projected.CanPlay);
        Assert.False(projected.CanPause);
        Assert.False(projected.CanStop);
        Assert.False(projected.CanGoNext);
        Assert.False(projected.CanGoPrevious);
    }

    [Fact]
    public void Project_PlayingMedia_ClampsTimelineAndRateAndKeepsCapabilities()
    {
        var projected = MediaControls.Project(new MediaControlSnapshot(
            HasMedia: true, IsPaused: false, PositionSeconds: 150, DurationSeconds: 120,
            PlaybackRate: 10, CanGoNext: true, CanGoPrevious: false));

        Assert.Equal(MediaControlPlaybackStatus.Playing, projected.PlaybackStatus);
        Assert.Equal(TimeSpan.FromSeconds(120), projected.Position);
        Assert.Equal(TimeSpan.FromSeconds(120), projected.Duration);
        Assert.Equal(MediaControls.MaximumPlaybackRate, projected.PlaybackRate);
        Assert.True(projected.CanPlay);
        Assert.True(projected.CanPause);
        Assert.True(projected.CanStop);
        Assert.True(projected.CanGoNext);
        Assert.False(projected.CanGoPrevious);
    }

    [Fact]
    public void Project_PausedUnknownDuration_PublishesAValidZeroedTimeline()
    {
        var projected = MediaControls.Project(new MediaControlSnapshot(
            HasMedia: true, IsPaused: true, PositionSeconds: 42, DurationSeconds: double.NaN,
            PlaybackRate: double.NaN, CanGoNext: false, CanGoPrevious: false));

        Assert.Equal(MediaControlPlaybackStatus.Paused, projected.PlaybackStatus);
        Assert.Equal(TimeSpan.Zero, projected.Position);
        Assert.Equal(TimeSpan.Zero, projected.Duration);
        Assert.Equal(1, projected.PlaybackRate);
    }

    [Theory]
    [InlineData(-10, 100, 0)]
    [InlineData(50, 100, 50)]
    [InlineData(120, 100, 100)]
    [InlineData(double.NaN, 100, 0)]
    public void NormalizePosition_ClampsToKnownBounds(double requested, double duration, double expected)
        => Assert.Equal(expected, MediaControls.NormalizePosition(requested, duration));

    [Theory]
    [InlineData(0.1, 0.25)]
    [InlineData(1.5, 1.5)]
    [InlineData(8, 4)]
    public void NormalizePlaybackRate_UsesMprisParityBounds(double requested, double expected)
        => Assert.Equal(expected, MediaControls.NormalizePlaybackRate(requested));

    [Fact]
    public void NormalizePlaybackRate_RejectsNonFiniteRequests()
        => Assert.Null(MediaControls.NormalizePlaybackRate(double.PositiveInfinity));

    [Fact]
    public void ResolveMetadata_DisplayTitleWinsAndTagsSupplyArtistAndAlbum()
    {
        var metadata = MediaControls.ResolveMetadata(
            "Curated NFO title", "Artist", "Embedded title", "Album", "File name");

        Assert.Equal("Curated NFO title", metadata.Title);
        Assert.Equal("Artist", metadata.Artist);
        Assert.Equal("Album", metadata.Album);
        Assert.Equal("Artist · Album", metadata.SecondaryText);
    }

    [Fact]
    public void ResolveMetadata_FallsBackToFileNameAndMinesArtist()
    {
        var metadata = MediaControls.ResolveMetadata(null, null, null, null, "Boards of Canada - Roygbiv");

        Assert.Equal("Boards of Canada - Roygbiv", metadata.Title);
        Assert.Equal("Boards of Canada", metadata.Artist);
        Assert.Null(metadata.Album);
    }

    [Theory]
    [InlineData(0, 0)]
    [InlineData(50, 5)]
    [InlineData(600, 30)]
    [InlineData(double.NaN, 0)]
    public void ArtworkPosition_UsesAnEarlyBoundedFrame(double duration, double expected)
        => Assert.Equal(expected, MediaControls.ArtworkPosition(duration));
}
