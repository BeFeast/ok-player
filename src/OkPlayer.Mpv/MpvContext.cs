using System.Runtime.InteropServices;
using OkPlayer.Mpv.Interop;

namespace OkPlayer.Mpv;

/// <summary>
/// Managed wrapper around a libmpv core handle: lifecycle, options/commands/properties, property
/// observation, and a background event pump. UI-agnostic — events are raised on the pump thread,
/// so consumers must marshal to their UI dispatcher.
/// </summary>
public sealed class MpvContext : IDisposable
{
    private MpvHandle _handle;
    private Thread? _eventThread;
    private volatile bool _disposed;

    /// <summary>Raised when a file finishes loading (event data has no payload).</summary>
    public event EventHandler? FileLoaded;
    /// <summary>Raised when playback (re)starts after a seek or load.</summary>
    public event EventHandler? PlaybackRestart;
    /// <summary>Raised when the current file ends.</summary>
    public event EventHandler<MpvEndFileReason>? EndFile;
    /// <summary>Raised when mpv is shutting down.</summary>
    public event EventHandler? Shutdown;
    /// <summary>Raised when an observed property changes, with the value parsed from the event itself.
    /// Consumers must use this value rather than calling Get*Property: a synchronous get on the UI
    /// thread can deadlock against a core that is briefly busy (e.g. servicing a screenshot render).</summary>
    public event Action<string, object?>? PropertyChanged;
    /// <summary>Raised for each libmpv log message (level, prefix, text).</summary>
    public event Action<MpvLogLevel, string, string>? LogMessageReceived;
    /// <summary>Raised when an async command (issued via the reply-userdata overload) finishes; the bool is true on success.</summary>
    public event Action<ulong, bool>? CommandReply;

    public MpvContext()
    {
        MpvNative.EnsureResolver();
        _handle = MpvNative.mpv_create();
        if (!_handle.IsValid)
            throw new InvalidOperationException("mpv_create() returned null — libmpv-2.dll could not initialize.");
    }

    /// <summary>Point the loader at a bundled libmpv-2.dll. Call before constructing any MpvContext.</summary>
    public static void SetLibraryPath(string path) => MpvNative.SetCustomMpvPath(path);

    public MpvHandle Handle => _handle;

    public void SetOption(string name, string value)
        => MpvException.Check(MpvNative.mpv_set_option_string(_handle, name, value), $"set_option({name})");

    /// <summary>mpv_initialize. Set options (vo, hwdec, …) first, then call this, then create the render context.</summary>
    public void Initialize()
    {
        MpvException.Check(MpvNative.mpv_initialize(_handle), "initialize");
        _eventThread = new Thread(EventLoop) { IsBackground = true, Name = "mpv-events" };
        _eventThread.Start();
    }

    public void RequestLogMessages(MpvLogLevel level)
    {
        string s = level switch
        {
            MpvLogLevel.Fatal => "fatal",
            MpvLogLevel.Error => "error",
            MpvLogLevel.Warn => "warn",
            MpvLogLevel.Info => "info",
            MpvLogLevel.V => "v",
            MpvLogLevel.Debug => "debug",
            MpvLogLevel.Trace => "trace",
            _ => "no",
        };
        MpvNative.mpv_request_log_messages(_handle, s);
    }

    public void Command(params string[] args)
    {
        // mpv_command wants a NULL-terminated argv.
        var argv = new string?[args.Length + 1];
        Array.Copy(args, argv, args.Length);
        argv[args.Length] = null;
        MpvException.Check(MpvNative.mpv_command(_handle, argv), $"command({string.Join(' ', args)})");
    }

    /// <summary>Fire-and-forget command — does not block the caller. Use for actions that may need a
    /// render to complete (e.g. screenshot), which would deadlock if issued synchronously on the
    /// render thread.</summary>
    public void CommandAsync(params string[] args) => CommandAsync(0, args);

    /// <summary>Async command tagged with <paramref name="replyUserData"/>, echoed back via <see cref="CommandReply"/> when it finishes.</summary>
    public void CommandAsync(ulong replyUserData, params string[] args)
    {
        var argv = new string?[args.Length + 1];
        Array.Copy(args, argv, args.Length);
        argv[args.Length] = null;
        MpvException.Check(MpvNative.mpv_command_async(_handle, replyUserData, argv), $"command_async({string.Join(' ', args)})");
    }

    public void Loadfile(string pathOrUrl) => Command("loadfile", pathOrUrl, "replace");

    public void SetProperty(string name, string value)
        => MpvException.Check(MpvNative.mpv_set_property_string(_handle, name, value), $"set_property({name})");

