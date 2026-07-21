using System;
using System.Linq;
using System.Threading.Tasks;
using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Input;
using OkPlayer.Core;
using Windows.Storage.Pickers;

namespace OkPlayer.App;

public sealed partial class MainWindow : Window
{
    private bool _fullscreen;
    private Services.SystemMediaControlsService? _systemMediaControls;

    public MainWindow(string? initialFile = null, double? resumeSeconds = null, int? subTrack = null, int? audioTrack = null)
    {
        InitializeComponent();
        Title = "OK Player";
        try { AppWindow.SetIcon(System.IO.Path.Combine(AppContext.BaseDirectory, "Assets", "OkPlayer.ico")); } catch { }

        // Immersive: extend content under the title-bar band so the video reaches the top edge; the auto-hiding
        // top bar is the drag region. We hide the NATIVE min/max/close and draw our own in that auto-hiding chrome
        // (PlayerView.CaptionBar) so they vanish over video like the rest of the controls — the native buttons
        // can't. The host owns the AppWindow/presenter actions they invoke.
        ExtendsContentIntoTitleBar = true;
        SetTitleBar(Player.TitleBarElement);
        HideNativeCaption();
        UpdateMaxRestoreGlyph(); // seed the maximize/restore glyph for the window's starting state
        Player.CaptionMinimizeRequested += (_, _) => { if (AppWindow.Presenter is OverlappedPresenter p) p.Minimize(); };
        Player.CaptionMaximizeRestoreRequested += (_, _) => ToggleMaximize();
        Player.CaptionCloseRequested += (_, _) => Close();

        Player.ToggleFullscreenRequested += (_, _) => SetFullscreen(!_fullscreen);
        Player.ExitFullscreenRequested += (_, _) => SetFullscreen(false);
        Player.MiniPlayerRequested += (_, _) => SetMiniPlayer(!_miniPlayer);
        // re-cover the island top edge if WinUI re-lays it out while full screen (e.g. a DPI/size change), and
        // keep the custom maximize/restore glyph in sync with the window state (maximize, restore, snap, drag-to-top).
        AppWindow.Changed += (_, args) =>
        {
            if (_fullscreen && args.DidSizeChange) DispatcherQueue.TryEnqueue(CoverIslandTopEdge);
            if (args.DidPresenterChange || args.DidSizeChange) UpdateMaxRestoreGlyph();
        };
        Player.OpenFileRequested += async (_, _) => await OpenFileAsync();
        Player.QueueFilesRequested += async mode => await QueueFilesAsync(mode);
        Player.AddSubtitleRequested += async (_, _) => await AddSubtitleAsync();
        Player.FitToVideoRequested += (_, size) => FitToVideo(size.Width, size.Height);
        Player.AlwaysOnTopRequested += (_, on) => SetAlwaysOnTop(on);
        Player.SettingsRequested += (_, _) => OpenSettings();
        Player.SavePlaylistRequested += (_, content) => SavePlaylist(content);
        // Drag the window by the video / welcome backdrop, like the title bar. handledEventsToo=true so the
        // ScrollViewer's own move handling can't swallow the drag.
        foreach (var surface in Player.WindowDragSurfaces)
        {
            surface.AddHandler(UIElement.PointerPressedEvent, new PointerEventHandler(OnBackdropPointerPressed), true);
            surface.AddHandler(UIElement.PointerMovedEvent, new PointerEventHandler(OnBackdropPointerMoved), true);
            surface.AddHandler(UIElement.PointerReleasedEvent, new PointerEventHandler(OnBackdropPointerReleased), true);
            surface.AddHandler(UIElement.PointerCaptureLostEvent, new PointerEventHandler(OnBackdropPointerCaptureLost), true);
        }
        HookAspectResize(); // hold Shift while dragging an edge to keep the video's aspect
        Activated += (_, _) =>
        {
            // Desktop WinUI has no CoreWindow, so bind SMTC to this HWND through the Windows interop helper.
            _systemMediaControls ??= Services.SystemMediaControlsService.TryCreate(
                WinRT.Interop.WindowNative.GetWindowHandle(this), Player);
        };
        Closed += (_, _) =>
        {
            Player.SaveProgress();                 // persist resume position on app close
            _systemMediaControls?.Dispose();
            _systemMediaControls = null;
            App.Settings.Changed -= ApplyAppTheme; // don't keep this closed window rooted
            App.Settings.Changed -= Player.ApplySubtitleDefaults;
            App.Settings.Changed -= Player.ApplyAudioDefaults;
            App.Settings.Changed -= Player.ApplyVideoDefaults;
            _settingsWindow?.Close();              // don't leave Settings as a headless window
        };
        ApplyAppTheme();
        App.Settings.Changed += ApplyAppTheme;                 // theme chosen in Settings applies to the player too
        App.Settings.Changed += Player.ApplySubtitleDefaults;  // subtitle size/position changes apply live
        App.Settings.Changed += Player.ApplyAudioDefaults;     // loudness normalization toggles apply live
        App.Settings.Changed += Player.ApplyVideoDefaults;     // picture adjustment sliders apply live
        if (!string.IsNullOrEmpty(initialFile))
            Player.QueueInitialFile(initialFile, resumeSeconds, subTrack, audioTrack); // command-line / library launch
    }

