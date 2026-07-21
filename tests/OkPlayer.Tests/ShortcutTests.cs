using OkPlayer.Core;

namespace OkPlayer.Tests;

public class ShortcutTests
{
    private static ShortcutModifiers Ctrl() => new(Ctrl: true);
    private static ShortcutModifiers Shift() => new(Shift: true);
    private static ShortcutModifiers CtrlShift() => new(Ctrl: true, Shift: true);
    private static ShortcutChord Chord(string text) => ShortcutModel.ParseChord(text);

    [Fact]
    public void Parser_AcceptsActionOverrides()
    {
        var bindings = ShortcutModel.ResolvedBindingsFromText("play-pause=P\ncopy-frame=Ctrl+Shift+C\n");

        Assert.Equal(ShortcutAction.PlayPause, ShortcutModel.ActionForKey(bindings, "p", default));
        Assert.Null(ShortcutModel.ActionForKey(bindings, "space", default));
        Assert.Equal(ShortcutAction.CopyFrame, ShortcutModel.ActionForKey(bindings, "C", CtrlShift()));
    }

    [Fact]
    public void Parser_AcceptsSecondaryActionBinding()
    {
        var bindings = ShortcutModel.ResolvedBindingsFromText("play-pause=Space\nplay-pause=P\n");

        Assert.Equal(ShortcutAction.PlayPause, ShortcutModel.ActionForKey(bindings, "space", default));
        Assert.Equal(ShortcutAction.PlayPause, ShortcutModel.ActionForKey(bindings, "p", default));
    }

    [Fact]
    public void Parser_SkipsCommentsAndBlankLines()
    {
        var bindings = ShortcutModel.ResolvedBindingsFromText("# a comment\n; also a comment\n\nplay-pause=P\n");
        Assert.Equal(ShortcutAction.PlayPause, ShortcutModel.ActionForKey(bindings, "p", default));
    }

    [Fact]
    public void Parser_RejectsUnknownAction()
    {
        var error = Assert.Throws<ShortcutConfigException>(() => ShortcutModel.ResolvedBindingsFromText("dance=Space"));
        Assert.Equal(1, error.Line);
        Assert.Contains("Unknown action", error.Message);
    }

    [Fact]
    public void Parser_RejectsUnknownKey()
    {
        var error = Assert.Throws<ShortcutConfigException>(() => ShortcutModel.ResolvedBindingsFromText("play-pause=HyperDrive"));
        Assert.Equal(1, error.Line);
        Assert.Contains("Unknown key", error.Message);
    }

    [Fact]
    public void Parser_RejectsConflictingBindings()
    {
        var error = Assert.Throws<ShortcutConfigException>(() => ShortcutModel.ResolvedBindingsFromText("play-pause=C"));
        Assert.Equal(0, error.Line);
        Assert.Contains("conflicts", error.Message);
    }

    [Fact]
    public void Parser_KeepsSavedBindingThatConflictsWithNewDefault()
    {
        var bindings = ShortcutModel.ResolvedBindingsFromText("seek-back=Ctrl+Left");

        Assert.Equal(ShortcutAction.SeekBack, ShortcutModel.ActionForKey(bindings, "Left", Ctrl()));
        Assert.DoesNotContain(bindings, binding =>
            binding.Action == ShortcutAction.SubtitlePreviousCue && binding.Chord == Chord("Ctrl+Left"));
    }

    [Fact]
    public void Parser_RejectsThirdActionBinding()
    {
        var error = Assert.Throws<ShortcutConfigException>(() => ShortcutModel.ResolvedBindingsFromText(
            "play-pause=Space\nplay-pause=P\nplay-pause=Ctrl+P\n"));
        Assert.Equal(3, error.Line);
        Assert.Contains("at most two", error.Message);
    }

    [Fact]
    public void Defaults_KeepShiftCopyFrameDistinct()
    {
        var bindings = ShortcutModel.DefaultBindings();
        Assert.Equal(ShortcutAction.SaveScreenshot, ShortcutModel.ActionForKey(bindings, "c", default));
        Assert.Equal(ShortcutAction.CopyFrame, ShortcutModel.ActionForKey(bindings, "C", Shift()));
    }

