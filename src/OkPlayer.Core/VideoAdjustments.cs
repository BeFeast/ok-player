namespace OkPlayer.Core;

/// <summary>The four global libmpv picture controls exposed by both desktop shells.</summary>
public enum VideoAdjustmentKind
{
    Brightness,
    Contrast,
    Saturation,
    Gamma,
}

/// <summary>
/// Shared bounds, neutral value, and libmpv property mapping for picture adjustments.
/// Keeping this in the headless core prevents each UI shell from inventing its own range or names.
/// </summary>
public static class VideoAdjustments
{
    public const double Minimum = -100.0;
    public const double Maximum = 100.0;
    public const double Neutral = 0.0;

    public static double Normalize(double value)
        => double.IsFinite(value) ? Math.Clamp(value, Minimum, Maximum) : Neutral;

    public static string MpvProperty(VideoAdjustmentKind kind) => kind switch
    {
        VideoAdjustmentKind.Brightness => "brightness",
        VideoAdjustmentKind.Contrast => "contrast",
        VideoAdjustmentKind.Saturation => "saturation",
        VideoAdjustmentKind.Gamma => "gamma",
        _ => throw new ArgumentOutOfRangeException(nameof(kind), kind, null),
    };
}
