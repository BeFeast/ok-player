using OpenTK.Graphics.OpenGL;
using OpenTK.Graphics.Wgl;
using OpenTK.Platform.Windows;
using Silk.NET.Core.Native;
using Silk.NET.Direct3D11;
using Silk.NET.DXGI;

namespace OkPlayer.Render;

/// <summary>
/// A DXGI composition swap chain whose back buffer is rendered into by OpenGL via WGL_NV_DX_interop.
/// Per frame, Begin() registers + locks the back buffer as a GL renderbuffer attached to an FBO;
/// libmpv renders into that FBO; End() unlocks and presents. Native calls are HRESULT-checked and
/// the per-frame interop path skips the frame (returns false) rather than dereferencing bad handles.
/// </summary>
internal sealed unsafe class VideoSwapChain : IDisposable
{
    private readonly GlInteropDevice _device;
    private int _glColorRenderBuffer;
    private int _glDepthRenderBuffer;
    private IntPtr _dxInteropColor;

    public int BufferWidth { get; private set; }
    public int BufferHeight { get; private set; }
    public IntPtr SwapChainHandle { get; private set; }
    public int GLFrameBufferHandle { get; private set; }

    public VideoSwapChain(GlInteropDevice device, int logicalWidth, int logicalHeight, double scaleX, double scaleY)
    {
        _device = device;
        BufferWidth = Math.Max(1, Convert.ToInt32(logicalWidth * scaleX));
        BufferHeight = Math.Max(1, Convert.ToInt32(logicalHeight * scaleY));

        IDXGISwapChain1* swapChain = null;
        var desc = new SwapChainDesc1
        {
            Width = (uint)BufferWidth,
            Height = (uint)BufferHeight,
            Format = Format.FormatB8G8R8A8Unorm,
            Stereo = 0,
            SampleDesc = new SampleDesc { Count = 1, Quality = 0 },
            BufferUsage = DXGI.UsageRenderTargetOutput,
            BufferCount = 2,
            Scaling = Scaling.Stretch,
            SwapEffect = SwapEffect.FlipDiscard,
            Flags = 0,
            AlphaMode = AlphaMode.Ignore,
        };

        int hr = ((IDXGIFactory2*)_device.DxFactory)->CreateSwapChainForComposition(
            (IUnknown*)_device.DxDevice, &desc, null, &swapChain);
        if (hr < 0 || swapChain == null)
            throw new InvalidOperationException($"CreateSwapChainForComposition failed (0x{hr:X8}).");

        SwapChainHandle = (IntPtr)swapChain;
        GLFrameBufferHandle = GL.GenFramebuffer();

        // Apply the DPI transform up front so the very first frame is correct on a non-1.0 monitor,
        // even if no resize/DPI-change event follows creation.
        ApplyDpiTransform(scaleX, scaleY);
    }

    private void ApplyDpiTransform(double scaleX, double scaleY)
    {
        var transform = new Matrix3X2F
        {
            DXGI11 = 1.0f / (float)scaleX,
            DXGI22 = 1.0f / (float)scaleY,
        };
        ((IDXGISwapChain2*)SwapChainHandle)->SetMatrixTransform(in transform);
    }

