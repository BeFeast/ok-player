using System.Runtime.InteropServices;

namespace OkPlayer.Mpv.Interop;

/// <summary>Opaque libmpv core handle (mpv_handle*).</summary>
[StructLayout(LayoutKind.Sequential)]
public readonly struct MpvHandle
{
    public readonly IntPtr Value;
    public bool IsValid => Value != IntPtr.Zero;
}

/// <summary>Opaque render-context handle (mpv_render_context*).</summary>
[StructLayout(LayoutKind.Sequential)]
public struct MpvRenderContextHandle
{
    public IntPtr Value;
    public readonly bool IsValid => Value != IntPtr.Zero;
}

/// <summary>render.h mpv_render_param — a contiguous, Invalid-terminated array is passed to mpv.</summary>
[StructLayout(LayoutKind.Sequential)]
public struct MpvRenderParam
{
    public MpvRenderParamType Type;
    public IntPtr Data;
}

/// <summary>render_gl.h mpv_opengl_fbo.</summary>
[StructLayout(LayoutKind.Sequential, Size = 16)]
public struct MpvOpenGLFbo
{
    public int Fbo;
    public int W;
    public int H;
    public int InternalFormat;
}

/// <summary>render_gl.h mpv_opengl_init_params. GetProcAddress holds a function pointer
/// (Marshal.GetFunctionPointerForDelegate); keep the source delegate alive.</summary>
[StructLayout(LayoutKind.Sequential)]
public struct MpvOpenGLInitParams
{
    public IntPtr GetProcAddress;
    public IntPtr GetProcAddressContext;
}

/// <summary>client.h mpv_event.</summary>
[StructLayout(LayoutKind.Sequential)]
public struct MpvEvent
{
    public MpvEventId EventId;
    public MpvError Error;
    public ulong ReplyUserData;
    public IntPtr Data;
}

/// <summary>client.h mpv_event_property. Name is UTF-8 (NOT ANSI — fixed vs the reference bug).</summary>
[StructLayout(LayoutKind.Sequential)]
public struct MpvEventProperty
{
    public IntPtr NamePtr;
    public MpvFormat Format;
    public IntPtr Data;

    public readonly string? Name => Marshal.PtrToStringUTF8(NamePtr);
}

/// <summary>client.h mpv_event_log_message (UTF-8 strings).</summary>
[StructLayout(LayoutKind.Sequential)]
public struct MpvEventLogMessage
{
    public IntPtr PrefixPtr;
    public IntPtr LevelPtr;
    public IntPtr TextPtr;
    public MpvLogLevel LogLevel;

    public readonly string Prefix => Marshal.PtrToStringUTF8(PrefixPtr) ?? string.Empty;
    public readonly string Text => Marshal.PtrToStringUTF8(TextPtr) ?? string.Empty;
}

/// <summary>client.h mpv_event_end_file.</summary>
[StructLayout(LayoutKind.Sequential)]
public struct MpvEventEndFile
{
    public MpvEndFileReason Reason;
    public MpvError Error;
    public long PlaylistEntryId;
    public long PlaylistInsertId;
    public int PlaylistInsertNumEntries;
}
