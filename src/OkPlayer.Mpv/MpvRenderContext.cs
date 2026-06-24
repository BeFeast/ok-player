using System.Runtime.InteropServices;
using OkPlayer.Mpv.Interop;

namespace OkPlayer.Mpv;

/// <summary>
/// Managed wrapper over the libmpv render API (OpenGL). Created on the thread whose GL context is
/// current; Render/ReportSwap must run on that same thread. The update callback fires on an
/// arbitrary mpv thread and must only signal the render thread — never render inline.
/// </summary>
public sealed class MpvRenderContext : IDisposable
{
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate IntPtr GetProcAddressDelegate(IntPtr ctx, [MarshalAs(UnmanagedType.LPUTF8Str)] string name);

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate void UpdateCallbackDelegate(IntPtr ctx);

    private MpvRenderContextHandle _context;
    private readonly GetProcAddressDelegate _getProcAddress; // kept alive for the context lifetime
    private UpdateCallbackDelegate? _updateCallback;          // kept alive while registered

    public MpvRenderContext(MpvContext core, Func<string, IntPtr> glGetProcAddress)
    {
        _getProcAddress = (_, name) => glGetProcAddress(name);

        IntPtr apiType = Marshal.StringToCoTaskMemUTF8("opengl");
        var glInit = new MpvOpenGLInitParams
        {
            GetProcAddress = Marshal.GetFunctionPointerForDelegate(_getProcAddress),
            GetProcAddressContext = IntPtr.Zero,
        };
        IntPtr glInitPtr = Marshal.AllocHGlobal(Marshal.SizeOf<MpvOpenGLInitParams>());
        Marshal.StructureToPtr(glInit, glInitPtr, false);

        IntPtr advancedPtr = Marshal.AllocHGlobal(sizeof(int));
        Marshal.WriteInt32(advancedPtr, 1); // MPV_RENDER_PARAM_ADVANCED_CONTROL = 1

        var parameters = new[]
        {
            new MpvRenderParam { Type = MpvRenderParamType.ApiType, Data = apiType },
            new MpvRenderParam { Type = MpvRenderParamType.OpenGLInitParams, Data = glInitPtr },
            new MpvRenderParam { Type = MpvRenderParamType.AdvancedControl, Data = advancedPtr },
            new MpvRenderParam { Type = MpvRenderParamType.Invalid, Data = IntPtr.Zero },
        };

        try
        {
            MpvException.Check(
                MpvNative.mpv_render_context_create(out _context, core.Handle, parameters),
                "render_context_create");
        }
        finally
        {
            Marshal.FreeHGlobal(glInitPtr);
            Marshal.FreeHGlobal(advancedPtr);
            Marshal.FreeCoTaskMem(apiType);
        }
    }

    /// <summary>Register a callback invoked (on an arbitrary thread) when a new frame is available.
    /// The callback must only signal the render thread.</summary>
    public void SetUpdateCallback(Action onUpdate)
    {
        _updateCallback = _ => onUpdate();
        MpvNative.mpv_render_context_set_update_callback(
            _context, Marshal.GetFunctionPointerForDelegate(_updateCallback), IntPtr.Zero);
    }

    /// <summary>Poll for pending updates; returns Frame when a new frame should be rendered.</summary>
    public MpvRenderUpdateFlag Update() => MpvNative.mpv_render_context_update(_context);

    /// <summary>Render the current frame into the given GL FBO (physical pixels). FlipY=0: the FBO's
    /// color attachment is the D3D back-buffer (origins already match).</summary>
    public void Render(int fbo, int width, int height)
    {
        var fboStruct = new MpvOpenGLFbo { Fbo = fbo, W = width, H = height, InternalFormat = 0 };
        IntPtr fboPtr = Marshal.AllocHGlobal(Marshal.SizeOf<MpvOpenGLFbo>());
        Marshal.StructureToPtr(fboStruct, fboPtr, false);

        IntPtr flipPtr = Marshal.AllocHGlobal(sizeof(int)); // 4 bytes (fixes reference 0-byte bug)
        Marshal.WriteInt32(flipPtr, 0);

        var parameters = new[]
        {
            new MpvRenderParam { Type = MpvRenderParamType.Fbo, Data = fboPtr },
            new MpvRenderParam { Type = MpvRenderParamType.FlipY, Data = flipPtr },
            new MpvRenderParam { Type = MpvRenderParamType.Invalid, Data = IntPtr.Zero },
        };

        try { MpvNative.mpv_render_context_render(_context, parameters); }
        finally
        {
            Marshal.FreeHGlobal(fboPtr);
            Marshal.FreeHGlobal(flipPtr);
        }
    }

    /// <summary>Tell mpv a present happened (keeps its A/V timing model correct).</summary>
    public void ReportSwap() => MpvNative.mpv_render_context_report_swap(_context);

    public void Dispose()
    {
        if (_context.IsValid)
        {
            MpvNative.mpv_render_context_set_update_callback(_context, IntPtr.Zero, IntPtr.Zero);
            MpvNative.mpv_render_context_free(_context);
            _context = default;
        }
        _updateCallback = null;
    }
}