    /// <summary>Acquire + lock the back buffer for GL rendering. Returns false (frame skipped) if any
    /// native step fails — e.g. device removed, resize race, or a driver that rejects the back buffer.</summary>
    public bool Begin()
    {
        GL.BindFramebuffer(FramebufferTarget.Framebuffer, GLFrameBufferHandle);

        ID3D11Texture2D* colorBuffer = null;
        Guid guid = typeof(ID3D11Texture2D).GUID;
        int hr = ((IDXGISwapChain1*)SwapChainHandle)->GetBuffer(0, &guid, (void**)&colorBuffer);
        if (hr < 0 || colorBuffer == null)
            return false;

        _glColorRenderBuffer = GL.GenRenderbuffer();
        _glDepthRenderBuffer = GL.GenRenderbuffer();

        // Register the D3D back-buffer texture as a GL renderbuffer (re-registered each frame because
        // FlipDiscard rotates the back buffer — matches the proven reference; optimize later).
        _dxInteropColor = Wgl.DXRegisterObjectNV(
            _device.GlDevice, (nint)colorBuffer, (uint)_glColorRenderBuffer,
            (uint)RenderbufferTarget.Renderbuffer, WGL_NV_DX_interop.AccessReadWrite);
        if (_dxInteropColor == IntPtr.Zero)
        {
            colorBuffer->Release();
            DeleteRenderBuffers();
            return false;
        }

        GL.FramebufferRenderbuffer(FramebufferTarget.Framebuffer, FramebufferAttachment.ColorAttachment0,
            RenderbufferTarget.Renderbuffer, (uint)_glColorRenderBuffer);

        GL.BindRenderbuffer(RenderbufferTarget.Renderbuffer, _glDepthRenderBuffer);
        GL.RenderbufferStorage(RenderbufferTarget.Renderbuffer, RenderbufferStorage.Depth24Stencil8, BufferWidth, BufferHeight);
        GL.FramebufferRenderbuffer(FramebufferTarget.Framebuffer, FramebufferAttachment.DepthAttachment,
            RenderbufferTarget.Renderbuffer, (uint)_glDepthRenderBuffer);
        GL.FramebufferRenderbuffer(FramebufferTarget.Framebuffer, FramebufferAttachment.StencilAttachment,
            RenderbufferTarget.Renderbuffer, (uint)_glDepthRenderBuffer);

        colorBuffer->Release();

        if (!Wgl.DXLockObjectsNV(_device.GlDevice, 1, new[] { _dxInteropColor }))
        {
            Wgl.DXUnregisterObjectNV(_device.GlDevice, _dxInteropColor);
            _dxInteropColor = IntPtr.Zero;
            DeleteRenderBuffers();
            return false;
        }

        GL.BindFramebuffer(FramebufferTarget.Framebuffer, GLFrameBufferHandle);
        GL.Viewport(0, 0, BufferWidth, BufferHeight);
        return true;
    }

    /// <summary>Unlock and present. Only valid after a successful Begin().</summary>
    public void End()
    {
        GL.BindFramebuffer(FramebufferTarget.Framebuffer, 0);
        Wgl.DXUnlockObjectsNV(_device.GlDevice, 1, new[] { _dxInteropColor });
        Wgl.DXUnregisterObjectNV(_device.GlDevice, _dxInteropColor);
        _dxInteropColor = IntPtr.Zero;
        DeleteRenderBuffers();
        ((IDXGISwapChain1*)SwapChainHandle)->Present(0, 0);
    }

    private void DeleteRenderBuffers()
    {
        if (_glColorRenderBuffer != 0) { GL.DeleteRenderbuffer(_glColorRenderBuffer); _glColorRenderBuffer = 0; }
        if (_glDepthRenderBuffer != 0) { GL.DeleteRenderbuffer(_glDepthRenderBuffer); _glDepthRenderBuffer = 0; }
    }

    public void UpdateSize(int logicalWidth, int logicalHeight, double scaleX, double scaleY)
    {
        int newWidth = Convert.ToInt32(logicalWidth * scaleX);
        int newHeight = Convert.ToInt32(logicalHeight * scaleY);
        if (newWidth < 1 || newHeight < 1)
            return;

        // No interop object is registered here (Begin/End bracket each frame synchronously on the UI
        // thread), so the back buffer has no outstanding GL reference at resize time.
        int hr = ((IDXGISwapChain1*)SwapChainHandle)->ResizeBuffers(2, (uint)newWidth, (uint)newHeight, Format.FormatUnknown, 0);
        if (hr < 0)
            return; // keep the previous cached dimensions; do not diverge GL viewport from the real buffer

        BufferWidth = newWidth;
        BufferHeight = newHeight;
        ApplyDpiTransform(scaleX, scaleY);
    }

    public void Dispose()
    {
        GL.DeleteFramebuffer(GLFrameBufferHandle);
        GC.SuppressFinalize(this);
    }
}
