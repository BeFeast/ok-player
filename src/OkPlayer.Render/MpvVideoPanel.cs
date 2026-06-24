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
            _mpv.SetOption("vo", "libmpv");        // mandatory: the render API drives output
            _mpv.SetOption("hwdec", "auto-safe");  // hardware decode where safely mappable to GL
            _mpv.SetOption("keep-open", "yes");     // hold the last frame instead of closing on EOF
            _mpv.Initialize();

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

    public void TogglePause()
    {
        if (_mpv == null)
            return;
        bool paused = _mpv.GetPropertyBool("pause") ?? false;
        _mpv.SetProperty("pause", !paused);
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