    private void ApplyAppTheme()
    {
        if (Content is FrameworkElement root)
            root.RequestedTheme = SettingsWindow.ThemeFor(App.Settings.Current.Theme);
    }

    /// <summary>Open a file a second launch forwarded into this single instance (see <see cref="App"/>), then
    /// bring the window forward. Engine is already up in a running instance, so open immediately. UI thread.</summary>
    public void OpenFileFromRedirect(string path)
    {
        Player.OpenMedia(path);
        BringToForeground();
    }

    /// <summary>Surface this window: restore it if minimized, activate it, and force it to the foreground — a
    /// plain <see cref="Window.Activate"/> can't steal focus from the app the user is currently in.</summary>
    public void BringToForeground()
    {
        try
        {
            if (AppWindow.Presenter is OverlappedPresenter { State: OverlappedPresenterState.Minimized } p)
                p.Restore();
        }
        catch { }
        Activate();
        try { SetForegroundWindow(WinRT.Interop.WindowNative.GetWindowHandle(this)); } catch { }
    }

    [System.Runtime.InteropServices.DllImport("user32.dll")]
    private static extern bool SetForegroundWindow(IntPtr hWnd);

    private SettingsWindow? _settingsWindow;

    private void OpenSettings()
    {
        if (_settingsWindow is null)
        {
            _settingsWindow = new SettingsWindow();
            _settingsWindow.Closed += (_, _) => _settingsWindow = null; // single instance; clear on close
        }
        _settingsWindow.Activate();
    }

    /// <summary>Hide the native min/max/close so the custom caption buttons (in the auto-hiding chrome) are the
    /// only ones — keeping the resize border. Must be re-applied after a presenter swap back to Overlapped
    /// (leaving fullscreen / mini-player), which restores the native title bar.</summary>
    private void HideNativeCaption()
    {
        if (AppWindow.Presenter is OverlappedPresenter p)
            p.SetBorderAndTitleBar(hasBorder: true, hasTitleBar: false);
    }

    /// <summary>Maximize ⇄ restore — the custom maximize button's action.</summary>
    private void ToggleMaximize()
    {
        if (AppWindow.Presenter is OverlappedPresenter p)
        {
            if (p.State == OverlappedPresenterState.Maximized) p.Restore();
            else p.Maximize();
        }
    }

    /// <summary>Point the custom maximize button at the current window state (maximized → show restore glyph).</summary>
    private void UpdateMaxRestoreGlyph()
        => Player.SetMaximizedGlyph(AppWindow.Presenter is OverlappedPresenter { State: OverlappedPresenterState.Maximized });

