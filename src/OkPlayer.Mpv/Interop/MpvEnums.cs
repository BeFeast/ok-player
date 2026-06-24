namespace OkPlayer.Mpv.Interop;

/// <summary>libmpv error codes (client.h mpv_error). 0 = success, negatives are errors.</summary>
public enum MpvError
{
    Success = 0,
    EventQueueFull = -1,
    NoMem = -2,
    Uninitialized = -3,
    InvalidParameter = -4,
    OptionNotFound = -5,
    OptionFormat = -6,
    OptionError = -7,
    PropertyNotFound = -8,
    PropertyFormat = -9,
    PropertyUnavailable = -10,
    PropertyError = -11,
    Command = -12,
    LoadingFailed = -13,
    AudioOutputInitFailed = -14,
    VideoOutputInitFailed = -15,
    NothingToPlay = -16,
    UnknownFormat = -17,
    Unsupported = -18,
    NotImplemented = -19,
    Generic = -20,
}

/// <summary>libmpv data formats (client.h mpv_format).</summary>
public enum MpvFormat
{
    None = 0,
    String = 1,
    OsdString = 2,
    Flag = 3,
    Int64 = 4,
    Double = 5,
    Node = 6,
    NodeArray = 7,
    NodeMap = 8,
    ByteArray = 9,
}

/// <summary>libmpv event ids (client.h mpv_event_id) — the subset OK Player consumes.</summary>
public enum MpvEventId
{
    None = 0,
    Shutdown = 1,
    LogMessage = 2,
    GetPropertyReply = 3,
    SetPropertyReply = 4,
    CommandReply = 5,
    StartFile = 6,
    EndFile = 7,
    FileLoaded = 8,
    ClientMessage = 16,
    VideoReconfig = 17,
    AudioReconfig = 18,
    Seek = 20,
    PlaybackRestart = 21,
    PropertyChange = 22,
    QueueOverflow = 24,
    Hook = 25,
}

/// <summary>libmpv log levels (client.h mpv_log_level).</summary>
public enum MpvLogLevel
{
    None = 0,
    Fatal = 10,
    Error = 20,
    Warn = 30,
    Info = 40,
    V = 50,
    Debug = 60,
    Trace = 70,
}

/// <summary>Reason an EndFile event fired (client.h mpv_end_file_reason).</summary>
public enum MpvEndFileReason
{
    Eof = 0,
    Stop = 2,
    Quit = 3,
    Error = 4,
    Redirect = 5,
}

/// <summary>render.h mpv_render_param_type.</summary>
public enum MpvRenderParamType
{
    Invalid = 0,
    ApiType = 1,
    OpenGLInitParams = 2,
    Fbo = 3,
    FlipY = 4,
    Depth = 5,
    IccProfile = 6,
    AmbientLight = 7,
    X11Display = 8,
    WaylandDisplay = 9,
    AdvancedControl = 10,
    NextFrameInfo = 11,
    BlockForTargetTime = 12,
    SkipRendering = 13,
}

/// <summary>render.h mpv_render_update_flag.</summary>
[System.Flags]
public enum MpvRenderUpdateFlag : ulong
{
    None = 0,
    Frame = 1,
}
