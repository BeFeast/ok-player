namespace OkPlayer.Core;

/// <summary>
/// Static product identity shared across the app. The accent below is the brand override;
/// by default OK Player follows the Windows system accent (PRD §16.3 / §21 Q2).
/// </summary>
public static class AppInfo
{
    public const string ProductName = "OK Player";

    /// <summary>"OK Teal" 500 / base — the signature accent, used as spice only
    /// (seekbar fill, selection, active states, focus). Never splashed across chrome.</summary>
    public const string BrandAccentHex = "#10938A";
}
