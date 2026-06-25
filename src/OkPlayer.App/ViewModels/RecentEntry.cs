namespace OkPlayer.App.ViewModels;

/// <summary>A continue-watching card on the welcome screen, projected from a history record.</summary>
public sealed class RecentEntry
{
    public string Path { get; set; } = string.Empty;
    public string Title { get; set; } = string.Empty;
    public string Meta { get; set; } = string.Empty; // e.g. "45% · 1:23:45 left"
    public double Progress { get; set; }              // 0..1, drives the card's progress bar
    public double ProgressPercent => Progress * 100;  // for ProgressBar.Value
}

/// <summary>A saved bookmark shown in the Chapters panel's BOOKMARKS section.</summary>
public sealed class BookmarkEntry
{
    public double Time { get; set; }
    public string TimeText { get; set; } = string.Empty;
}
