using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Linq;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Media.Imaging;
using OkPlayer.App.Services;
using OkPlayer.App.ViewModels;
using Windows.ApplicationModel.DataTransfer;
using Windows.Foundation;
using Windows.System;

namespace OkPlayer.App.Views;

/// <summary>The full History surface (Claude Design "OK Player — History"): a canvas-takeover idle state
/// listing everything ever opened, grouped by day, with search, per-row resume/remove and the welcome
/// footer reused with its left slot flipped. Reads <see cref="App.History"/>; raises events the host
/// (<see cref="PlayerView"/>) turns into opens, a close, or Settings navigation.</summary>
public sealed partial class HistoryView : UserControl
{
    /// <summary>A request to play a history row: resume from the saved position, or restart from 0.</summary>
    public readonly record struct OpenRequest(string Path, bool FromStart);

    public event EventHandler<OpenRequest>? OpenRequested;
    public event EventHandler? CloseRequested;
    public event EventHandler? SettingsRequested;
    public event EventHandler<string>? ToastRequested;

    private readonly HistoryService _history = App.History;
    private readonly List<(HistoryRow Row, HistoryBucket Bucket)> _allRows = new();
    private string _query = string.Empty;
    private bool _suppressSearch;   // guards programmatic SearchBox.Text changes
    private bool _sawItems;         // distinguishes "first run" (never had history) from "cleared"
    private bool _reloading;        // guards against our own Remove re-triggering a full reload via Changed

    public HistoryView()
    {
        InitializeComponent();
        KeyDown += OnViewKeyDown;
        // Reflect out-of-band history changes (Settings "Clear history" / retention prune) while open.
        _history.Changed += OnHistoryChanged;
        Unloaded += (_, _) => _history.Changed -= OnHistoryChanged; // shared instance outlives the view
    }

    private void OnHistoryChanged()
    {
        if (_reloading)
            return; // our own Remove already updated the list in place
        DispatcherQueue.TryEnqueue(() => { if (Visibility == Visibility.Visible) Load(); });
    }

    /// <summary>(Re)populate from history. Call each time the surface is shown.</summary>
    public void Load()
    {
        _suppressSearch = true;
        SearchBox.Text = string.Empty;
        _suppressSearch = false;
        _query = string.Empty;
        SearchClear.Visibility = Visibility.Collapsed; // OnSearchChanged is suppressed above, so clear it here

        _allRows.Clear();
        DateTime now = DateTime.Now;
        foreach (var (path, rec) in _history.All())
        {
            DateTime opened = ParseLocal(rec.LastOpenedUtc, now);
            var state = HistoryFormat.DeriveState(rec.Position, rec.Duration, rec.Finished);
            var row = new HistoryRow
            {
                Path = path,
                Title = string.IsNullOrEmpty(rec.Title) ? Path.GetFileNameWithoutExtension(path) : rec.Title!,
                Folder = HistoryFormat.FolderLabel(path),
                When = HistoryFormat.WhenLabel(opened, now),
                StateKind = state.Kind,
                StateLabel = state.Label,
                Percent = state.Percent,
                PlaceholderGradient = PosterGradient(_allRows.Count),
            };
            if (!string.IsNullOrEmpty(rec.PosterPath) && File.Exists(rec.PosterPath))
            {
                // Decode to ~thumbnail size, not the poster's native frame size — a long history would
                // otherwise hold hundreds of full-res bitmaps for 64px rows.
                try { row.Poster = PosterImage.Load(rec.PosterPath!, decodePixelWidth: 128); }
                catch { /* unreadable poster -> gradient */ }
            }
            _allRows.Add((row, HistoryFormat.BucketFor(opened, now)));
        }

        if (_allRows.Count > 0)
            _sawItems = true;
        Rebuild();
        Scroll.ChangeView(null, 0, null, disableAnimation: true);
    }

