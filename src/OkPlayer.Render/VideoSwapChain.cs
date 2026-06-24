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
/// libmpv renders into that FBO; End() unlocks and presents.
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
        BufferWidth = Convert.ToInt32(logicalWidth * scaleX);
        BufferHeight = Convert.ToInt32(logicalHeight * scaleY);

        IDXGISwapChain1* swapChain;
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
        ((IDXGIFactory2*)_device.DxFactory)->CreateSwapChainForComposition(
            (IUnknown*)_device.DxDevice, &desc, null, &swapChain);
        SwapChainHandle = (IntPtr)swapChain;

        GLFrameBufferHandle = GL.GenFramebuffer();
    }

    public void Begin()
    {
        ID3D11Texture2D* colorBuffer;
        GL.BindFramebuffer(FramebufferTarget.Framebuffer, GLFrameBufferHandle);

        {
            Guid guid = typeof(ID3D11Texture2D).GUID;
            ((IDXGISwapChain1*)SwapChainHandle)->GetBuffer(0, &guid, (void**)&colorBuffer);
        }

        _glColorRenderBuffer = GL.GenRenderbuffer();
        _glDepthRenderBuffer = GL.GenRenderbuffer();

        // Register the D3D back-buffer texture as a GL renderbuffer (re-registered each frame because
        // FlipDiscard rotates the back buffer — matches the proven reference; optimize later).
        _dxInteropColor = Wgl.DXRegisterObjectNV(
            _device.GlDevice, (nint)colorBuffer, (uint)_glColorRenderBuffer,
            (uint)RenderbufferTarget.Renderbuffer, WGL_NV_DX_interop.AccessReadWrite);

        GL.FramebufferRenderbuffer(FramebufferTarget.Framebuffer, FramebufferAttachment.ColorAttachment0,
            RenderbufferTarget.Renderbuffer, (uint)_glColorRenderBuffer);

        GL.BindRenderbuffer(RenderbufferTarget.Renderbuffer, _glDepthRenderBuffer);
        GL.RenderbufferStorage(RenderbufferTarget.Renderbuffer, RenderbufferStorage.Depth24Stencil8, BufferWidth, BufferHeight);
        GL.FramebufferRenderbuffer(FramebufferTarget.Framebuffer, FramebufferAttachment.DepthAttachment,
            RenderbufferTarget.Renderbuffer, (uint)_glDepthRenderBuffer);
        GL.FramebufferRenderbuffer(FramebufferTarget.Framebuffer, FramebufferAttachment.StencilAttachment,
            RenderbufferTarget.Renderbuffer, (uint)_glDepthRenderBuffer);

        colorBuffer->Release();

        Wgl.DXLockObjectsNV(_device.GlDevice, 1, new[] { _dxInteropColor });
        GL.BindFramebuffer(FramebufferTarget.Framebuffer, GLFrameBufferHandle);
        GL.Viewport(0, 0, BufferWidth, BufferHeight);
    }

    public void End()
    {
        GL.BindFramebuffer(FramebufferTarget.Framebuffer, 0);
        Wgl.DXUnlockObjectsNV(_device.GlDevice, 1, new[] { _dxInteropColor });
        Wgl.DXUnregisterObjectNV(_device.GlDevice, _dxInteropColor);
        GL.DeleteRenderbuffer(_glColorRenderBuffer);
        GL.DeleteRenderbuffer(_glDepthRenderBuffer);
        ((IDXGISwapChain1*)SwapChainHandle)->Present(0, 0);
    }

    public void UpdateSize(int logicalWidth, int logicalHeight, double scaleX, double scaleY)
    {
        BufferWidth = Convert.ToInt32(logicalWidth * scaleX);
        BufferHeight = Convert.ToInt32(logicalHeight * scaleY);
        ((IDXGISwapChain1*)SwapChainHandle)->ResizeBuffers(2, (uint)BufferWidth, (uint)BufferHeight, Format.FormatUnknown, 0);
        var transform = new Matrix3X2F
        {
            DXGI11 = 1.0f / (float)scaleX,
            DXGI22 = 1.0f / (float)scaleY,
        };
        ((IDXGISwapChain2*)SwapChainHandle)->SetMatrixTransform(in transform);
    }

    public void Dispose()
    {
        GL.DeleteFramebuffer(GLFrameBufferHandle);
        GC.SuppressFinalize(this);
    }
}
