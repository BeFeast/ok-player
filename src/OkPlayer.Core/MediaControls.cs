using System;
using System.Collections.Generic;

namespace OkPlayer.Core;

/// <summary>Portable commands exposed by an operating-system media session.</summary>
public enum MediaControlCommand
{
    Play,
    Pause,
    Stop,
    Next,
    Previous,
}

/// <summary>Portable playback states projected by an operating-system media session.</summary>
public enum MediaControlPlaybackStatus
{
    Closed,
    Paused,
    Playing,
}

/// <summary>The player state needed to project an operating-system media session.</summary>
public readonly record struct MediaControlSnapshot(
    bool HasMedia,
    bool IsPaused,
    double PositionSeconds,
    double DurationSeconds,
    double PlaybackRate,
    bool CanGoNext,
    bool CanGoPrevious);

/// <summary>A normalized, platform-neutral operating-system media-session projection.</summary>
public readonly record struct MediaControlProjection(
    MediaControlPlaybackStatus PlaybackStatus,
    TimeSpan Position,
    TimeSpan Duration,
    double PlaybackRate,
    bool CanPlay,
    bool CanPause,
    bool CanStop,
    bool CanGoNext,
    bool CanGoPrevious);

/// <summary>Normalized metadata shared by operating-system media-session projections.</summary>
public readonly record struct MediaControlMetadata(string Title, string? Artist, string? Album)
{
    public string SecondaryText => string.Join(" · ", Present(Artist, Album));

    private static IEnumerable<string> Present(params string?[] values)
    {
        foreach (string? value in values)
            if (!string.IsNullOrWhiteSpace(value))
                yield return value!;
    }
}

/// <summary>
/// Pure mapping and normalization for OS media controls. Platform shells translate their native events into
/// this surface, then dispatch the returned command through the player's existing command path.
/// </summary>
public static class MediaControls
{
    public const double MinimumPlaybackRate = 0.25;
    public const double MaximumPlaybackRate = 4.0;

    /// <summary>Map a native transport-button name. Unknown/unsupported buttons deliberately return null.</summary>
    public static MediaControlCommand? CommandFromButtonName(string? buttonName) => buttonName switch
    {
        "Play" => MediaControlCommand.Play,
        "Pause" => MediaControlCommand.Pause,
        "Stop" => MediaControlCommand.Stop,
        "Next" => MediaControlCommand.Next,
        "Previous" => MediaControlCommand.Previous,
        _ => null,
    };

    public static MediaControlProjection Project(MediaControlSnapshot snapshot)
    {
        if (!snapshot.HasMedia)
        {
            return new MediaControlProjection(
                MediaControlPlaybackStatus.Closed,
                TimeSpan.Zero,
                TimeSpan.Zero,
                1.0,
                CanPlay: false,
                CanPause: false,
                CanStop: false,
                CanGoNext: false,
                CanGoPrevious: false);
        }

        double duration = NonNegativeFinite(snapshot.DurationSeconds);
        // SMTC requires Position to sit inside StartTime..EndTime. With no known end (live/settling media),
        // publish a zeroed timeline while still projecting the playing/paused state.
        double position = duration > 0 ? NormalizePosition(snapshot.PositionSeconds, duration) : 0;
        double rate = NormalizePlaybackRate(snapshot.PlaybackRate) ?? 1.0;
        return new MediaControlProjection(
            snapshot.IsPaused ? MediaControlPlaybackStatus.Paused : MediaControlPlaybackStatus.Playing,
            TimeSpan.FromSeconds(position),
            TimeSpan.FromSeconds(duration),
            rate,
            CanPlay: true,
            CanPause: true,
            CanStop: true,
            snapshot.CanGoNext,
            snapshot.CanGoPrevious);
    }

    /// <summary>Normalize an absolute seek request to the known media bounds.</summary>
    public static double NormalizePosition(double requestedSeconds, double durationSeconds)
    {
        double requested = NonNegativeFinite(requestedSeconds);
        double duration = NonNegativeFinite(durationSeconds);
        return duration > 0 ? Math.Clamp(requested, 0, duration) : requested;
    }

    /// <summary>Normalize a requested playback rate to the same supported range as Linux MPRIS.</summary>
    public static double? NormalizePlaybackRate(double requestedRate)
        => double.IsFinite(requestedRate)
            ? Math.Clamp(requestedRate, MinimumPlaybackRate, MaximumPlaybackRate)
            : null;

    /// <summary>
    /// Resolve metadata from the same display title used by the player window, supplemented by media tags and
    /// the file name. The display title wins so a curated NFO title remains consistent across app and OS UI.
    /// </summary>
    public static MediaControlMetadata ResolveMetadata(
        string? displayTitle,
        string? tagArtist,
        string? tagTitle,
        string? tagAlbum,
        string? fileStem)
    {
        string? display = Clean(displayTitle);
        string? stem = Clean(fileStem);
        string? title = display ?? Clean(tagTitle) ?? stem ?? "OK Player";
        var (artist, _) = TrackTags.Resolve(tagArtist, tagTitle, display, stem);
        return new MediaControlMetadata(title, Clean(artist), Clean(tagAlbum));
    }

    /// <summary>Pick an early representative frame for OS artwork without seeking to the very first fade-in.</summary>
    public static double ArtworkPosition(double durationSeconds)
    {
        double duration = NonNegativeFinite(durationSeconds);
        return duration <= 0 ? 0 : Math.Min(30, duration * 0.1);
    }

    private static double NonNegativeFinite(double value)
        => double.IsFinite(value) ? Math.Clamp(value, 0, TimeSpan.MaxValue.TotalSeconds - 1) : 0;
    private static string? Clean(string? value) => string.IsNullOrWhiteSpace(value) ? null : value.Trim();
}
