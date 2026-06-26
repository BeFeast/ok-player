using CommunityToolkit.Mvvm.ComponentModel;
using Microsoft.UI.Xaml;
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

/// <summary>An audio output device from libmpv's audio-device-list. <see cref="Name"/> is the mpv
/// device id passed to the <c>audio-device</c> property; <see cref="Label"/> is the display description.</summary>
public sealed class AudioDevice
{
    public string Name { get; init; } = string.Empty;
    public string Label { get; init; } = string.Empty;
    public bool Selected { get; init; }
}

/// <summary>A chapter from libmpv's chapter-list. <see cref="Thumbnail"/> fills in asynchronously.</summary>
public sealed partial class ChapterInfo : ObservableObject
{
    public int Index { get; init; }
    public string Title { get; init; } = string.Empty;
    public double Time { get; init; }
    public string TimeText { get; init; } = string.Empty;
    public bool IsUserDefined { get; init; } // user-added (editable) vs read-only from the file

    [ObservableProperty] private ImageSource? _thumbnail;
    [ObservableProperty] private bool _isCurrent; // the playing chapter (accent tint + inset bar)

    public Visibility CurrentBarVisibility => IsCurrent ? Visibility.Visible : Visibility.Collapsed;
    public Visibility EditVisibility => IsUserDefined ? Visibility.Visible : Visibility.Collapsed; // rename/delete affordances
    partial void OnIsCurrentChanged(bool value) => OnPropertyChanged(nameof(CurrentBarVisibility));
}
