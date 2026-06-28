using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using OpenTK.Graphics.OpenGL;
using OkPlayer.Mpv;
using OkPlayer.Mpv.Interop;
using OkPlayer.Render.Interop;
using WinRT;

namespace OkPlayer.Render;

/// <summary>
/// The single video plane. Hosts a SwapChainPanel and drives the libmpv render loop on the UI thread
/// (composition cadence). Owns the MpvContext, so it is also the playback engine the chrome talks to.
/// </summary>
public sealed class MpvVideoPanel : ContentControl, IDisposable
{
    private SwapChainPanel? _panel;
    private GlInteropDevice? _device;
    private VideoSwapChain? _swapChain;
    private MpvContext? _mpv;
    private MpvRenderContext? _render;

    private bool _initialized;
    private bool _renderingHooked;
    private volatile bool _forceRender;
    private TimeSpan _lastRenderTime = TimeSpan.FromSeconds(-1);
    // Reply ids are shared across this ONE MpvContext (the VM attaches to the same engine): keep them distinct
    // from PlayerViewModel.SeekReply (= 2) or a reply is read as the wrong command's completion.
    private const ulong ScreenshotReply = 1;
    private const ulong ClipboardReply = 3;
    private const ulong SubtitleAddReply = 4;

    /// <summary>Raised (on the event thread) when a screenshot has finished saving successfully.</summary>
    public event EventHandler? ScreenshotSaved;

    /// <summary>The underlying libmpv context — the engine the OSC / panels command. Null until initialized.</summary>
    public MpvContext? Engine => _mpv;

    /// <summary>Raised once the engine + render context are ready.</summary>
    public event EventHandler? EngineReady;

    public MpvVideoPanel()
    {
        HorizontalContentAlignment = HorizontalAlignment.Stretch;
        VerticalContentAlignment = VerticalAlignment.Stretch;
        IsTabStop = false;
        Loaded += (_, _) => EnsureInitialized();
        Unloaded += (_, _) => Dispose();
        SizeChanged += OnSizeChanged;
    }

    /// <summary>Create the GL/D3D device, swap-chain host, and libmpv engine. Idempotent.</summary>
    public void EnsureInitialized()
    {
        if (_initialized)
            return;
        _initialized = true; // set early to block re-entrant calls during init

        try
        {
            _device = new GlInteropDevice();

            _panel = new SwapChainPanel();
            _panel.CompositionScaleChanged += (_, _) => UpdateSwapChainSize();
            Content = _panel;

            _mpv = new MpvContext();
            _mpv.CommandReply += OnCommandReply;   // clear the screenshot render-yield as soon as it finishes
            _mpv.SetOption("vo", "libmpv");        // mandatory: the render API drives output
            _mpv.SetOption("hwdec", HardwareDecoding ? "auto-safe" : "no"); // hw decode (Settings -> Video)
            _mpv.SetOption("keep-open", "yes");     // hold the last frame instead of closing on EOF
            // Don't promote an audio file's embedded cover art to a video track. Routing that single still
            // frame through the libmpv render API freezes the UI (the app shows its own now-playing card for
            // audio instead). Without this, a flac/mp3 with album art opens, starts playing, then hangs.
            _mpv.SetOption("audio-display", "no");
            _mpv.SetOption("volume-max", "130");    // allow the PRD volume boost (>100%)
            _mpv.SetOption("osc", "no");            // we draw our own on-screen controls
            _mpv.SetOption("input-default-bindings", "no"); // the app owns the keyboard map
            string pictures = System.Environment.GetFolderPath(System.Environment.SpecialFolder.MyPictures);
            if (!string.IsNullOrEmpty(pictures))
                _mpv.SetOption("screenshot-directory", pictures);
            ApplyUserConfig(_mpv); // power-user escape hatch — applied last so it can override the soft defaults above
            _mpv.Initialize();
            // EnsureInitialized runs on the UI thread (Loaded), which is also where the render loop drives mpv.
            // Arm the debug guard so any blocking mpv call mistakenly issued on this thread fails fast.
            _mpv.MarkRenderThread();

            _render = new MpvRenderContext(_mpv, GlInteropDevice.GetProcAddress);
            _render.SetUpdateCallback(() => _forceRender = true);

            // SizeChanged often fires before Loaded (before the device existed), so create the swap
            // chain now that the control is laid out; CompositionScaleChanged corrects DPI later.
            if (HasRenderableSize)
                TryCreateSwapChain();

            HookRendering();
            EngineReady?.Invoke(this, EventArgs.Empty);
        }
        catch
        {
            // A subcomponent ctor failed (no WGL_NV_DX_interop, missing libmpv-2.dll, …). Roll back so
            // a later retry re-initializes instead of returning early into a null engine.
            _initialized = false;
            TeardownEngine();
            throw;
        }
    }

    /// <summary>Use hardware video decoding (auto-safe) vs software. Read at engine init; the host sets it
    /// from user settings before the panel loads. Applied per engine, so it takes effect on restart.</summary>
    public static bool HardwareDecoding { get; set; } = true;

