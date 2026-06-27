using OkPlayer.Core;
using Xunit;

namespace OkPlayer.Tests;

/// <summary>The welcome shelf shows as many continue-watching cards as fit its width and never scrolls
/// horizontally; these pin that fit rule for the shipped card geometry (194px card, 14px gap).</summary>
public class RecentsShelfTests
{
    const double Card = 194;
    const double Gap = 14;

    [Theory]
    // n cards need n*194 + (n-1)*14 px. 772 is the welcome column's content width (860 MaxWidth - 88 padding):
    // 3 cards = 610, 4 cards = 818 > 772, so exactly 3 fit.
    [InlineData(772, 3)]
    [InlineData(610, 3)]  // exactly three cards wide -> three
    [InlineData(609, 2)]  // one px short of three -> two
    [InlineData(832, 4)]  // a wider column fits four
    [InlineData(208, 1)]  // one card + one gap
    [InlineData(120, 1)]  // narrower than a card still shows one (better than an empty shelf)
    public void VisibleCount_FitsAsManyWholeCardsAsThereIsRoomFor(double width, int expected)
        => Assert.Equal(expected, RecentsShelf.VisibleCount(width, available: 20, Card, Gap));

    [Fact]
    public void VisibleCount_IsCappedByWhatIsAvailable()
        => Assert.Equal(2, RecentsShelf.VisibleCount(rowWidth: 2000, available: 2, Card, Gap));

    [Fact]
    public void VisibleCount_IsZero_WhenNothingIsAvailable()
        => Assert.Equal(0, RecentsShelf.VisibleCount(rowWidth: 772, available: 0, Card, Gap));

    [Theory]
    [InlineData(10, 3)] // unmeasured -> the default, capped by availability
    [InlineData(2, 2)]
    public void VisibleCount_FallsBackToTheDefault_BeforeTheRowIsMeasured(int available, int expected)
        => Assert.Equal(expected, RecentsShelf.VisibleCount(rowWidth: 0, available, Card, Gap, unmeasuredDefault: 3));
}