    [System.Runtime.InteropServices.DllImport("user32.dll", CharSet = System.Runtime.InteropServices.CharSet.Unicode)]
    private static extern IntPtr FindWindowExW(IntPtr parent, IntPtr after, string? cls, string? title);
    [System.Runtime.InteropServices.DllImport("user32.dll")]
    private static extern bool SetWindowPos(IntPtr hWnd, IntPtr after, int x, int y, int cx, int cy, uint flags);
    private const uint SWP_NOZORDER = 0x4, SWP_NOACTIVATE = 0x10;
    private IntPtr _islandBridge;

    /// <summary>WinUI lays the XAML island host (DesktopChildSiteBridge) out 1px below the client origin in
    /// full screen, so the video starts at y=1 and the backdrop shows through at y=0 as a white hairline.
    /// Force the island to cover the whole client (y=0..bottom) so the video owns the top scanline.</summary>
    private void CoverIslandTopEdge()
    {
        if (!_fullscreen)
            return;
        IntPtr top = WinRT.Interop.WindowNative.GetWindowHandle(this);
        if (_islandBridge == IntPtr.Zero)
            _islandBridge = FindWindowExW(top, IntPtr.Zero, "Microsoft.UI.Content.DesktopChildSiteBridge", null);
        if (_islandBridge != IntPtr.Zero && GetClientRect(top, out var rc))
            SetWindowPos(_islandBridge, IntPtr.Zero, 0, 0, rc.Right - rc.Left, rc.Bottom - rc.Top, SWP_NOZORDER | SWP_NOACTIVATE);
    }

    private bool _alwaysOnTop;

    /// <summary>Pin/unpin the window above others via the overlapped presenter (the idiomatic WinUI 3 path,
    /// vs. raw HWND_TOPMOST). Re-applied after fullscreen, which swaps the presenter and would drop it.</summary>
    private void SetAlwaysOnTop(bool on)
    {
        _alwaysOnTop = on;
        if (AppWindow.Presenter is OverlappedPresenter p)
            p.IsAlwaysOnTop = on;
    }

    private bool _miniPlayer;

    /// <summary>Toggle the compact-overlay mini-player — native Windows picture-in-picture: a small,
    /// always-on-top window the OS keeps above others. Mutually exclusive with fullscreen. Leaving it
    /// restores the overlapped presenter and re-applies the user's always-on-top choice (a fresh presenter
    /// starts un-pinned).</summary>
    private void SetMiniPlayer(bool on)
    {
        if (on == _miniPlayer)
            return;
        if (on && _fullscreen)
        {
            SetFullscreen(false); // can't be both; drop fullscreen first
            if (_fullscreen) // the off-switch hit a presenter failure — don't enter a both-modes-on state
            {
                Services.Log.Warn("SetMiniPlayer aborted: could not leave fullscreen first");
                return;
            }
        }
        Services.Log.Step($"SetPresenter({(on ? "CompactOverlay" : "Overlapped")}) [mini-player]");
        try
        {
            AppWindow.SetPresenter(on ? AppWindowPresenterKind.CompactOverlay : AppWindowPresenterKind.Overlapped);
        }
        catch (Exception ex)
        {
            // The presenter didn't change — leave _miniPlayer untouched so the flag still matches reality.
            Services.Log.Exception("SetMiniPlayer.SetPresenter", ex);
            return;
        }
        _miniPlayer = on; // commit the flag only once the presenter actually switched
        Services.Log.Step("SetPresenter done [mini-player]");
        if (!on) // back to the overlapped presenter — it restores the native title bar, so re-hide it
        {
            HideNativeCaption();
            UpdateMaxRestoreGlyph();
            if (_alwaysOnTop && AppWindow.Presenter is OverlappedPresenter p)
                p.IsAlwaysOnTop = true;
        }
    }

