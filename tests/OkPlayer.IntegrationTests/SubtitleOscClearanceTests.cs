using System;
using System.IO;
using System.Linq;
using OkPlayer.Mpv;
using Xunit.Abstractions;

namespace OkPlayer.IntegrationTests;

/// <summary>Pixel-level regression guard for PRD P1-D9: while the OSC chrome is up, subtitles must be lifted
/// clear of it for EVERY subtitle kind. This renders the fixtures through the real libmpv image VO at the
/// exact sub-pos the app applies and measures where the burned-in caption actually lands on the frame.
///
/// It is the test that would have caught the two "OSC over captions" regressions: an earlier fix drove the
/// lift through sub-margin-y / sub-ass-force-margins, which libass silently ignores for ASS subtitles, so the
/// captions never moved while text subs did. sub-pos lifts ALL kinds — these assertions prove it on rendered
/// pixels, not on property names. The companion <see cref="DefaultPosition_PutsSubtitleInTheOscZone"/> case is
/// the sanity anchor: if the fixture or threshold drifts so a subtitle never lands in the zone, the clearance
/// assertions would pass vacuously.</summary>
[Trait("Category", "Integration")]
public class SubtitleOscClearanceTests
{
    // Mirrors PlayerViewModel.OscSubtitleLift (16) applied to the default SubtitlePosition (100): the app
    // sets sub-pos to (100 - 16) = 84 while the chrome is visible. Keep in sync if the lift constant changes.
    const double BasePos = 100;
    const double OscLift = 16;
    const double LiftedPos = BasePos - OscLift; // 84

    // The OSC chrome (seek bar + time row) occupies roughly the bottom 13% of the frame. A subtitle whose
    // lowest bright pixel sits at or below this line overlaps the controls; above it, it clears them.
    const double OscTopFraction = 0.87;

    // Burned-in white caption on the dark fixture video; a generous luma cut cleanly separates text from
    // background without depending on anti-alias edges.
    const byte BrightLuma = 170;

    readonly ITestOutputHelper _out;
    public SubtitleOscClearanceTests(ITestOutputHelper output) => _out = output;

    static int _seq; // unique render-output directory suffix per call

    static string Inv(double v) => v.ToString(System.Globalization.CultureInfo.InvariantCulture);

    public static TheoryData<string, string> Fixtures => new()
    {
        { "ASS",  "subtest.mkv" },      // ASS/libass — the kind sub-margin-y ignored
        { "TEXT", "subtest_text.mkv" }, // plain text/SRT
    };

    [Theory]
    [MemberData(nameof(Fixtures))]
    public void LiftedPosition_ClearsTheOscZone(string kind, string fixture)
    {
        var frame = Render(fixture, c => c.SetOption("sub-pos", Inv(LiftedPos)));
        var (top, bottom, count) = MeasureBrightBand(frame);
        int oscTop = (int)(frame.Height * OscTopFraction);
        _out.WriteLine($"{kind} lifted(sub-pos={LiftedPos}): bright rows {top}..{bottom} of {frame.Height} " +
                       $"(px={count}); OSC zone starts at y={oscTop}");

        Assert.True(count > 50, $"{kind}: expected the lifted subtitle to still be visible, found {count} bright px");
        Assert.True(bottom < oscTop,
            $"{kind}: lifted subtitle's lowest pixel y={bottom} must clear the OSC zone (y>={oscTop}). " +
            "The lift no-ops on this subtitle kind — exactly the OSC-over-captions regression.");
    }

    [Theory]
    [MemberData(nameof(Fixtures))]
    public void DefaultPosition_PutsSubtitleInTheOscZone(string kind, string fixture)
    {
        // Sanity anchor: at the unlifted default the subtitle DOES sit in the OSC zone, so the clearance test
        // above is measuring a real movement rather than passing because nothing renders there.
        var frame = Render(fixture, c => c.SetOption("sub-pos", Inv(BasePos)));
        var (top, bottom, count) = MeasureBrightBand(frame);
        int oscTop = (int)(frame.Height * OscTopFraction);
        _out.WriteLine($"{kind} default(sub-pos={BasePos}): bright rows {top}..{bottom} of {frame.Height} " +
                       $"(px={count}); OSC zone starts at y={oscTop}");

        Assert.True(count > 50, $"{kind}: expected a visible subtitle at default position, found {count} bright px");
        Assert.True(bottom >= oscTop,
            $"{kind}: default subtitle's lowest pixel y={bottom} should fall in the OSC zone (y>={oscTop}); " +
            "if it doesn't, the fixture or OscTopFraction drifted and the clearance test is vacuous.");
    }