    /// <summary>Rebuild the visible groups/state for the current query without re-reading history.</summary>
    private void Rebuild()
    {
        bool isPrivate = _history.Private;
        string q = _query.Trim();

        if (_allRows.Count == 0)
        {
            SearchWrap.Visibility = Visibility.Collapsed;
            PrivateBanner.Visibility = Visibility.Collapsed;
            if (_sawItems)
                ShowState("", "History cleared", "Nothing left to show. New files you open will start a fresh history.", clearButton: false);
            else
                ShowState("", "Nothing here yet", "Files you open will show up here — in progress, finished, and everything in between.", clearButton: false);
            RefreshFooter(isPrivate);
            return;
        }

        SearchWrap.Visibility = Visibility.Visible;
        PrivateBanner.Visibility = isPrivate ? Visibility.Visible : Visibility.Collapsed;

        if (q.Length > 0)
        {
            var matches = _allRows.Where(x => Match(x.Row, q)).Select(x => x.Row).ToList();
            if (matches.Count == 0)
            {
                ShowState("", "No matches", $"Nothing in your history matches “{_query.Trim()}”.", clearButton: true);
                RefreshFooter(isPrivate);
                return;
            }
            ResultsCaption.Text = $"{matches.Count} result{(matches.Count == 1 ? "" : "s")}";
            ResultsCaption.Visibility = Visibility.Visible;
            GroupsList.ItemsSource = new[] { new HistoryGroup { Header = string.Empty, ShowHeader = false, Rows = matches } };
        }
        else
        {
            ResultsCaption.Visibility = Visibility.Collapsed;
            var groups = new List<HistoryGroup>();
            foreach (HistoryBucket bucket in new[] { HistoryBucket.Today, HistoryBucket.Yesterday, HistoryBucket.EarlierThisWeek, HistoryBucket.Earlier })
            {
                var rows = _allRows.Where(x => x.Bucket == bucket).Select(x => x.Row).ToList();
                if (rows.Count > 0)
                    groups.Add(new HistoryGroup { Header = HistoryFormat.BucketHeader(bucket), ShowHeader = true, Rows = rows });
            }
            GroupsList.ItemsSource = groups;
        }

        ListPanel.Visibility = Visibility.Visible;
        StateCard.Visibility = Visibility.Collapsed;
        RefreshFooter(isPrivate);
    }

    private static bool Match(HistoryRow row, string q)
        => (row.Title + " " + row.Folder).Contains(q, StringComparison.OrdinalIgnoreCase);

    private void ShowState(string glyph, string title, string body, bool clearButton)
    {
        StateIcon.Glyph = glyph;
        StateTitle.Text = title;
        StateBody.Text = body;
        StateButton.Visibility = clearButton ? Visibility.Visible : Visibility.Collapsed;
        StateCard.Visibility = Visibility.Visible;
        ListPanel.Visibility = Visibility.Collapsed;
    }

    private void RefreshFooter(bool isPrivate)
    {
        FooterPill.Text = isPrivate ? "Private mode" : "Recording history";
        FooterDot.Opacity = isPrivate ? 0.4 : 1.0;
    }

    private static DateTime ParseLocal(string iso, DateTime fallback)
        => DateTime.TryParse(iso, CultureInfo.InvariantCulture, DateTimeStyles.RoundtripKind, out var t)
            ? t.ToLocalTime() : fallback;

    // ---- search ----

    private void OnSearchChanged(object sender, TextChangedEventArgs e)
    {
        if (_suppressSearch)
            return;
        _query = SearchBox.Text;
        SearchClear.Visibility = string.IsNullOrEmpty(_query) ? Visibility.Collapsed : Visibility.Visible;
        Rebuild();
    }

    private void OnSearchKeyDown(object sender, KeyRoutedEventArgs e)
    {
        if (e.Key != VirtualKey.Escape)
            return;
        if (!string.IsNullOrEmpty(SearchBox.Text))
            SearchBox.Text = string.Empty;       // first Esc clears the query
        else
            CloseRequested?.Invoke(this, EventArgs.Empty); // a second Esc closes History
        e.Handled = true;
    }