    public void SetProperty(string name, double value)
        => MpvException.Check(MpvNative.mpv_set_property_double(_handle, name, MpvFormat.Double, ref value), $"set_property({name})");

    public void SetProperty(string name, bool value)
    {
        int flag = value ? 1 : 0;
        MpvException.Check(MpvNative.mpv_set_property_flag(_handle, name, MpvFormat.Flag, ref flag), $"set_property({name})");
    }

    public string? GetPropertyString(string name)
    {
        IntPtr ptr = MpvNative.mpv_get_property_string_raw(_handle, name);
        if (ptr == IntPtr.Zero)
            return null;
        try { return Marshal.PtrToStringUTF8(ptr); }
        finally { MpvNative.mpv_free(ptr); } // mpv owns the heap string — free it (fixes reference leak)
    }

    public double? GetPropertyDouble(string name)
        => MpvNative.mpv_get_property_double(_handle, name, MpvFormat.Double, out double v) == MpvError.Success ? v : null;

    public long? GetPropertyLong(string name)
        => MpvNative.mpv_get_property_long(_handle, name, MpvFormat.Int64, out long v) == MpvError.Success ? v : null;

    public bool? GetPropertyBool(string name)
        => MpvNative.mpv_get_property_flag(_handle, name, MpvFormat.Flag, out int v) == MpvError.Success ? v != 0 : null;

    public void ObserveProperty(string name, MpvFormat format = MpvFormat.None)
        => MpvException.Check(MpvNative.mpv_observe_property(_handle, 0, name, format), $"observe_property({name})");

    private void EventLoop()
    {
        while (!_disposed)
        {
            IntPtr evPtr = MpvNative.mpv_wait_event(_handle, -1);
            if (evPtr == IntPtr.Zero)
                continue;

            MpvEvent ev = Marshal.PtrToStructure<MpvEvent>(evPtr);
            switch (ev.EventId)
            {
                case MpvEventId.Shutdown:
                    Shutdown?.Invoke(this, EventArgs.Empty);
                    return;
                case MpvEventId.LogMessage:
                    var log = Marshal.PtrToStructure<MpvEventLogMessage>(ev.Data);
                    LogMessageReceived?.Invoke(log.LogLevel, log.Prefix, log.Text);
                    break;
                case MpvEventId.FileLoaded:
                    FileLoaded?.Invoke(this, EventArgs.Empty);
                    break;
                case MpvEventId.EndFile:
                    var end = Marshal.PtrToStructure<MpvEventEndFile>(ev.Data);
                    EndFile?.Invoke(this, end.Reason);
                    break;
                case MpvEventId.PlaybackRestart:
                    PlaybackRestart?.Invoke(this, EventArgs.Empty);
                    break;
                case MpvEventId.CommandReply:
                    CommandReply?.Invoke(ev.ReplyUserData, (int)ev.Error == 0); // mpv error 0 == success
                    break;
                case MpvEventId.PropertyChange:
                    var prop = Marshal.PtrToStructure<MpvEventProperty>(ev.Data);
                    var name = prop.Name;
                    if (name is not null)
                        PropertyChanged?.Invoke(name, ParsePropertyValue(prop));
                    break;
            }
        }
    }

    /// <summary>Read an observed property's value straight from the event payload (valid only during
    /// this event), so consumers never have to call back into mpv on another thread.</summary>
    private static object? ParsePropertyValue(MpvEventProperty prop)
    {
        if (prop.Data == IntPtr.Zero)
            return null;
        return prop.Format switch
        {
            MpvFormat.Double => Marshal.PtrToStructure<double>(prop.Data),
            MpvFormat.Flag => Marshal.ReadInt32(prop.Data) != 0,
            MpvFormat.Int64 => Marshal.ReadInt64(prop.Data),
            MpvFormat.String or MpvFormat.OsdString => Marshal.PtrToStringUTF8(Marshal.ReadIntPtr(prop.Data)),
            _ => null,
        };
    }

    public void Dispose()
    {
        if (_disposed)
            return;
        _disposed = true;
        if (_handle.IsValid)
        {
            MpvNative.mpv_wakeup(_handle); // unblock the pump's mpv_wait_event so it observes _disposed
            bool pumpExited = _eventThread is null || _eventThread.Join(TimeSpan.FromSeconds(5));
            if (pumpExited)
            {
                // Safe to destroy: the pump has returned and will not touch the handle again.
                MpvNative.mpv_terminate_destroy(_handle);
                _handle = default;
            }
            // If the pump is still stuck, leak the handle rather than risk a use-after-free.
        }
    }
}