    [Fact]
    public void ConfigText_SerializesOnlyCustomBindings()
    {
        var bindings = ShortcutModel.DefaultBindings().ToList();
        int index = bindings.FindIndex(binding => binding.Action == ShortcutAction.PlayPause);
        bindings[index] = new ShortcutBinding(ShortcutAction.PlayPause, Chord("P"));

        string text = ShortcutModel.ConfigTextFromBindings(bindings);
        Assert.Equal("play-pause=P", text);
        ShortcutModel.ResolvedBindingsFromText(text);
    }

    [Fact]
    public void ConfigText_SerializesSecondaryBindings()
    {
        var bindings = ShortcutModel.DefaultBindings().ToList();
        bindings.Add(new ShortcutBinding(ShortcutAction.PlayPause, Chord("P")));

        string text = ShortcutModel.ConfigTextFromBindings(bindings);
        Assert.Equal("play-pause=Space\nplay-pause=P", text);
        var resolved = ShortcutModel.ResolvedBindingsFromText(text);
        Assert.Equal(ShortcutAction.PlayPause, ShortcutModel.ActionForKey(resolved, "space", default));
        Assert.Equal(ShortcutAction.PlayPause, ShortcutModel.ActionForKey(resolved, "p", default));
    }

    [Fact]
    public void ConfigText_ReturnsBlankForDefaults()
        => Assert.Equal("", ShortcutModel.ConfigTextFromBindings(ShortcutModel.DefaultBindings()));

    [Fact]
    public void Capture_RejectsModifierOnlyKeys()
    {
        Assert.False(ShortcutModel.TryChordFromCapturedKey("Shift_L", Shift(), out _, out string? error));
        Assert.Equal("Press a non-modifier key.", error);

        Assert.True(ShortcutModel.TryChordFromCapturedKey("comma", Ctrl(), out ShortcutChord? chord, out _));
        Assert.Equal("Ctrl+,", chord!.Label);
    }

    [Fact]
    public void Capture_RejectsNamelessKeys()
        => Assert.False(ShortcutModel.TryChordFromCapturedKey(null, default, out _, out _));

    [Fact]
    public void Labels_KeepLetterODistinctFromZero()
        => Assert.Equal("O", Chord("O").Label);

    [Fact]
    public void SlotConflict_IgnoresOnlySlotBeingReassigned()
    {
        var rows = new[]
        {
            new ActionChords(ShortcutAction.PlayPause, Chord("Space"), Chord("P")),
            new ActionChords(ShortcutAction.Fullscreen, Chord("F"), null),
        };

        Assert.Null(ShortcutModel.SlotConflict(rows, ShortcutAction.PlayPause, ShortcutSlot.Primary, Chord("Space")));
        Assert.Equal(ShortcutAction.Fullscreen,
            ShortcutModel.SlotConflict(rows, ShortcutAction.PlayPause, ShortcutSlot.Primary, Chord("F")));
        Assert.Equal(ShortcutAction.PlayPause,
            ShortcutModel.SlotConflict(rows, ShortcutAction.PlayPause, ShortcutSlot.Primary, Chord("P")));
        Assert.Equal(ShortcutAction.PlayPause,
            ShortcutModel.SlotConflict(rows, ShortcutAction.Fullscreen, ShortcutSlot.Secondary, Chord("P")));
    }

    [Fact]
    public void ActionChords_FlattenToBindingsInRowOrder()
    {
        var rows = new[]
        {
            new ActionChords(ShortcutAction.PlayPause, Chord("Space"), Chord("P")),
            new ActionChords(ShortcutAction.Fullscreen, Chord("F"), null),
        };

        Assert.Equal(new[]
        {
            new ShortcutBinding(ShortcutAction.PlayPause, Chord("Space")),
            new ShortcutBinding(ShortcutAction.PlayPause, Chord("P")),
            new ShortcutBinding(ShortcutAction.Fullscreen, Chord("F")),
        }, ShortcutModel.BindingsFromActionChords(rows));
    }
}