    private void OnClearSearchClick(object sender, RoutedEventArgs e)
    {
        SearchBox.Text = string.Empty;
        SearchBox.Focus(FocusState.Programmatic);
    }

    private void OnViewKeyDown(object sender, KeyRoutedEventArgs e)
    {
        var focused = FocusManager.GetFocusedElement(XamlRoot) as FrameworkElement;
        bool inSearch = ReferenceEquals(focused, SearchBox);
        if (e.Key == (VirtualKey)0xBF && !inSearch && SearchWrap.Visibility == Visibility.Visible) // "/" focuses search
        {
            SearchBox.Focus(FocusState.Programmatic);
            e.Handled = true;
        }
        else if (e.Key == VirtualKey.Escape && !inSearch)
        {
            CloseRequested?.Invoke(this, EventArgs.Empty);
            e.Handled = true;
        }
    }

    // ---- navigation / footer ----

    private void OnBackClick(object sender, RoutedEventArgs e) => CloseRequested?.Invoke(this, EventArgs.Empty);
    private void OnFooterLeftClick(object sender, RoutedEventArgs e) => CloseRequested?.Invoke(this, EventArgs.Empty);
    private void OnSettingsClick(object sender, RoutedEventArgs e) => SettingsRequested?.Invoke(this, EventArgs.Empty);
    private void OnRetentionClick(Microsoft.UI.Xaml.Documents.Hyperlink sender, Microsoft.UI.Xaml.Documents.HyperlinkClickEventArgs args)
        => SettingsRequested?.Invoke(this, EventArgs.Empty);

    // ---- row interaction ----

    private void OnRowPointerEntered(object sender, PointerRoutedEventArgs e)
    {
        if ((sender as FrameworkElement)?.DataContext is HistoryRow row)
            row.Hovered = true;
    }

    private void OnRowPointerExited(object sender, PointerRoutedEventArgs e)
    {
        if ((sender as FrameworkElement)?.DataContext is HistoryRow row)
            row.Hovered = false;
    }

    private void OnRowTapped(object sender, TappedRoutedEventArgs e)
    {
        if ((sender as FrameworkElement)?.DataContext is HistoryRow row)
            OpenRequested?.Invoke(this, new OpenRequest(row.Path, FromStart: false));
    }

    private void OnRowContextRequested(UIElement sender, ContextRequestedEventArgs e)
    {
        if ((sender as FrameworkElement)?.DataContext is not HistoryRow row)
            return;
        Point pt = e.TryGetPosition(sender, out var p) ? p : new Point(0, 0);
        ShowRowMenu(row, (FrameworkElement)sender, pt);
        e.Handled = true;
    }

    private void OnRowMenuClick(object sender, RoutedEventArgs e)
    {
        if (sender is FrameworkElement fe && fe.DataContext is HistoryRow row)
            ShowRowMenu(row, fe, null);
    }

    /// <summary>The overflow button opens its menu via <see cref="OnRowMenuClick"/> (Click). Swallow the
    /// Tapped that bubbles up from the same press so the row's <see cref="OnRowTapped"/> doesn't also fire and
    /// open the file — which would dismiss the just-opened menu and make the button look like it only works on
    /// right-click.</summary>
    private void OnRowMenuButtonTapped(object sender, TappedRoutedEventArgs e) => e.Handled = true;

    /// <summary>Swallow the surface-level right-click so the player's context menu never shows over
    /// History; rows that want a menu handle ContextRequested themselves (and mark it handled first).</summary>
    private void OnSurfaceContextRequested(UIElement sender, ContextRequestedEventArgs e) => e.Handled = true;