    private void SetFullscreen(bool on)
    {
        if (on == _fullscreen)
            return;
        if (on && _miniPlayer)
        {
            SetMiniPlayer(false); // mutually exclusive: leave compact overlay before going fullscreen
            if (_miniPlayer) // the off-switch hit a presenter failure — don't enter a both-modes-on state
            {
                Services.Log.Warn("SetFullscreen aborted: could not leave mini-player first");
                return;
            }
        }
        // SetPresenter(FullScreen) is the prime suspect for "the whole desktop disappeared": a borderless
        // full-screen window that then freezes covers everything. Breadcrumb it (so a hang here is named) and
        // guard it (so a driver/compositor throw can't tear down the app).
        Services.Log.Step($"SetPresenter({(on ? "FullScreen" : "Overlapped")}) [fullscreen]");
        try
        {
            AppWindow.SetPresenter(on ? AppWindowPresenterKind.FullScreen : AppWindowPresenterKind.Overlapped);
        }
        catch (Exception ex)
        {
            // The presenter didn't change — leave _fullscreen untouched so the flag still matches reality.
            Services.Log.Exception("SetFullscreen.SetPresenter", ex);
            return;
        }
        _fullscreen = on; // commit the flag only once the presenter actually switched
        Services.Log.Step("SetPresenter done [fullscreen]");
        if (!on) // back to the overlapped presenter — it restores the native title bar, so re-hide it
        {
            HideNativeCaption();
            UpdateMaxRestoreGlyph();
            if (_alwaysOnTop && AppWindow.Presenter is OverlappedPresenter p)
                p.IsAlwaysOnTop = true; // the new overlapped presenter starts un-pinned — restore the user's choice
        }
        if (on)
        {
            // re-apply across the layout passes that follow the presenter change (each would otherwise
            // reset the island back to y=1)
            DispatcherQueue.TryEnqueue(CoverIslandTopEdge);
            DispatcherQueue.TryEnqueue(Microsoft.UI.Dispatching.DispatcherQueuePriority.Low, CoverIslandTopEdge);
        }
    }

    /// <summary>Size the window to the video's aspect and place it fully inside the monitor: shrink to fit
    /// ~94% of the work area (never upscaling past native), then centre it so the screen edges never clip it.</summary>
    private void FitToVideo(int w, int h)
    {
        if (w <= 0 || h <= 0 || _fullscreen)
            return;
        var work = DisplayArea.GetFromWindowId(AppWindow.Id, DisplayAreaFallback.Nearest).WorkArea;
        // One scale for both axes keeps the client at the video's exact aspect (video fills it, no black
        // margins); clamp to <=1 so a small video is never blown up past its native size.
        double scale = Math.Min(1.0, Math.Min(work.Width * 0.94 / w, work.Height * 0.94 / h));
        // A single scale on both axes (Max(1,…) only guards against a zero size) keeps the exact video
        // aspect — independent per-axis minimums would distort it and reintroduce black margins.
        int cw = Math.Max(1, (int)Math.Round(w * scale));
        int ch = Math.Max(1, (int)Math.Round(h * scale));
        IntPtr hwnd = WinRT.Interop.WindowNative.GetWindowHandle(this);
        AppWindow.ResizeClient(new Windows.Graphics.SizeInt32(cw, ch));
        // ResizeClient's height arg excludes the extended title-bar band, but the real content client (what
        // mpv fills) includes it — so the client comes out taller than asked and mpv letterboxes the video.
        // Measure that delta and re-request the height minus it, so the client lands on the exact video aspect.
        int bandDelta = 0;
        if (GetClientRect(hwnd, out var rc))
        {
            int delta = (rc.Bottom - rc.Top) - ch;
            bandDelta = delta;
            if (delta > 0 && ch - delta > 0)
                AppWindow.ResizeClient(new Windows.Graphics.SizeInt32(cw, ch - delta));
        }
        // The OS may have clamped the window UP to the minimum size (WM_GETMINMAXINFO) — common on a small
        // display, where a video that fits narrower than the minimum width gets a window wider than the video
        // aspect, which mpv pillarboxes with black side bars (#110). Measure the real client and, if it no
        // longer matches the video aspect, grow the other axis so the video fills the minimum-size window.
        if (GetClientRect(hwnd, out var rcClamped))
        {
            int cwActual = rcClamped.Right - rcClamped.Left, chActual = rcClamped.Bottom - rcClamped.Top;
            // Cap the grown axis at the same on-screen budget the initial fit used, so a narrow/portrait clip
            // whose width is pinned at the minimum can't grow a window taller than the monitor.
            int maxCw = (int)(work.Width * 0.94), maxCh = (int)(work.Height * 0.94);
            if (OkPlayer.Core.WindowFit.FillClient(w, h, cwActual, chActual, maxCw, maxCh) is { } fill)
                AppWindow.ResizeClient(new Windows.Graphics.SizeInt32(fill.Width, Math.Max(1, fill.Height - bandDelta)));
        }
        // Centre the whole window within the work area so a window sized for a large video never extends past
        // the monitor edges.
        var outer = AppWindow.Size;
        int x = work.X + Math.Max(0, (work.Width - outer.Width) / 2);
        int y = work.Y + Math.Max(0, (work.Height - outer.Height) / 2);
        AppWindow.Move(new Windows.Graphics.PointInt32(x, y));
        GetClientRect(hwnd, out var rcDone);
        Services.Log.Step($"FitToVideo: video={w}x{h} work={work.Width}x{work.Height} " +
            $"client={rcDone.Right - rcDone.Left}x{rcDone.Bottom - rcDone.Top}");
    }

