using OkPlayer.Core;

namespace OkPlayer.Tests;

/// <summary>Unit tests for resolving (artist, track) for a lyrics lookup. Real tags must win; a missing artist
/// tag should be mined from a "Artist - Title" display/filename, but only to fill the gap — never to override
/// a real tag.</summary>
public class TrackTagsTests
{
    [Fact]
    public void RealTags_AreUsedVerbatim_AndDisplayIsNotMined()
    {
        var (artist, track) = TrackTags.Resolve("Daft Punk", "Aerodynamic", "Something - Else", "99 - whatever");
        Assert.Equal("Daft Punk", artist);
        Assert.Equal("Aerodynamic", track);
    }

    [Fact]
    public void NoTags_MinesArtistAndTitleFromDisplay()
    {
        var (artist, track) = TrackTags.Resolve(null, null, "Daft Punk - Aerodynamic", "01 - track");
        Assert.Equal("Daft Punk", artist);
        Assert.Equal("Aerodynamic", track);
    }

    [Fact]
    public void MissingArtistTag_IsFilledFromDisplay_TitleTagKept()
    {
        var (artist, track) = TrackTags.Resolve(null, "Aerodynamic", "Daft Punk - Aerodynamic", null);
        Assert.Equal("Daft Punk", artist);
        Assert.Equal("Aerodynamic", track); // the real title tag is kept, not the split half
    }

    [Fact]
    public void NoDash_WholeStringBecomesTrack_ArtistNull()
    {
        var (artist, track) = TrackTags.Resolve(null, null, "Some Untitled Jam", null);
        Assert.Null(artist);
        Assert.Equal("Some Untitled Jam", track);
    }

    [Fact]
    public void FallsBackToFileStem_WhenNoDisplay()
    {
        var (artist, track) = TrackTags.Resolve(null, null, null, "Radiohead - Idioteque");
        Assert.Equal("Radiohead", artist);
        Assert.Equal("Idioteque", track);
    }

    [Fact]
    public void Whitespace_IsTreatedAsAbsent_AndTrimmed()
    {
        var (artist, track) = TrackTags.Resolve("   ", "  Hey  ", "  ", null);
        Assert.Null(artist);
        Assert.Equal("Hey", track);
    }

    [Fact]
    public void EmptyEverything_IsAllNull()
    {
        var (artist, track) = TrackTags.Resolve(null, null, null, null);
        Assert.Null(artist);
        Assert.Null(track);
    }
}
