using System;
using System.IO;
using System.Linq;
using OkPlayer.Core;
using OkPlayer.Mpv;
using Xunit.Abstractions;

namespace OkPlayer.IntegrationTests;

/// <summary>Pixel-level regression guard for the subtitle STYLE presets (Settings → Subtitles). Each preset
/// is a fixed set of mpv subtitle-style options (<see cref="SubtitleStyle"/> in Core); this renders the
/// plain-text fixture through the real libmpv image VO with a preset applied and measures the burned-in
/// caption's colour.
///
/// Why colour and not merely "did it render": the subtitle work's recurring failure mode is an option that
/// is ACCEPTED but silently no-ops (sub-margin-y on ASS — see SubtitleOscClearanceTests). Applying each
/// preset through <see cref="MpvContext.SetOption"/> already fails this test loudly if any option NAME is
/// wrong (mpv_set_option_string errors on an unknown option), and asserting the rendered fill colour proves
/// <c>sub-color</c> actually TOOK EFFECT — so a future option rename or a wrong colour format can't pass
/// unnoticed. The text/SRT fixture is used by design: presets style mpv's own text renderer, while ASS subs
/// keep their embedded styling (<c>sub-ass-override=scale</c>) and are intentionally left unrepainted.</summary>
[Trait("Category", "Integration")]
public class SubtitleStyleTests
{
    // Separates the subtitle fill from the dark video background, matching SubtitleOscClearanceTests.
    const byte BrightLuma = 170;

    readonly ITestOutputHelper _out;
    public SubtitleStyleTests(ITestOutputHelper output) => _out = output;

    static int _seq; // unique render-output directory suffix per call

    [Fact]
    public void EveryPreset_RendersAVisibleCaption_WithValidMpvOptionNames()
    {
        // Render() applies each option via SetOption, which throws on an unknown option name, so this also
        // asserts every preset's option names are valid for the bundled libmpv — the cheap guard against an
        // option being renamed out from under us (the exact class of bug behind the sub-margin-y regressions).
        foreach (var style in SubtitleStyle.All)
        {
            var frame = Render(style);
            int bright = CountBright(frame);
            _out.WriteLine($"{style.Key}: {bright} bright px");
            Assert.True(bright > 50, $"{style.Key}: expected a visible styled caption, found {bright} bright px");
        }
    }

    [Fact]
    public void ClassicPreset_RendersYellow_DefaultRendersWhite()
    {
        var def = MeanBrightColor(Render(SubtitleStyle.Default));
        var cls = MeanBrightColor(Render(SubtitleStyle.Classic));
        _out.WriteLine($"Default fill ~ R{def.R} G{def.G} B{def.B}; Classic fill ~ R{cls.R} G{cls.G} B{cls.B}");

        // Default: white -> all channels high (in particular blue is high).
        Assert.True(def.B > 150, $"Default caption should render white (high blue), got B={def.B}");

        // Classic: yellow -> red & green high, blue low. This is the assertion that proves sub-color applied:
        // had it silently no-opped, Classic would render white like Default and its blue would stay high.
        Assert.True(cls.R > 150 && cls.G > 150,
            $"Classic caption should render yellow (high red+green), got R={cls.R} G={cls.G}");
        Assert.True(cls.B < 110, $"Classic caption should render yellow (low blue), got B={cls.B}");
        Assert.True(def.B - cls.B > 60,
            $"Classic should be markedly less blue than Default (proves sub-color took effect): " +
            $"def.B={def.B} cls.B={cls.B}");
    }

    /// <summary>Renders one frame of the text fixture with <paramref name="style"/>'s options burned in via
    /// libmpv's image VO, and returns its decoded pixels. Mirrors the synchronisation discipline of
    /// SubtitleOscClearanceTests: wait only until the VO has produced the file, then let the using-block's
    /// Dispose (mpv_terminate_destroy) flush and close the PNG before we decode it.</summary>
    static PngLuma Render(SubtitleStyle style)
    {
        string outdir = Path.Combine(Path.GetTempPath(), "okp-substyle",
            $"{style.Key}-{System.Threading.Interlocked.Increment(ref _seq)}");
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
            ctx.SetOption("frames", "1");
            foreach (var (name, value) in style.Options)
                ctx.SetOption(name, value);       // throws on an unknown/renamed option name -> loud failure
            ctx.Initialize();
            ctx.Command("loadfile", Fix("subtest_text.mkv"), "replace");

            for (int i = 0; i < 200 && !Directory.EnumerateFiles(outdir, "*.png").Any(); i++)
                System.Threading.Thread.Sleep(50);
        } // <- Dispose() => mpv_terminate_destroy: flushes & closes the PNG before returning

        string? frame = Directory.EnumerateFiles(outdir, "*.png").OrderBy(f => f).LastOrDefault();
        Assert.True(frame is not null, "libmpv image VO produced no PNG; " +
                                       "is libmpv-2.dll present next to the test assembly?");
        return PngLuma.Load(frame!);
    }

    static int CountBright(PngLuma img)
    {
        int n = 0;
        for (int y = 0; y < img.Height; y++)
            for (int x = 0; x < img.Width; x++)
                if (img.At(x, y) >= BrightLuma) n++;
        return n;
    }

    /// <summary>Average RGB over the bright (fill) pixels — the subtitle's solid interior, away from the
    /// anti-aliased edge into the black outline — so the result is the caption's actual text colour.</summary>
    static (int R, int G, int B) MeanBrightColor(PngLuma img)
    {
        long r = 0, g = 0, b = 0, n = 0;
        for (int y = 0; y < img.Height; y++)
            for (int x = 0; x < img.Width; x++)
                if (img.At(x, y) >= BrightLuma)
                {
                    var (pr, pg, pb) = img.ColorAt(x, y);
                    r += pr; g += pg; b += pb; n++;
                }
        return n == 0 ? (0, 0, 0) : ((int)(r / n), (int)(g / n), (int)(b / n));
    }

    static string Fix(string n) => Path.Combine(AppContext.BaseDirectory, "fixtures", n);
}
