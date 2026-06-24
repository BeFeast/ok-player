using CommunityToolkit.Mvvm.ComponentModel;
using Microsoft.UI.Xaml.Media;

namespace OkPlayer.App.ViewModels;

/// <summary>A selectable subtitle or audio track from libmpv's track-list.</summary>
public sealed class TrackInfo
{
    public long Id { get; init; }
    public string Label { get; init; } = string.Empty;
    public bool Selected { get; init; }
    public bool External { get; init; }
}

/// <summary>A chapter from libmpv's chapter-list. <see cref="Thumbnail"/> fills in asynchronously.</summary>
public sealed partial class ChapterInfo : ObservableObject
{
    public int Index { get; init; }
    public string Title { get; init; } = string.Empty;
    public double Time { get; init; }
    public string TimeText { get; init; } = string.Empty;

    [ObservableProperty] private ImageSource? _thumbnail;
}