    [System.Runtime.InteropServices.DllImport("user32.dll")]
    private static extern bool GetClientRect(IntPtr hWnd, out NativeRect lpRect);

    private struct NativeRect { public int Left, Top, Right, Bottom; }

    // Win32 MINMAXINFO (lParam of WM_GETMINMAXINFO). Only ptMinTrackSize is written; the layout must match
    // exactly so the marshalled offsets line up. (NativePoint is defined alongside the bg-drag P/Invokes.)
    [System.Runtime.InteropServices.StructLayout(System.Runtime.InteropServices.LayoutKind.Sequential)]
    private struct MinMaxInfo
    {
        public NativePoint ptReserved;
        public NativePoint ptMaxSize;
        public NativePoint ptMaxPosition;
        public NativePoint ptMinTrackSize;
        public NativePoint ptMaxTrackSize;
    }

    // ---- drag the window by the video / welcome backdrop (any non-control area), like the title bar ----

    [System.Runtime.InteropServices.DllImport("user32.dll")]
    private static extern bool GetCursorPos(out NativePoint p);
    private struct NativePoint { public int X, Y; }

    private bool _bgDragArmed, _bgDragging;
    private NativePoint _bgCursor0;          // absolute cursor at press
    private Windows.Graphics.PointInt32 _bgWin0; // window origin at press

    private void OnBackdropPointerPressed(object sender, PointerRoutedEventArgs e)
    {
        // Full screen has no movable window; touch/pen should tap through, not drag.
        if (_fullscreen || e.Pointer.PointerDeviceType != Microsoft.UI.Input.PointerDeviceType.Mouse)
            return;
        if (!e.GetCurrentPoint((UIElement)sender).Properties.IsLeftButtonPressed)
            return;
        GetCursorPos(out _bgCursor0);
        _bgWin0 = AppWindow.Position;
        _bgDragArmed = true;
        _bgDragging = false;
    }

    private void OnBackdropPointerMoved(object sender, PointerRoutedEventArgs e)
    {
        if (!_bgDragArmed)
            return;
        if (!e.GetCurrentPoint((UIElement)sender).Properties.IsLeftButtonPressed)
        {
            _bgDragArmed = false; // released somewhere we didn't see
            return;
        }
        GetCursorPos(out var cur);
        int dx = cur.X - _bgCursor0.X, dy = cur.Y - _bgCursor0.Y;
        if (!_bgDragging && Math.Abs(dx) + Math.Abs(dy) > 4)
        {
            // Promote to a drag only past a small threshold, so a plain click still reaches play/pause.
            _bgDragging = true;
            ((UIElement)sender).CapturePointer(e.Pointer);
        }
        if (_bgDragging)
            AppWindow.Move(new Windows.Graphics.PointInt32(_bgWin0.X + dx, _bgWin0.Y + dy));
    }

