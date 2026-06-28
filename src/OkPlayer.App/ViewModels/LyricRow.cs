using CommunityToolkit.Mvvm.ComponentModel;
using Microsoft.UI.Text;

namespace OkPlayer.App.ViewModels;

/// <summary>One line in the synced-lyrics overlay. <see cref="IsActive"/> (the line the playhead is on) drives
/// the visual emphasis purely through binding — the current line is bright + semibold, the rest dim — so the
/// karaoke highlight is just a flag flip per tick, no per-line restyling in code.</summary>
public partial class LyricRow : ObservableObject
{
    /// <summary>The lyric text (word-timestamp tags already stripped). Empty for a deliberate instrumental gap.</summary>
    public string Text { get; }

    /// <summary>When this line becomes active, in seconds. Zero for plain (untimed) lyrics — click-to-seek and
    /// the highlight are disabled in that case.</summary>
    public double Time { get; }

    public LyricRow(string text, double time)
    {
        Text = text;
        Time = time;
    }

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(LineOpacity))]
    [NotifyPropertyChangedFor(nameof(LineWeight))]
    private bool _isActive;

    public double LineOpacity => IsActive ? 1.0 : 0.42;
    // FontWeight is Windows.UI.Text.* but FontWeights is Microsoft.UI.Text.* (the WinUI 3 split).
    public Windows.UI.Text.FontWeight LineWeight => IsActive ? FontWeights.SemiBold : FontWeights.Normal;
}
