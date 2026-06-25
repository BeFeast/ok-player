using System.Collections.ObjectModel;
using CommunityToolkit.Mvvm.ComponentModel;
using Microsoft.UI;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Media;

namespace OkPlayer.App.ViewModels;

/// <summary>Shared light-theme tokens for the Media-info card (design band 13). Kept here so the
/// data rows can expose ready-to-bind brushes/fonts and the XAML stays declarative.</summary>
internal static class MediaInfoTokens
{
    public static readonly FontFamily Mono = new("Cascadia Code, Consolas");
    public static readonly FontFamily Segoe = new("Segoe UI Variable Text, Segoe UI");
    public static readonly SolidColorBrush Value = new(Windows.UI.Color.FromArgb(0xFF, 0x1A, 0x1A, 0x1A));
    public static readonly SolidColorBrush Accent = new(Windows.UI.Color.FromArgb(0xFF, 0x0C, 0x7C, 0x75));
    public static readonly SolidColorBrush Transparent = new(Colors.Transparent);
    public static readonly SolidColorBrush TrackTint = new(Windows.UI.Color.FromArgb(0x12, 0x10, 0x93, 0x8A));
    public static readonly SolidColorBrush TrackBorder = new(Windows.UI.Color.FromArgb(0x29, 0x10, 0x93, 0x8A));
}

/// <summary>One label → value row in a Streams/Stats section.</summary>
public sealed class InfoRow
{
    public string Label { get; set; } = string.Empty;
    public string Value { get; set; } = string.Empty;
    public bool Mono { get; set; }
    public bool Accent { get; set; }

    public FontFamily ValueFontFamily => Mono ? MediaInfoTokens.Mono : MediaInfoTokens.Segoe;
    public Brush ValueForeground => Accent ? MediaInfoTokens.Accent : MediaInfoTokens.Value;
    public Windows.UI.Text.FontWeight ValueWeight => Accent ? Microsoft.UI.Text.FontWeights.SemiBold : Microsoft.UI.Text.FontWeights.Medium;
}

/// <summary>A section card with an eyebrow (+ optional id-chip / badge) and label/value rows laid out in
/// two columns (rows fill left→right, matching the design's 1fr 1fr grid).</summary>
public sealed class InfoSection
{
    public string Eyebrow { get; set; } = string.Empty;
    public string? IdChip { get; set; } // e.g. "#0:0" on VIDEO
    public string? Badge { get; set; }  // e.g. "HDR10"
    public bool BadgeAmber { get; set; }
    public ObservableCollection<InfoRow> Left { get; } = new();
    public ObservableCollection<InfoRow> Right { get; } = new();

    public void Add(string label, string? value, bool mono = false, bool accent = false)
    {
        if (string.IsNullOrWhiteSpace(value))
            return;
        var row = new InfoRow { Label = label, Value = value!, Mono = mono, Accent = accent };
        if ((Left.Count + Right.Count) % 2 == 0)
            Left.Add(row);
        else
            Right.Add(row);
    }

    public int Count => Left.Count + Right.Count;
    public Visibility IdChipVisibility => string.IsNullOrEmpty(IdChip) ? Visibility.Collapsed : Visibility.Visible;
    public Visibility BadgeVisibility => string.IsNullOrEmpty(Badge) ? Visibility.Collapsed : Visibility.Visible;
}

/// <summary>One audio/subtitle track row.</summary>
public sealed class TrackRow
{
    public string IdChip { get; set; } = string.Empty;
    public string Title { get; set; } = string.Empty;
    public string Detail { get; set; } = string.Empty;
    public bool Highlight { get; set; } // default/active track → teal tint row + bold title
    public string? Badge { get; set; }  // DEFAULT / ON / EXT

    public Windows.UI.Text.FontWeight TitleWeight => Highlight ? Microsoft.UI.Text.FontWeights.SemiBold : Microsoft.UI.Text.FontWeights.Medium;
    public Visibility BadgeVisibility => string.IsNullOrEmpty(Badge) ? Visibility.Collapsed : Visibility.Visible;
    public Brush RowBackground => Highlight ? MediaInfoTokens.TrackTint : MediaInfoTokens.Transparent;
    public Brush RowBorder => Highlight ? MediaInfoTokens.TrackBorder : MediaInfoTokens.Transparent;
}

/// <summary>An audio or subtitle track section (eyebrow + rows). Hidden when there are no tracks.</summary>
public sealed class TrackSection
{
    public string Eyebrow { get; set; } = string.Empty;
    public ObservableCollection<TrackRow> Tracks { get; } = new();
    public Visibility Visibility => Tracks.Count > 0 ? Microsoft.UI.Xaml.Visibility.Visible : Microsoft.UI.Xaml.Visibility.Collapsed;
}

/// <summary>Backs the Media-info card (design band 13): two tabs — Streams and Stats for nerds.</summary>
public sealed partial class MediaInfoViewModel : ObservableObject
{
    public string FileName { get; set; } = string.Empty;
    public string DirectoryPath { get; set; } = string.Empty;

    // Streams tab
    public ObservableCollection<InfoSection> StreamSections { get; } = new(); // FILE, VIDEO, HDR (HDR omitted if SDR)
    public TrackSection AudioSection { get; } = new() { Eyebrow = "AUDIO" };
    public TrackSection SubtitleSection { get; } = new() { Eyebrow = "SUBTITLES" };

    // Stats tab
    public ObservableCollection<InfoSection> StatsSections { get; } = new(); // DECODE, LIVE, DISPLAY

    [ObservableProperty] private bool _streamsActive = true;

    public Visibility StreamsVisibility => StreamsActive ? Visibility.Visible : Visibility.Collapsed;
    public Visibility StatsVisibility => StreamsActive ? Visibility.Collapsed : Visibility.Visible;

    partial void OnStreamsActiveChanged(bool value)
    {
        OnPropertyChanged(nameof(StreamsVisibility));
        OnPropertyChanged(nameof(StatsVisibility));
    }
}
