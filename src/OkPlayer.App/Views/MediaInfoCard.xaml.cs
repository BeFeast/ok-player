using System;
using Microsoft.UI;
using Microsoft.UI.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using OkPlayer.App.ViewModels;

namespace OkPlayer.App.Views;

/// <summary>The Media-info card (design band 13) — two tabs (Streams · Stats for nerds). Bound to a
/// <see cref="MediaInfoViewModel"/> via DataContext; raises Close/Copy for the host to handle.</summary>
public sealed partial class MediaInfoCard : UserControl
{
    public event EventHandler? CloseRequested;
    public event EventHandler? CopyRequested;

    public MediaInfoCard()
    {
        InitializeComponent();
        DataContextChanged += (_, _) => UpdateTabs();
        UpdateTabs();
    }

    private MediaInfoViewModel? Vm => DataContext as MediaInfoViewModel;

    private void OnStreamsTab(object sender, RoutedEventArgs e) { if (Vm is { } vm) { vm.StreamsActive = true; UpdateTabs(); } }
    private void OnStatsTab(object sender, RoutedEventArgs e) { if (Vm is { } vm) { vm.StreamsActive = false; UpdateTabs(); } }
    private void OnClose(object sender, RoutedEventArgs e) => CloseRequested?.Invoke(this, EventArgs.Empty);
    private void OnCopyAll(object sender, RoutedEventArgs e) => CopyRequested?.Invoke(this, EventArgs.Empty);

    private static readonly SolidColorBrush ActivePill = new(Colors.White);
    private static readonly SolidColorBrush ActiveText = new(Windows.UI.Color.FromArgb(0xFF, 0x0A, 0x65, 0x5F));
    private static readonly SolidColorBrush InactiveText = new(Windows.UI.Color.FromArgb(0x80, 0, 0, 0));
    private static readonly SolidColorBrush Transparent = new(Colors.Transparent);

    private void UpdateTabs()
    {
        bool streams = Vm?.StreamsActive ?? true;
        StyleTab(StreamsTab, streams);
        StyleTab(StatsTab, !streams);
    }

    private static void StyleTab(Button b, bool active)
    {
        b.Background = active ? ActivePill : Transparent;
        b.Foreground = active ? ActiveText : InactiveText;
        b.FontWeight = active ? FontWeights.SemiBold : FontWeights.Medium;
    }
}
