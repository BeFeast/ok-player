using CommunityToolkit.Mvvm.ComponentModel;
using Microsoft.UI.Xaml;
using OkPlayer.Render;

namespace OkPlayer.App.ViewModels;

/// <summary>One editable <c>key = value</c> row in Settings → Advanced (the mpv.conf escape hatch). The
/// Key/Value are two-way bound to a pair of TextBoxes and map 1:1 to an
/// <see cref="OkPlayer.App.Services.MpvOption"/> on save (the code-behind serialises the rows via
/// <see cref="OkPlayer.App.Services.MpvConfText"/>). <see cref="ProtectedVisibility"/> surfaces an inline
/// hint when the key is an OK Player-managed option the engine loader will silently skip.</summary>
public sealed partial class MpvOptionRow : ObservableObject
{
    [ObservableProperty] private string _key = string.Empty;
    [ObservableProperty] private string _value = string.Empty;

    public MpvOptionRow() { }

    public MpvOptionRow(string key, string value)
    {
        _key = key;
        _value = value;
    }

    /// <summary>Set when the row was just appended via "Add option", so its key field grabs focus once its
    /// container is realised (see <c>SettingsWindow.OnRowKeyLoaded</c>). A one-shot, non-bindable flag — the
    /// container for a freshly added ItemsControl item isn't available synchronously when the row is added.</summary>
    public bool AutoFocus { get; set; }

    /// <summary>Visible when the key is an engine-managed option the loader ignores, so the user learns the
    /// row won't take effect — without being blocked from typing it. Recomputed whenever the key changes.</summary>
    public Visibility ProtectedVisibility =>
        MpvVideoPanel.IsProtectedOption(Key) ? Visibility.Visible : Visibility.Collapsed;

    // The generated Key setter has already updated the backing field, so re-reading Key here yields the new
    // value; nudge the dependent visibility so the hint glyph tracks edits live, keystroke by keystroke.
    partial void OnKeyChanged(string value) => OnPropertyChanged(nameof(ProtectedVisibility));
}
