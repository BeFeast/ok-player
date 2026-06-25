using System;
using System.Threading.Tasks;
using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Input;
using OkPlayer.Core;
using Windows.Storage.Pickers;

namespace OkPlayer.App;

public sealed partial class MainWindow : Window
{
    private static readonly string[] MediaExtensions =
    {
        ".mkv", ".mp4", ".m4v", ".avi", ".mov", ".webm", ".m2ts", ".ts", ".wmv", ".flv",
        ".mp3", ".flac", ".m4a", ".opus", ".wav", ".ogg", ".mka",
    };

    private bool _fullscreen;

    public MainWindow(string? initialFile = null)
    {
        InitializeComponent();
        Title = "OK Player";
        try { AppWindow.SetIcon(System.IO.Path.Combine(AppContext.BaseDirectory, "Assets", "OkPlayer.ico")); } catch { }

        // Immersive: extend content under the title-bar band so the video reaches the top edge;
        // the auto-hiding top bar is the drag region, and the caption buttons go transparent with
        // white glyphs so Mica/video shows through.
        ExtendsContentIntoTitleBar = true;
        SetTitleBar(Player.TitleBarElement);
        Player.MediaPresenceChanged += (_, hasMedia) => SetCaptionForVideo(hasMedia);
        SetCaptionForVideo(false); // start on the light welcome shell (dark caption glyphs)

        Player.ToggleFullscreenRequested += (_, _) => SetFullscreen(!_fullscreen);
        Player.ExitFullscreenRequested += (_, _) => SetFullscreen(false);
        // re-cover the island top edge if WinUI re-lays it out while full screen (e.g. a DPI/size change)
        AppWindow.Changed += (_, args) => { if (_fullscreen && args.DidSizeChange) DispatcherQueue.TryEnqueue(CoverIslandTopEdge); };
        Player.OpenFileRequested += async (_, _) => await OpenFileAsync();
        Player.FitToVideoRequested += (_, size) => FitToVideo(size.Width, size.Height);
        Player.SettingsRequested += (_, _) => OpenSettings();
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
        Closed += (_, _) =>
        {
            Player.SaveProgress();                 // persist resume position on app close
            App.Settings.Changed -= ApplyAppTheme; // don't keep this closed window rooted
            App.Settings.Changed -= Player.ApplySubtitleDefaults;
            _settingsWindow?.Close();              // don't leave Settings as a headless window
        };
        ApplyAppTheme();
        App.Settings.Changed += ApplyAppTheme;                 // theme chosen in Settings applies to the player too
        App.Settings.Changed += Player.ApplySubtitleDefaults;  // subtitle size/position changes apply live
        if (!string.IsNullOrEmpty(initialFile))
            Player.QueueInitialFile(initialFile); // a file passed on the command line ("Open with")
    }

    private void ApplyAppTheme()
    {
        if (Content is FrameworkElement root)
            root.RequestedTheme = App.Settings.Current.Theme == "Light" ? ElementTheme.Light : ElementTheme.Default;
    }

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

    private void SetCaptionForVideo(bool overVideo)
    {
        var tb = AppWindow.TitleBar;
        tb.ButtonBackgroundColor = Microsoft.UI.Colors.Transparent;
        tb.ButtonInactiveBackgroundColor = Microsoft.UI.Colors.Transparent;
        if (overVideo)
        {
            tb.ButtonForegroundColor = Microsoft.UI.Colors.White;
            tb.ButtonInactiveForegroundColor = Windows.UI.Color.FromArgb(0xB0, 0xFF, 0xFF, 0xFF);
            tb.ButtonHoverBackgroundColor = Windows.UI.Color.FromArgb(0x33, 0xFF, 0xFF, 0xFF);
            tb.ButtonHoverForegroundColor = Microsoft.UI.Colors.White;
            tb.ButtonPressedBackgroundColor = Windows.UI.Color.FromArgb(0x22, 0xFF, 0xFF, 0xFF);
            tb.ButtonPressedForegroundColor = Microsoft.UI.Colors.White;
        }
        else
        {
            // light Mica shell — let the caption glyphs follow the system theme (dark on light)
            tb.ButtonForegroundColor = null;
            tb.ButtonInactiveForegroundColor = null;
            tb.ButtonHoverBackgroundColor = null;
            tb.ButtonHoverForegroundColor = null;
            tb.ButtonPressedBackgroundColor = null;
            tb.ButtonPressedForegroundColor = null;
        }
    }

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

    private void SetFullscreen(bool on)
    {
        if (on == _fullscreen)
            return;
        _fullscreen = on;
        AppWindow.SetPresenter(on ? AppWindowPresenterKind.FullScreen : AppWindowPresenterKind.Overlapped);
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
        if (GetClientRect(hwnd, out var rc))
        {
            int delta = (rc.Bottom - rc.Top) - ch;
            if (delta > 0 && ch - delta > 0)
                AppWindow.ResizeClient(new Windows.Graphics.SizeInt32(cw, ch - delta));
        }
        // Centre the whole window within the work area so a window sized for a large video never extends past
        // the monitor edges.
        var outer = AppWindow.Size;
        int x = work.X + Math.Max(0, (work.Width - outer.Width) / 2);
        int y = work.Y + Math.Max(0, (work.Height - outer.Height) / 2);
        AppWindow.Move(new Windows.Graphics.PointInt32(x, y));
    }

    [System.Runtime.InteropServices.DllImport("user32.dll")]
    private static extern bool GetClientRect(IntPtr hWnd, out NativeRect lpRect);

    private struct NativeRect { public int Left, Top, Right, Bottom; }

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
    private const int VK_SHIFT = 0x10;

    [System.Runtime.InteropServices.DllImport("user32.dll")]
    private static extern short GetAsyncKeyState(int vKey);
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
        foreach (var ext in MediaExtensions)
            picker.FileTypeFilter.Add(ext);

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
}