    [Theory]
    [InlineData("ASS",  "subtest.mkv",      false)] // libass ignores sub-margin-y for ASS — stays in the zone
    [InlineData("TEXT", "subtest_text.mkv", true)]  // text/bitmap subs DO honor sub-margin-y — they lift
    public void SubMarginY_LiftsTextButNotAss(string kind, string fixture, bool expectClears)
    {
        // Root-cause canary. The two OSC-over-captions regressions came from driving the lift through
        // sub-margin-y / sub-ass-force-margins: libass honors the margin for text subs but ignores it for ASS
        // (sub-ass-override defaults to "scale"), so ASS captions never moved. This pins that exact asymmetry
        // on rendered pixels — it is WHY the app uses sub-pos. If libass ever starts honoring margins for ASS,
        // the ASS case here flips and we can revisit; until then it guards against anyone reintroducing the
        // margin-based lift and assuming it covers every subtitle kind.
        var frame = Render(fixture, c => c.SetOption("sub-margin-y", "128")); // sub-pos stays at the 100 default
        var (_, bottom, count) = MeasureBrightBand(frame);
        int oscTop = (int)(frame.Height * OscTopFraction);
        _out.WriteLine($"{kind} sub-margin-y=128: lowest bright row {bottom} of {frame.Height} " +
                       $"(px={count}); OSC zone starts at y={oscTop}; expectClears={expectClears}");

        Assert.True(count > 50, $"{kind}: expected the subtitle to render under sub-margin-y, found {count} bright px");
        if (expectClears)
            Assert.True(bottom < oscTop,
                $"{kind}: sub-margin-y should lift text subs clear of the OSC zone, but lowest pixel y={bottom} >= {oscTop}.");
        else
            Assert.True(bottom >= oscTop,
                $"{kind}: sub-margin-y unexpectedly lifted the ASS subtitle (lowest pixel y={bottom} < {oscTop}). " +
                "libass may now honor margins for ASS — revisit whether the sub-pos lift is still required.");
    }

    /// <summary>Renders one frame of <paramref name="fixture"/> with the active subtitle burned in via libmpv's
    /// image VO, after applying <paramref name="configureSub"/> (the subtitle-position option under test), and
    /// returns its luma.</summary>
    static PngLuma Render(string fixture, Action<MpvContext> configureSub)
    {
        string outdir = Path.Combine(Path.GetTempPath(), "okp-subclear",
            $"{Path.GetFileNameWithoutExtension(fixture)}-{System.Threading.Interlocked.Increment(ref _seq)}");
        if (Directory.Exists(outdir)) Directory.Delete(outdir, recursive: true);
        Directory.CreateDirectory(outdir);

        using (var ctx = new MpvContext())
        {
            ctx.SetOption("vo", "image");
            ctx.SetOption("vo-image-outdir", outdir);
            ctx.SetOption("vo-image-format", "png");
            ctx.SetOption("vf", "sub");          // burn the active subtitle into the rendered frame
            ctx.SetOption("sid", "1");
            ctx.SetOption("keep-open", "no");
            ctx.SetOption("start", "1");          // sample at t=1s, inside the fixture's 0..30s subtitle
            ctx.SetOption("frames", "3");
            configureSub(ctx);
            ctx.Initialize();
            ctx.Command("loadfile", Fix(fixture), "replace");

            // Wait for the image VO to flush frames (it writes 00000001.png ...). Poll rather than sleep a
            // fixed slug so the test is as quick as the machine allows but tolerant on a busy CI box.
            string? png = null;
            for (int i = 0; i < 100 && png is null; i++)
            {
                System.Threading.Thread.Sleep(50);
                png = Directory.EnumerateFiles(outdir, "*.png").OrderBy(f => f).LastOrDefault();
            }
            System.Threading.Thread.Sleep(150); // let the last file finish writing
        }

        string? frame = Directory.EnumerateFiles(outdir, "*.png").OrderBy(f => f).LastOrDefault();
        Assert.True(frame is not null, $"libmpv image VO produced no PNG for {fixture}; " +
                                       "is libmpv-2.dll present next to the test assembly?");
        return PngLuma.Load(frame!);
    }

    /// <summary>Finds the vertical band of bright (subtitle) pixels: the first and last rows that contain a
    /// meaningful run of bright pixels, plus the total bright-pixel count. A per-row threshold rejects stray
    /// bright specks so a single hot pixel can't be mistaken for a caption row.</summary>
    static (int top, int bottom, int count) MeasureBrightBand(PngLuma img)
    {
        int top = -1, bottom = -1, total = 0;
        const int rowMin = 8; // a real text row lights up far more than this many pixels
        for (int y = 0; y < img.Height; y++)
        {
            int rowCount = 0;
            for (int x = 0; x < img.Width; x++)
                if (img.At(x, y) >= BrightLuma) rowCount++;
            if (rowCount >= rowMin)
            {
                if (top < 0) top = y;
                bottom = y;
                total += rowCount;
            }
        }
        return (top, bottom, total);
    }

    static string Fix(string n) => Path.Combine(AppContext.BaseDirectory, "fixtures", n);
}
