using System.Collections.Generic;
using System.Linq;
using OkPlayer.Core;
using Xunit;

namespace OkPlayer.Tests;

public class SrtDocumentTests
{
    [Fact]
    public void ParsesBasicCues()
    {
        const string srt = """
            1
            00:00:01,000 --> 00:00:04,000
            The quick brown fox

            2
            00:00:05,500 --> 00:00:08,250
            jumps over
            the lazy dog
            """;
        var cues = SrtDocument.Parse(srt);
        Assert.Equal(2, cues.Count);
        Assert.Equal(1, cues[0].Index);
        Assert.Equal(1.0, cues[0].Start, 3);
        Assert.Equal(4.0, cues[0].End, 3);
        Assert.Equal("The quick brown fox", cues[0].Text);
        Assert.Equal(5.5, cues[1].Start, 3);
        Assert.Equal(8.25, cues[1].End, 3);
        Assert.Equal("jumps over the lazy dog", cues[1].Text); // multi-line joined
    }

    [Fact]
    public void StripsTags_AndToleratesDotMs_Crlf_Bom()
    {
        const string srt = "﻿1\r\n00:00:02.000 --> 00:00:03.000\r\n<i>Hello</i> {\\an8}world\r\n";
        var cues = SrtDocument.Parse(srt);
        Assert.Single(cues);
        Assert.Equal("Hello world", cues[0].Text);
        Assert.Equal(2.0, cues[0].Start, 3);
    }

    [Fact]
    public void ParsesWhenIndexLineMissing()
    {
        const string srt = "00:01:00,000 --> 00:01:02,000\nLine without an index";
        var cues = SrtDocument.Parse(srt);
        Assert.Single(cues);
        Assert.Equal(60.0, cues[0].Start, 3);
        Assert.Equal("Line without an index", cues[0].Text);
    }

    [Theory]
    [InlineData("")]
    [InlineData("   ")]
    [InlineData("not a subtitle file")]
    [InlineData("1\nno timecode here\njust text")]
    public void GarbageYieldsNoCues(string input) => Assert.Empty(SrtDocument.Parse(input));
}

public class SubtitleSyncAlignerTests
{
    // Three cues as AUTHORED in the .srt.
    private static readonly SrtCue[] Cues =
    {
        new(1, 10.0, 12.0, "The quick brown fox"),
        new(2, 13.0, 15.0, "jumps over the lazy dog"),
        new(3, 16.0, 18.0, "hello there general kenobi"),
    };

    // Build an ASR sample for the same lines spoken at `actualStart`, `actualStart+3`, `actualStart+6`.
    private static List<AsrToken> Spoken(double t1, double t2, double t3)
    {
        List<AsrToken> Words(string text, double start) =>
            text.Split(' ').Select((w, i) => new AsrToken(w, start + i * 0.4)).ToList();
        var list = new List<AsrToken>();
        list.AddRange(Words("the quick brown fox", t1));
        list.AddRange(Words("jumps over the lazy dog", t2));
        list.AddRange(Words("hello there general kenobi", t3));
        return list;
    }

    [Fact]
    public void SubtitlesEarly_ReturnsPositiveDelay()
    {
        // Audio actually happens 3 s LATER than the cues are authored → subs need +3 s delay.
        var asr = Spoken(13.0, 16.0, 19.0);
        var r = SubtitleSyncAligner.Align(asr, Cues);
        Assert.NotNull(r);
        Assert.Equal(3.0, r!.OffsetSeconds, 1);
        Assert.True(r.Votes >= 2);
        Assert.True(r.Confidence > 0.6);
    }

    [Fact]
    public void SubtitlesLate_ReturnsNegativeDelay()
    {
        // Audio happens 2 s EARLIER than authored → subs need −2 s delay.
        var asr = Spoken(8.0, 11.0, 14.0);
        var r = SubtitleSyncAligner.Align(asr, Cues);
        Assert.NotNull(r);
        Assert.Equal(-2.0, r!.OffsetSeconds, 1);
    }

    [Fact]
    public void AlreadyInSync_ReturnsNearZero()
    {
        var asr = Spoken(10.0, 13.0, 16.0);
        var r = SubtitleSyncAligner.Align(asr, Cues);
        Assert.NotNull(r);
        Assert.Equal(0.0, r!.OffsetSeconds, 1);
    }

    [Fact]
    public void ImperfectAsr_StillAligns()
    {
        // One word wrong / dropped per line — overlap match should still carry it.
        var asr = new List<AsrToken>
        {
            new("the", 13.0), new("quick", 13.4), new("BROWN", 13.8), // case-insensitive
            new("jumps", 16.0), new("over", 16.4), new("lazy", 17.2), // "the"/"dog" dropped
        };
        var r = SubtitleSyncAligner.Align(asr, Cues);
        Assert.NotNull(r);
        Assert.Equal(3.0, r!.OffsetSeconds, 1);
    }

    [Fact]
    public void NoMatch_ReturnsNull()
    {
        var asr = new List<AsrToken>
        {
            new("completely", 5.0), new("unrelated", 5.5), new("spoken", 6.0), new("words", 6.5),
        };
        Assert.Null(SubtitleSyncAligner.Align(asr, Cues));
    }

    [Fact]
    public void EmptyInputs_ReturnNull()
    {
        Assert.Null(SubtitleSyncAligner.Align(new List<AsrToken>(), Cues));
        Assert.Null(SubtitleSyncAligner.Align(Spoken(10, 13, 16), new List<SrtCue>()));
    }
}
