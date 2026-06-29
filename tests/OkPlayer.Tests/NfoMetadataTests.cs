using OkPlayer.Core;
using Xunit;

namespace OkPlayer.Tests;

public class NfoMetadataTests
{
    [Fact]
    public void Kodi_Movie_ReadsTitleYearPlot()
    {
        const string xml = """
            <movie>
              <title>Blade Runner 2049</title>
              <year>2017</year>
              <plot>A young blade runner uncovers a long-buried secret.</plot>
            </movie>
            """;
        var nfo = NfoMetadata.Parse(xml);
        Assert.NotNull(nfo);
        Assert.Equal("Blade Runner 2049", nfo!.Title);
        Assert.Equal(2017, nfo.Year);
        Assert.Equal("A young blade runner uncovers a long-buried secret.", nfo.Plot);
    }

    [Fact]
    public void Episode_ReadsAiredYear_WhenNoYearElement()
    {
        const string xml = """
            <episodedetails>
              <title>The Constant</title>
              <aired>2008-02-28</aired>
              <outline>Desmond experiences unusual side effects.</outline>
            </episodedetails>
            """;
        var nfo = NfoMetadata.Parse(xml);
        Assert.NotNull(nfo);
        Assert.Equal("The Constant", nfo!.Title);
        Assert.Equal(2008, nfo.Year);                          // from <aired>
        Assert.Equal("Desmond experiences unusual side effects.", nfo.Plot); // <outline> fallback
    }

    [Fact]
    public void Premiered_SuppliesYear_OverMissingYear()
    {
        var nfo = NfoMetadata.Parse("<movie><title>Dune</title><premiered>2021-10-22</premiered></movie>");
        Assert.Equal(2021, nfo!.Year);
    }

    [Fact]
    public void OriginalTitle_FallsBack_WhenNoTitle()
    {
        var nfo = NfoMetadata.Parse("<movie><originaltitle>Spirited Away</originaltitle></movie>");
        Assert.NotNull(nfo);
        Assert.Equal("Spirited Away", nfo!.Title);
    }

    [Fact]
    public void NestedTitle_DoesNotMasqueradeAsItemTitle()
    {
        // A <title> nested inside <set> must not be picked as the movie title — only direct children count.
        const string xml = """
            <movie>
              <set><name>Trilogy</name><title>Set Title</title></set>
              <title>The Real Movie</title>
            </movie>
            """;
        Assert.Equal("The Real Movie", NfoMetadata.Parse(xml)!.Title);
    }

    [Fact]
    public void TitleTrimmed_AndNamespaceAgnostic()
    {
        var nfo = NfoMetadata.Parse("<movie><title>  Arrival  </title></movie>");
        Assert.Equal("Arrival", nfo!.Title);
        Assert.Null(nfo.Year);
        Assert.Null(nfo.Plot);
    }

    [Theory]
    [InlineData("")]
    [InlineData("   ")]
    [InlineData("https://www.imdb.com/title/tt1856101/")]    // legacy URL-only .nfo — not XML
    [InlineData("<movie><year>2020</year></movie>")]         // no title -> not useful
    [InlineData("<movie></movie>")]
    [InlineData("not xml at all <<<")]
    public void Unusable_ReturnsNull(string input)
    {
        Assert.Null(NfoMetadata.Parse(input));
    }

    [Fact]
    public void GarbageYear_Ignored()
    {
        var nfo = NfoMetadata.Parse("<movie><title>X</title><year>n/a</year></movie>");
        Assert.NotNull(nfo);
        Assert.Null(nfo!.Year);
    }
}
