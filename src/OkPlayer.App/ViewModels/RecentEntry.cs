using CommunityToolkit.Mvvm.ComponentModel;
using Microsoft.UI.Xaml.Media;

namespace OkPlayer.App.ViewModels;

/// <summary>A continue-watching poster card on the welcome screen, projected from a history record.</summary>
public sealed partial class RecentEntry : ObservableObject
{
    public string Path { get; set; } = string.Empty;
    public string Title { get; set; } = string.Empty;
    public string Meta { get; set; } = string.Empty;      // "2016 · 2h 16m" style
    public string TimeLeft { get; set; } = string.Empty;  // "16m left" badge
    public double Progress { get; set; }                   // 0..1, drives the progress bar
    public double ProgressPercent => Progress * 100;       // for ProgressBar.Value
    public double ProgressFillWidth => Progress * 200;     // px fill against the 200px shelf-card width
    public Brush? PlaceholderGradient { get; set; }        // shown until/without a poster
    [ObservableProperty] private ImageSource? _poster;     // cached poster frame (fills in async)
}

/// <summary>A saved bookmark shown in the Chapters panel's BOOKMARKS section.</summary>
public sealed class BookmarkEntry
{
    public double Time { get; set; }
    public string TimeText { get; set; } = string.Empty;
}

/// <summary>One file in the Up-Next panel's "from this folder" list (projected from the folder playlist).</summary>
public sealed class PlaylistRow
{
    public string Path { get; set; } = string.Empty;
    public string Title { get; set; } = string.Empty;
    public bool IsCurrent { get; set; }   // the now-playing file (highlighted)
    public bool IsNext { get; set; }      // the very next file (gets the NEXT badge)
    public bool IsWatched { get; set; }   // seen before (per-file memory) — dimmed + check glyph

    public Microsoft.UI.Xaml.Visibility CurrentVisibility => Vis(IsCurrent);
    public Microsoft.UI.Xaml.Visibility NextVisibility => Vis(IsNext);
    public Microsoft.UI.Xaml.Visibility CheckVisibility => Vis(IsWatched);
    public Microsoft.UI.Xaml.Visibility CircleVisibility => Vis(!IsWatched);
    public double RowOpacity => IsWatched && !IsCurrent ? 0.55 : 1.0;

    private static Microsoft.UI.Xaml.Visibility Vis(bool v) =>
        v ? Microsoft.UI.Xaml.Visibility.Visible : Microsoft.UI.Xaml.Visibility.Collapsed;
}