    /// <summary>The power-user escape-hatch config: mpv.conf-style <c>key=value</c> lines applied to the
    /// engine at startup. Lives next to the other OkPlayer state so it's easy to find and hand-edit.</summary>
    public static string UserConfigPath => System.IO.Path.Combine(
        System.Environment.GetFolderPath(System.Environment.SpecialFolder.ApplicationData), "OkPlayer", "mpv.conf");

    // Options the user must not override — they'd break video output, the on-screen controls, the app's
    // keyboard ownership, or open a remote-control / logging surface. The loader group
    // (include / script / scripts / load-scripts) is blocked too: it can pull in another config or run
    // arbitrary Lua/JS, so a copy-pasted mpv.conf can't turn the escape hatch into code execution.
    private static readonly System.Collections.Generic.HashSet<string> ProtectedOptions =
        new(System.StringComparer.OrdinalIgnoreCase)
        {
            "vo", "osc", "input-default-bindings", "config", "config-dir", "input-conf",
            "input-ipc-server", "terminal", "msg-level", "wid", "log-file",
            "include", "script", "scripts", "load-scripts", "scripts-dir",
        };

    /// <summary>True when <paramref name="key"/> is an engine-critical option the escape hatch must not
    /// override (video output, the OSC, the keyboard map, the config-loader / scripting surface). Trimmed and
    /// case-insensitive, matching how <see cref="ApplyUserConfig"/> tests each line. Exposed so the
    /// Settings → Advanced editor can flag a row the loader will silently skip (App references Render).</summary>
    public static bool IsProtectedOption(string key) =>
        !string.IsNullOrWhiteSpace(key) && ProtectedOptions.Contains(key.Trim());

    private static void ApplyUserConfig(MpvContext mpv)
    {
        try
        {
            if (!System.IO.File.Exists(UserConfigPath))
                return;
            foreach (string rawLine in System.IO.File.ReadAllLines(UserConfigPath))
            {
                string line = rawLine.Trim();
                if (line.Length == 0 || line[0] == '#')
                    continue;
                int eq = line.IndexOf('=');
                string key = (eq >= 0 ? line[..eq] : line).Trim();
                string val = eq >= 0 ? line[(eq + 1)..].Trim() : "yes";
                if (key.Length == 0 || ProtectedOptions.Contains(key))
                    continue;
                try { mpv.SetOption(key, val); }
                catch { /* skip an unknown/invalid option rather than fail to start */ }
            }
        }
        catch { /* the escape hatch is best-effort; never block startup on it */ }
    }

    private double ScaleX => _panel is { CompositionScaleX: > 0 } ? _panel.CompositionScaleX : 1.0;
    private double ScaleY => _panel is { CompositionScaleY: > 0 } ? _panel.CompositionScaleY : 1.0;

    // True only when both physical-pixel dimensions are at least 1px, so a fractional logical size
    // (0 &lt; w &lt; 1) during layout/animation can't truncate to a zero-sized swap chain.
    private bool HasRenderableSize => (int)(ActualWidth * ScaleX) >= 1 && (int)(ActualHeight * ScaleY) >= 1;

    private void HookRendering()
    {
        if (_renderingHooked)
            return;
        CompositionTarget.Rendering += OnRendering;
        _renderingHooked = true;
    }

    private void OnSizeChanged(object sender, SizeChangedEventArgs e)
    {
        if (_device == null || !HasRenderableSize)
            return;
        if (_swapChain == null)
            TryCreateSwapChain();
        else
            UpdateSwapChainSize();
    }

    private void TryCreateSwapChain()
    {
        if (_swapChain != null || _device == null || _panel == null || !HasRenderableSize)
            return;
        _swapChain = new VideoSwapChain(_device, (int)ActualWidth, (int)ActualHeight, ScaleX, ScaleY);
        _panel.As<ISwapChainPanelNative>().SetSwapChain(_swapChain.SwapChainHandle);
        _forceRender = true;
    }

    private void UpdateSwapChainSize()
    {
        if (_swapChain == null || _panel == null)
            return;
        _swapChain.UpdateSize((int)ActualWidth, (int)ActualHeight, ScaleX, ScaleY);
        _forceRender = true;
    }

    private void OnRendering(object? sender, object e)
    {
        var args = (RenderingEventArgs)e;
        if (_lastRenderTime == args.RenderingTime) // dedupe duplicate composition ticks
            return;
        _lastRenderTime = args.RenderingTime;
        Draw();
    }

