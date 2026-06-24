using OkPlayer.Mpv.Interop;

namespace OkPlayer.Mpv;

/// <summary>Thrown when a libmpv call returns a non-success error code.</summary>
public sealed class MpvException : Exception
{
    public MpvError Error { get; }

    public MpvException(MpvError error, string context)
        : base($"{context} failed: {MpvNative.ErrorString(error)} ({(int)error})")
    {
        Error = error;
    }

    internal static void Check(MpvError error, string context)
    {
        if (error != MpvError.Success)
            throw new MpvException(error, context);
    }
}
