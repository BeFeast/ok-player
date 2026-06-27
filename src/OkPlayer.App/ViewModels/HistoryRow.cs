using System.Collections.Generic;
using CommunityToolkit.Mvvm.ComponentModel;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Data;
using Microsoft.UI.Xaml.Media;
using OkPlayer.App.Services;

namespace OkPlayer.App.ViewModels;

/// <summary>One row in the History list, projected from a <see cref="FileRecord"/> via
/// <see cref="HistoryFormat"/>. Carries everything the row template binds: the title/folder/when text,
/// the resume-state (a finished chip, a "time left" countdown, or a "barely started" hint), the
/// thumbnail (poster or a placeholder gradient) and a <see cref="Hovered"/> flag that reveals the row
/// background + overflow button on pointer-over.</summary>
public sealed partial class HistoryRow : ObservableObject
{
    public string Path { get; init; } = string.Empty;
    public string Title { get; init; } = string.Empty;
    public string Folder { get; init; } = string.Empty;
    public string When { get; init; } = string.Empty;
    public HistoryStateKind StateKind { get; init; }
    public string StateLabel { get; init; } = string.Empty;
    public double Percent { get; init; } // 0..1 watched fraction, drives the thumbnail fill

    public Brush? PlaceholderGradient { get; init; } // shown until/without a poster
    [ObservableProperty] private ImageSource? _poster;
    [ObservableProperty] private bool _hovered;       // pointer-over: reveals row bg + the ⋮ button

    public bool IsFinished => StateKind == HistoryStateKind.Finished;
    public Visibility FinishedVisibility => Vis(IsFinished);
    public Visibility ProgressVisibility => Vis(StateKind == HistoryStateKind.Progress);
    public Visibility BarelyVisibility => Vis(StateKind == HistoryStateKind.Barely);
    public Visibility FillVisibility => Vis(!IsFinished);
    // Finished thumbnails read as "done" — dimmed; in-progress ones stay full strength.
    public double ThumbOpacity => IsFinished ? 0.5 : 1.0;
    // Fill width against the 64px thumb; a small floor so a just-started file still shows a sliver.
    public double FillWidth => IsFinished ? 0 : System.Math.Max(3, Percent * 64);

    private static Visibility Vis(bool b) => b ? Visibility.Visible : Visibility.Collapsed;
}

/// <summary>A day-bucket group in the History list (e.g. "TODAY") and its rows. Search mode collapses
/// every match into one header-less group.</summary>
public sealed class HistoryGroup
{
    public string Header { get; init; } = string.Empty;
    public bool ShowHeader { get; init; }
    public Visibility HeaderVisibility => ShowHeader ? Visibility.Visible : Visibility.Collapsed;
    public IReadOnlyList<HistoryRow> Rows { get; init; } = System.Array.Empty<HistoryRow>();
}

/// <summary>Maps a bool to a 0/1 opacity so a <see cref="HistoryRow.Hovered"/> flag can drive the
/// fade-in of the row background and overflow button without retemplating or visual states.</summary>
public sealed partial class BoolToOpacityConverter : IValueConverter
{
    public object Convert(object value, System.Type targetType, object parameter, string language)
        => value is true ? 1.0 : 0.0;

    public object ConvertBack(object value, System.Type targetType, object parameter, string language)
        => throw new System.NotSupportedException();
}