    private void Draw()
    {
        if (_swapChain == null || _render == null)
            return;

        bool hasFrame = (_render.Update() & MpvRenderUpdateFlag.Frame) != 0;
        if (!hasFrame && !_forceRender)
            return;
        _forceRender = false;

        if (!_swapChain.Begin())
            return; // interop acquire failed (device removed / resize race) — skip this frame
        // Clear to OPAQUE black: the SwapChainPanel composites over the window backdrop (Mica), so any pixel
        // mpv doesn't cover (e.g. a 1px row at the window's top edge in fullscreen) must be opaque black, not
        // transparent — otherwise the light Mica shows through as a white hairline.
        GL.ClearColor(0f, 0f, 0f, 1f);
        GL.Clear(ClearBufferMask.ColorBufferBit | ClearBufferMask.DepthBufferBit);
        _render.Render(_swapChain.GLFrameBufferHandle, _swapChain.BufferWidth, _swapChain.BufferHeight);
        _swapChain.End();
        _render.ReportSwap();
    }

    // ---------- playback API (thin pass-through to the engine) ----------

    public void Open(string pathOrUrl)
    {
        EnsureInitialized();
        _mpv!.Loadfile(pathOrUrl);
    }

    public void Play() => _mpv?.SetProperty("pause", false);
    public void Pause() => _mpv?.SetProperty("pause", true);

    /// <summary>Take a screenshot to the screenshot directory. <paramref name="includeSubtitles"/> uses mpv's
    /// "subtitles" mode (decoded frame + rendered subtitles) instead of the bare "video" frame. Fire-and-forget:
    /// the async command runs while the render loop keeps driving the pipeline (a paused/yielded render with
    /// vo=libmpv would starve the grab and it would never land), and ScreenshotSaved fires on success.
    /// Returns false only if no engine is loaded.</summary>
    public bool Screenshot(bool includeSubtitles = false)
    {
        if (_mpv is not { } mpv)
            return false;
        try
        {
            mpv.CommandAsync(ScreenshotReply, "screenshot", includeSubtitles ? "subtitles" : "video");
            return true;
        }
        catch (MpvException)
        {
            return false;
        }
    }

    /// <summary>Grab the current frame to <paramref name="path"/> (so the caller can copy it to the clipboard).
    /// Raises <see cref="ScreenshotForClipboard"/> when the reply confirms the file is written; the caller owns
    /// the path it passed, so no path is tracked here (a fresh grab can't strand a prior one's state).</summary>
    public bool ScreenshotToClipboard(string path, bool includeSubtitles = false)
    {
        if (_mpv is not { } mpv)
            return false;
        try
        {
            mpv.CommandAsync(ClipboardReply, "screenshot-to-file", path, includeSubtitles ? "subtitles" : "video");
            return true;
        }
        catch (MpvException)
        {
            return false;
        }
    }

    /// <summary>Raised (on the event thread) for EVERY submitted clipboard grab — the bool is whether it wrote
    /// the file. Fires even on failure so the caller can keep its per-grab bookkeeping (one submit ↔ one reply)
    /// in sync; a failed reply would otherwise strand the caller's queued path.</summary>
    public event EventHandler<bool>? ScreenshotForClipboard;

    /// <summary>Load an external subtitle file and select it. Returns true if the command was submitted;
    /// <see cref="SubtitleAdded"/> then fires with whether mpv actually loaded it (a corrupt/unsupported
    /// file fails), so the caller can report success vs failure accurately rather than assuming success.</summary>
    public bool AddSubtitle(string path)
    {
        if (_mpv is not { } mpv)
            return false;
        try
        {
            mpv.CommandAsync(SubtitleAddReply, "sub-add", path, "select");
            return true;
        }
        catch (MpvException)
        {
            return false;
        }
    }

    /// <summary>Raised (on the event thread) for EVERY submitted sub-add — the bool is whether mpv loaded
    /// the file. Fires even on failure so the caller's one-submit-one-reply bookkeeping stays in sync.</summary>
    public event EventHandler<bool>? SubtitleAdded;

    private void OnCommandReply(ulong id, bool success)
    {
        if (id == ScreenshotReply && success)
            ScreenshotSaved?.Invoke(this, EventArgs.Empty);
        else if (id == ClipboardReply)
            ScreenshotForClipboard?.Invoke(this, success);
        else if (id == SubtitleAddReply)
            SubtitleAdded?.Invoke(this, success);
    }

    public void Dispose()
    {
        TeardownEngine();
        _initialized = false; // allow a later reload / reparent to re-initialize cleanly
    }

    /// <summary>Tear down the engine + GL/D3D resources in dependency order. Every field is
    /// null-checked, so this is safe to call partially and doubles as failed-init rollback.</summary>
    private void TeardownEngine()
    {
        if (_renderingHooked)
        {
            CompositionTarget.Rendering -= OnRendering; // stop the render loop before freeing resources
            _renderingHooked = false;
        }
        _render?.Dispose();        // free the mpv render context (GL context still current)
        _render = null;
        _swapChain?.Dispose();     // release the swap chain COM object + GL framebuffer
        _swapChain = null;
        _mpv?.Dispose();           // terminate libmpv
        _mpv = null;
        _device?.Dispose();        // release the D3D device + close the WGL_NV_DX_interop device
        _device = null;
        _lastRenderTime = TimeSpan.FromSeconds(-1);
        // The static shared GL context/window is intentionally retained (single-window app).
    }
}
