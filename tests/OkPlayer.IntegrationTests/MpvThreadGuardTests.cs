using OkPlayer.Mpv;

namespace OkPlayer.IntegrationTests;

/// <summary>Real-libmpv tests of the render-thread deadlock guard contract (the open-time freeze in #33).
/// They load the actual engine, so they need libmpv-2.dll at runtime and are tagged Category=Integration —
/// the fast headless CI excludes them; they run locally and in a Windows job that has fetched the natives.
/// The guard is DEBUG-only, so the assertions that depend on it are compiled only in Debug.</summary>
[Trait("Category", "Integration")]
public class MpvThreadGuardTests
{
    [Fact]
    public void Initialize_LoadsTheRealEngine()
    {
        using var ctx = new MpvContext();
        ctx.Initialize(); // throws if libmpv-2.dll can't load or mpv_initialize fails
    }

#if DEBUG
    [Fact]
    public void BlockingCall_OnTheRenderThread_FailsFast()
    {
        using var ctx = new MpvContext();
        ctx.Initialize();
        ctx.MarkRenderThread();

        // The whole point of the guard: a synchronous (blocking) mpv call from the marked render/UI thread
        // — the exact shape that deadlocked against a busy core in #33 — throws immediately instead.
        Assert.Throws<InvalidOperationException>(() => ctx.Command("set", "volume", "50"));
        Assert.Throws<InvalidOperationException>(() => ctx.GetPropertyDouble("volume"));
        Assert.Throws<InvalidOperationException>(() => ctx.GetPropertyString("media-title"));
    }

    [Fact]
    public void AsyncPaths_OnTheRenderThread_StayAllowed()
    {
        using var ctx = new MpvContext();
        ctx.Initialize();
        ctx.MarkRenderThread();

        // The deadlock-free paths the app actually uses must NOT trip the guard.
        Assert.Null(Record.Exception(() =>
        {
            ctx.CommandAsync("seek", "0", "absolute");
            ctx.SetProperty("volume", 50.0);
            ctx.SetProperty("pause", true);
            ctx.Loadfile("does-not-exist.mkv");
        }));
    }
#endif

    [Fact]
    public void AudioDeviceList_IsReadableThroughFlatProperties()
    {
        using var ctx = new MpvContext();
        ctx.Initialize();

        // This is the exact read path PlayerViewModel.ReadAudioDevices uses to populate the output-device
        // switcher: the flat audio-device-list/count + per-index name/description sub-properties.
        long count = ctx.GetPropertyLong("audio-device-list/count") ?? -1;
        Assert.True(count >= 0, "audio-device-list/count should marshal as a readable long");
        for (long i = 0; i < count; i++)
        {
            string? name = ctx.GetPropertyString($"audio-device-list/{i}/name");
            Assert.False(string.IsNullOrEmpty(name), $"device {i} must expose a name (the id we set audio-device to)");
            _ = ctx.GetPropertyString($"audio-device-list/{i}/description"); // optional, must at least not throw
        }

        // The property the picker writes is always present (defaults to "auto") and round-trips.
        Assert.NotNull(ctx.GetPropertyString("audio-device"));
    }

    [Fact]
    public void VideoAdjustmentProperties_AreValidAndWritable()
    {
        using var ctx = new MpvContext();
        ctx.Initialize();

        // The Video submenu writes exactly these properties — assert the names are real and accept our
        // values against libmpv (a typo'd property name would otherwise fail silently in the async path).
        ctx.Command("set", "video-rotate", "90");
        Assert.Equal(90, ctx.GetPropertyLong("video-rotate"));

        ctx.Command("set", "panscan", "1.0");
        Assert.Equal(1.0, ctx.GetPropertyDouble("panscan"));

        ctx.Command("set", "video-aspect-override", "16:9");
        Assert.False(string.IsNullOrEmpty(ctx.GetPropertyString("video-aspect-override")));

        // Reset values the "Reset video" item sends — "no" restores mpv's automatic aspect handling.
        ctx.Command("set", "video-rotate", "0");
        ctx.Command("set", "panscan", "0.0");
        ctx.Command("set", "video-aspect-override", "no");
        Assert.Equal(0, ctx.GetPropertyLong("video-rotate"));
        Assert.Equal(0.0, ctx.GetPropertyDouble("panscan"));
    }