    private void OnBackdropPointerReleased(object sender, PointerRoutedEventArgs e)
    {
        if (_bgDragging)
        {
            ((UIElement)sender).ReleasePointerCapture(e.Pointer);
            e.Handled = true; // a drag isn't a click — suppress the play/pause tap that would otherwise follow
        }
        _bgDragArmed = false;
        _bgDragging = false;
    }

    private void OnBackdropPointerCaptureLost(object sender, PointerRoutedEventArgs e)
    {
        _bgDragArmed = false;
        _bgDragging = false;
    }

    // ---- aspect-locked resize: hold Shift while dragging a window edge to keep the video's aspect ----

    private const uint WM_SIZING = 0x0214;
    private const uint WM_GETMINMAXINFO = 0x0024;
    private const int VK_SHIFT = 0x10;

    // Smallest window we let the user drag to. Logical (DPI-independent) pixels — scaled to physical in the
    // WM_GETMINMAXINFO handler. Below this the welcome shelf and chrome start clipping their content.
    private const int MinWindowLogicalWidth = 720;
    private const int MinWindowLogicalHeight = 480;

    [System.Runtime.InteropServices.DllImport("user32.dll")]
    private static extern short GetAsyncKeyState(int vKey);
    [System.Runtime.InteropServices.DllImport("user32.dll")]
    private static extern uint GetDpiForWindow(IntPtr hWnd);
    [System.Runtime.InteropServices.DllImport("user32.dll")]
    private static extern bool GetWindowRect(IntPtr hWnd, out NativeRect lpRect);
    [System.Runtime.InteropServices.DllImport("comctl32.dll", SetLastError = true)]
    private static extern bool SetWindowSubclass(IntPtr hWnd, SubclassProc proc, IntPtr id, IntPtr data);
    [System.Runtime.InteropServices.DllImport("comctl32.dll")]
    private static extern IntPtr DefSubclassProc(IntPtr hWnd, uint msg, IntPtr wParam, IntPtr lParam);
    private delegate IntPtr SubclassProc(IntPtr hWnd, uint msg, IntPtr wParam, IntPtr lParam, IntPtr id, IntPtr data);

    private SubclassProc? _aspectSubclass; // hold a reference so the GC can't collect the native callback

    private void HookAspectResize()
    {
        _aspectSubclass = AspectResizeWndProc;
        SetWindowSubclass(WinRT.Interop.WindowNative.GetWindowHandle(this), _aspectSubclass, (IntPtr)1, IntPtr.Zero);
    }

    /// <summary>WM_SIZING hook: while Shift is held and a video is loaded, snap the proposed window rect so
    /// the client keeps the video's aspect (no letterboxing). Everything else chains to the default proc.</summary>
    private IntPtr AspectResizeWndProc(IntPtr hWnd, uint msg, IntPtr wParam, IntPtr lParam, IntPtr id, IntPtr data)
    {
        // Clamp the resize floor only in the normal windowed state. The compact-overlay mini-player and
        // fullscreen drive their own sizing (the mini-player is deliberately tiny), so forcing the welcome
        // floor on them would inflate the mini-player window — chain those to the default proc untouched.
        if (msg == WM_GETMINMAXINFO && !_miniPlayer && !_fullscreen)
        {
            // Clamp the resize floor so the window can't be dragged small enough to crop the welcome content.
            // ptMinTrackSize is the whole-window track size in physical pixels, so scale the logical floor by DPI.
            uint dpi = GetDpiForWindow(hWnd);
            double scale = dpi > 0 ? dpi / 96.0 : 1.0;
            var mmi = System.Runtime.InteropServices.Marshal.PtrToStructure<MinMaxInfo>(lParam);
            mmi.ptMinTrackSize.X = (int)Math.Round(MinWindowLogicalWidth * scale);
            mmi.ptMinTrackSize.Y = (int)Math.Round(MinWindowLogicalHeight * scale);
            System.Runtime.InteropServices.Marshal.StructureToPtr(mmi, lParam, false);
            return IntPtr.Zero; // handled
        }
        if (msg == WM_SIZING && !_fullscreen && (GetAsyncKeyState(VK_SHIFT) & 0x8000) != 0)
        {
            double aspect = Player.VideoAspect;
            if (aspect > 0 && GetClientRect(hWnd, out var cr) && GetWindowRect(hWnd, out var wr))
            {
                int frameW = (wr.Right - wr.Left) - (cr.Right - cr.Left);
                int frameH = (wr.Bottom - wr.Top) - (cr.Bottom - cr.Top);
                var rect = System.Runtime.InteropServices.Marshal.PtrToStructure<NativeRect>(lParam);
                var (l, t, r, b) = AspectResize.Constrain(rect.Left, rect.Top, rect.Right, rect.Bottom,
                    (int)wParam, aspect, frameW, frameH);
                rect.Left = l; rect.Top = t; rect.Right = r; rect.Bottom = b;
                System.Runtime.InteropServices.Marshal.StructureToPtr(rect, lParam, false);
                return (IntPtr)1; // TRUE: we adjusted the proposed rect
            }
        }
        return DefSubclassProc(hWnd, msg, wParam, lParam);
    }

