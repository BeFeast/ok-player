using System.Reflection;
using System.Runtime.InteropServices;

namespace OkPlayer.Mpv.Interop;

/// <summary>
/// Raw libmpv P/Invoke surface via [LibraryImport] source generators.
/// Import name "mpv" resolves to libmpv-2.dll (or a custom path) and "gl" to opengl32.dll
/// through a DllImportResolver that must be installed before the first call (EnsureResolver()).
/// </summary>
internal static partial class MpvNative
{
    private const string Lib = "mpv";

    private static int _resolverInstalled;
    private static string? _customMpvPath;

    /// <summary>Point the "mpv" import at a bundled libmpv-2.dll. Call before EnsureResolver()/first use.</summary>
    public static void SetCustomMpvPath(string? path) => _customMpvPath = path;

    public static void EnsureResolver()
    {
        if (Interlocked.Exchange(ref _resolverInstalled, 1) == 0)
            NativeLibrary.SetDllImportResolver(typeof(MpvNative).Assembly, Resolve);
    }

    private static IntPtr Resolve(string name, Assembly assembly, DllImportSearchPath? searchPath)
    {
        string file = name switch
        {
            "mpv" => string.IsNullOrEmpty(_customMpvPath) ? "libmpv-2.dll" : _customMpvPath!,
            "gl" => "opengl32.dll",
            _ => name,
        };
        return NativeLibrary.Load(file, assembly, searchPath);
    }

    public static string ErrorString(MpvError error)
        => Marshal.PtrToStringUTF8(mpv_error_string(error)) ?? error.ToString();

    // ---- client.h ----
    [LibraryImport(Lib)] internal static partial MpvHandle mpv_create();
    [LibraryImport(Lib)] internal static partial MpvError mpv_initialize(MpvHandle handle);
    [LibraryImport(Lib)] internal static partial void mpv_terminate_destroy(MpvHandle handle);
    [LibraryImport(Lib)] internal static partial void mpv_wakeup(MpvHandle handle);
    [LibraryImport(Lib)] internal static partial IntPtr mpv_wait_event(MpvHandle handle, double timeout);
    [LibraryImport(Lib)] internal static partial void mpv_free(IntPtr data);
    [LibraryImport(Lib)] internal static partial IntPtr mpv_error_string(MpvError error);

    [LibraryImport(Lib, StringMarshalling = StringMarshalling.Utf8)]
    internal static partial MpvError mpv_set_option_string(MpvHandle handle, string name, string data);

    [LibraryImport(Lib, StringMarshalling = StringMarshalling.Utf8)]
    internal static partial MpvError mpv_command(MpvHandle handle, [In] string?[] args);

    [LibraryImport(Lib, StringMarshalling = StringMarshalling.Utf8)]
    internal static partial MpvError mpv_command_async(MpvHandle handle, ulong replyUserData, [In] string?[] args);

    [LibraryImport(Lib, StringMarshalling = StringMarshalling.Utf8)]
    internal static partial MpvError mpv_set_property_string(MpvHandle handle, string name, string data);

    [LibraryImport(Lib, EntryPoint = "mpv_get_property_string", StringMarshalling = StringMarshalling.Utf8)]
    internal static partial IntPtr mpv_get_property_string_raw(MpvHandle handle, string name);

    [LibraryImport(Lib, EntryPoint = "mpv_set_property", StringMarshalling = StringMarshalling.Utf8)]
    internal static partial MpvError mpv_set_property_double(MpvHandle handle, string name, MpvFormat format, ref double data);

    [LibraryImport(Lib, EntryPoint = "mpv_set_property", StringMarshalling = StringMarshalling.Utf8)]
    internal static partial MpvError mpv_set_property_flag(MpvHandle handle, string name, MpvFormat format, ref int data);

    [LibraryImport(Lib, EntryPoint = "mpv_get_property", StringMarshalling = StringMarshalling.Utf8)]
    internal static partial MpvError mpv_get_property_double(MpvHandle handle, string name, MpvFormat format, out double data);

    [LibraryImport(Lib, EntryPoint = "mpv_get_property", StringMarshalling = StringMarshalling.Utf8)]
    internal static partial MpvError mpv_get_property_flag(MpvHandle handle, string name, MpvFormat format, out int data);

    [LibraryImport(Lib, EntryPoint = "mpv_get_property", StringMarshalling = StringMarshalling.Utf8)]
    internal static partial MpvError mpv_get_property_long(MpvHandle handle, string name, MpvFormat format, out long data);

    [LibraryImport(Lib, StringMarshalling = StringMarshalling.Utf8)]
    internal static partial MpvError mpv_observe_property(MpvHandle handle, ulong replyUserData, string name, MpvFormat format);

    [LibraryImport(Lib, StringMarshalling = StringMarshalling.Utf8)]
    internal static partial MpvError mpv_request_log_messages(MpvHandle handle, string minLevel);

    // ---- render.h / render_gl.h ----
    [LibraryImport(Lib)]
    internal static partial MpvError mpv_render_context_create(out MpvRenderContextHandle context, MpvHandle handle, [In] MpvRenderParam[] parameters);

    [LibraryImport(Lib)]
    internal static partial MpvError mpv_render_context_render(MpvRenderContextHandle context, [In] MpvRenderParam[] parameters);

    [LibraryImport(Lib)]
    internal static partial MpvError mpv_render_context_report_swap(MpvRenderContextHandle context);

    [LibraryImport(Lib)]
    internal static partial MpvRenderUpdateFlag mpv_render_context_update(MpvRenderContextHandle context);

    [LibraryImport(Lib)]
    internal static partial void mpv_render_context_set_update_callback(MpvRenderContextHandle context, IntPtr callback, IntPtr callbackContext);

    [LibraryImport(Lib)]
    internal static partial void mpv_render_context_free(MpvRenderContextHandle context);
}
