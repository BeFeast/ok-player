using OpenTK;
using OpenTK.Graphics.Wgl;
using OpenTK.Windowing.Common;
using OpenTK.Windowing.Desktop;
using OpenTK.Windowing.GraphicsLibraryFramework;
using Silk.NET.Core.Native;
using Silk.NET.Direct3D11;
using Silk.NET.DXGI;

namespace OkPlayer.Render;

/// <summary>
/// Owns the D3D11 device/factory and the process-wide shared OpenGL context, and bridges them with
/// WGL_NV_DX_interop (wglDXOpenDeviceNV). The GL context is made current on the constructing thread
/// (the UI thread) and all GL + mpv_render calls must run on that thread.
/// </summary>
internal sealed unsafe class GlInteropDevice
{
    private static IGraphicsContext? s_sharedContext;
    private static IBindingsContext? s_sharedBindings;
    // The hidden GLFW window owns the WGL context; keep it alive for the process lifetime so the GC
    // can't finalize it out from under the stored context pointer.
    private static NativeWindow? s_sharedWindow;

    public IntPtr DxFactory { get; }
    public IntPtr DxDevice { get; }
    public IntPtr DxDeviceContext { get; }
    public IntPtr GlDevice { get; }

    public GlInteropDevice()
    {
        IDXGIFactory2* factory;
        ID3D11Device* device;
        ID3D11DeviceContext* deviceContext;

        {
            Guid guid = typeof(IDXGIFactory2).GUID;
            DXGI.GetApi(null).CreateDXGIFactory2(0, &guid, (void**)&factory);
        }
        {
            var flags = CreateDeviceFlag.BgraSupport | CreateDeviceFlag.VideoSupport;
            D3D11.GetApi(null).CreateDevice(
                null, D3DDriverType.Hardware, 0, (uint)flags, null, 0, D3D11.SdkVersion,
                &device, null, &deviceContext);
        }

        DxFactory = (IntPtr)factory;
        DxDevice = (IntPtr)device;
        DxDeviceContext = (IntPtr)deviceContext;

        EnsureSharedGlContext();

        GlDevice = Wgl.DXOpenDeviceNV((IntPtr)device);
        if (GlDevice == IntPtr.Zero)
            throw new NotSupportedException(
                "WGL_NV_DX_interop is unavailable on this GPU/driver (wglDXOpenDeviceNV returned null). " +
                "An ANGLE/EGL fallback backend is required on such machines.");
    }

    /// <summary>GL function-pointer loader handed to libmpv's render API.</summary>
    public static IntPtr GetProcAddress(string name) => s_sharedBindings?.GetProcAddress(name) ?? IntPtr.Zero;

    private static void EnsureSharedGlContext()
    {
        if (s_sharedContext != null)
            return;

        var settings = NativeWindowSettings.Default;
        settings.StartFocused = false;
        settings.StartVisible = false;
        settings.NumberOfSamples = 0;
        settings.APIVersion = new Version(4, 6);
        settings.Flags = ContextFlags.Offscreen;
        settings.Profile = ContextProfile.Compatability; // OpenTK's spelling; matches the 4.6 compat ask
        settings.WindowBorder = WindowBorder.Hidden;
        settings.WindowState = WindowState.Minimized;

        s_sharedWindow = new NativeWindow(settings); // hidden GLFW window + real WGL context; kept alive

        s_sharedBindings = new GLFWBindingsContext();
        Wgl.LoadBindings(s_sharedBindings); // load WGL extension entry points (incl. NV_DX_interop)

        s_sharedContext = s_sharedWindow.Context;
        s_sharedContext.MakeCurrent();
    }
}