    private async Task OpenFileAsync()
    {
        var picker = new FileOpenPicker { SuggestedStartLocation = PickerLocationId.VideosLibrary };
        // Unpackaged: associate the picker with this window's HWND.
        WinRT.Interop.InitializeWithWindow.Initialize(picker, WinRT.Interop.WindowNative.GetWindowHandle(this));
        foreach (var ext in MediaFormats.Extensions)
            picker.FileTypeFilter.Add(ext);
        picker.FileTypeFilter.Add(".m3u");  // open a saved playlist
        picker.FileTypeFilter.Add(".m3u8");

        try
        {
            var file = await picker.PickSingleFileAsync();
            if (file is not null)
                Player.OpenMedia(file.Path); // OpenMedia is itself non-throwing
        }
        catch (Exception)
        {
            // Picker failure is non-fatal; swallow so the async-void caller can't crash the app.
        }
    }

    private async Task AddSubtitleAsync()
    {
        var picker = new FileOpenPicker { SuggestedStartLocation = PickerLocationId.VideosLibrary };
        WinRT.Interop.InitializeWithWindow.Initialize(picker, WinRT.Interop.WindowNative.GetWindowHandle(this));
        foreach (var ext in MediaFormats.SubtitleExtensions)
            picker.FileTypeFilter.Add(ext);
        try
        {
            var file = await picker.PickSingleFileAsync();
            if (file is not null)
                Player.AddSubtitle(file.Path);
        }
        catch (Exception)
        {
            // Picker failure is non-fatal; swallow so the async-void caller can't crash the app.
        }
    }

    private async Task QueueFilesAsync(QueueInsertMode mode)
    {
        var picker = new FileOpenPicker { SuggestedStartLocation = PickerLocationId.VideosLibrary };
        WinRT.Interop.InitializeWithWindow.Initialize(picker, WinRT.Interop.WindowNative.GetWindowHandle(this));
        foreach (var ext in MediaFormats.Extensions)
            picker.FileTypeFilter.Add(ext);

        try
        {
            var files = await picker.PickMultipleFilesAsync();
            if (files.Count > 0)
                Player.QueueMedia(files.Select(file => file.Path).ToArray(), mode);
        }
        catch (Exception)
        {
            // Picker failure is non-fatal; the active queue remains untouched.
        }
    }

    private void SavePlaylist(string m3uContent)
    {
        // The shell Save dialog (Win32SaveDialog) is synchronous and runs its own modal message loop, so the
        // app stays responsive while it's up; we replaced the WinRT FileSavePicker, which threw E_FAIL / hung
        // in this unpackaged app.
        try
        {
            IntPtr hwnd = WinRT.Interop.WindowNative.GetWindowHandle(this);
            string? path = Win32SaveDialog.PickSavePath(hwnd, "playlist", "Playlist", "m3u");
            if (path is not null)
                System.IO.File.WriteAllText(path, m3uContent);
        }
        catch (Exception)
        {
            // a save failure is non-fatal
        }
    }
}