    private void ShowRowMenu(HistoryRow row, FrameworkElement target, Point? at)
    {
        var menu = new MenuFlyout();
        if (Application.Current.Resources.TryGetValue("OkMenuFlyoutPresenterStyle", out var style) && style is Style s)
            menu.MenuFlyoutPresenterStyle = s;

        // "Resume" only when there is somewhere to resume to (matches the design: not finished, started).
        bool resumable = row.StateKind != HistoryStateKind.Finished && row.Percent > 0;
        if (resumable)
            menu.Items.Add(MenuItem("Resume", (_, _) => Open(row, fromStart: false)));
        menu.Items.Add(MenuItem("Play from start", (_, _) => Open(row, fromStart: true)));

        menu.Items.Add(new MenuFlyoutSeparator());
        menu.Items.Add(MenuItem("Reveal in Explorer", (_, _) => Reveal(row)));
        menu.Items.Add(MenuItem("Copy path", (_, _) => CopyPath(row)));
        menu.Items.Add(new MenuFlyoutSeparator());
        menu.Items.Add(MenuItem("Remove from history", (_, _) => Remove(row)));

        if (at is { } pt)
            menu.ShowAt(target, new FlyoutShowOptions { Position = pt });
        else
            menu.ShowAt(target);
    }

    private static MenuFlyoutItem MenuItem(string text, RoutedEventHandler onClick)
    {
        var item = new MenuFlyoutItem { Text = text };
        item.Click += onClick;
        return item;
    }

    // ---- actions ----

    private void Open(HistoryRow row, bool fromStart)
        => OpenRequested?.Invoke(this, new OpenRequest(row.Path, fromStart));

    private void Reveal(HistoryRow row)
    {
        try
        {
            System.Diagnostics.Process.Start(new System.Diagnostics.ProcessStartInfo
            {
                FileName = "explorer.exe",
                Arguments = $"/select,\"{row.Path}\"",
                UseShellExecute = true,
            });
        }
        catch { ToastRequested?.Invoke(this, "Couldn't open Explorer"); }
    }

    private void CopyPath(HistoryRow row)
    {
        var data = new DataPackage();
        data.SetText(row.Path);
        Clipboard.SetContent(data);
        ToastRequested?.Invoke(this, "Path copied");
    }

    private void Remove(HistoryRow row)
    {
        _allRows.RemoveAll(x => ReferenceEquals(x.Row, row));
        _reloading = true;
        _history.Remove(row.Path); // persists + fires Changed so the welcome recents refresh too
        _reloading = false;        // ...but our own Changed shouldn't trigger a full reload (we updated in place)
        _sawItems = true;          // a removal that empties the list reads as "cleared", not "first run"
        Rebuild();
        ToastRequested?.Invoke(this, "Removed from history");
    }

    // Soft light placeholder gradients (same band-04 palette as the welcome shelf) so a row without a
    // poster still reads as a clean card on the light Mica shell rather than a black block.
    private static readonly (string A, string B)[] PosterPalette =
    {
        ("#FFE7EEF4", "#FFCFDCE8"), ("#FFE6EEEB", "#FFCEDED7"), ("#FFEFE9E2", "#FFDBD0C4"),
        ("#FFEAEAF2", "#FFD3D3E4"), ("#FFEDEAE6", "#FFD8D0C6"),
    };

    private static Brush PosterGradient(int index)
    {
        var (a, b) = PosterPalette[index % PosterPalette.Length];
        return new LinearGradientBrush
        {
            StartPoint = new Point(0.1, 0),
            EndPoint = new Point(0.9, 1),
            GradientStops =
            {
                new GradientStop { Color = Hex(a), Offset = 0 },
                new GradientStop { Color = Hex(b), Offset = 1 },
            },
        };
    }

    private static Windows.UI.Color Hex(string s)
        => Windows.UI.Color.FromArgb(0xFF,
            Convert.ToByte(s.Substring(3, 2), 16),
            Convert.ToByte(s.Substring(5, 2), 16),
            Convert.ToByte(s.Substring(7, 2), 16));
}