    [Fact]
    public void SubtitleMargins_LiftEverySubtitleKind()
    {
        // mpv applies sub-margin-y (the property the OSC-lift toggles so controls never overlap captions,
        // PRD P1-D9) to text/bitmap subs by default, but ASS-styled subs are governed by sub-ass-override —
        // which defaults to "scale" and ignores margins. That default is the exact reason the lift silently
        // no-opped on ASS subtitles; sub-ass-force-margins defaults off, which is what we flip on.
        using (var bare = new MpvContext())
        {
            bare.Initialize();
            Assert.Equal("scale", bare.GetPropertyString("sub-ass-override"));
            Assert.Equal("no", bare.GetPropertyString("sub-ass-force-margins"));
        }

        // MpvVideoPanel sets these two as init options so the lift works for every subtitle kind. Assert
        // libmpv accepts them (a typo'd option would otherwise fail silently) and that they flip to "yes".
        // sub-ass-force-margins applies the margins to ASS subs WITHOUT restyling them (unlike
        // sub-ass-override=force), so their fonts/colors stay intact while the lift clears the OSC.
        using var ctx = new MpvContext();
        ctx.SetOption("sub-use-margins", "yes");
        ctx.SetOption("sub-ass-force-margins", "yes");
        ctx.Initialize();
        Assert.Equal("yes", ctx.GetPropertyString("sub-use-margins"));
        Assert.Equal("yes", ctx.GetPropertyString("sub-ass-force-margins"));
        Assert.Equal("scale", ctx.GetPropertyString("sub-ass-override")); // styling preserved, margins not forced via override

        // sub-margin-y is the property SetSubtitleMargin writes (128 chrome-up / 56 chrome-down) — confirm
        // the name is real and accepts the lift value against the actual engine.
        ctx.Command("set", "sub-margin-y", "128");
        Assert.Equal(128, ctx.GetPropertyLong("sub-margin-y"));
    }

    [Fact]
    public void CallsAfterDispose_AreNoOps_NotCrashes()
    {
        var ctx = new MpvContext();
        ctx.Initialize();
        ctx.Dispose();

        // After teardown, reads/commands must short-circuit instead of passing a freed handle to libmpv
        // (the disposal race an off-thread device read or a late device-switch click could otherwise hit).
        Assert.Null(ctx.GetPropertyLong("audio-device-list/count"));
        Assert.Null(ctx.GetPropertyString("audio-device"));
        Assert.Null(ctx.GetPropertyDouble("volume"));
        Assert.Null(ctx.GetPropertyBool("pause"));
        Assert.Null(Record.Exception(() => ctx.SetProperty("audio-device", "auto")));
        Assert.Null(Record.Exception(() => ctx.CommandAsync("af", "remove", "@okpnorm")));
    }

    [Fact]
    public void BlockingReads_FromOtherThreads_AreAllowed()
    {
        using var ctx = new MpvContext();
        ctx.Initialize();
        ctx.MarkRenderThread(); // marks THIS thread only

        // The event-pump and thumbnail engines read off the render thread; those must never be guarded.
        // Use a dedicated thread, not Task.Run: the threadpool can hand back the very thread that called
        // MarkRenderThread (xUnit itself runs on the pool), which would make this assertion flaky.
        Exception? captured = null;
        var reader = new System.Threading.Thread(() =>
        {
            try { ctx.GetPropertyDouble("volume"); }
            catch (Exception ex) { captured = ex; }
        });
        reader.Start();
        reader.Join();
        Assert.Null(captured);
    }
}
