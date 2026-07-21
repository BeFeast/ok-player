using System;
using System.ComponentModel;
using System.IO;
using System.Threading;
using System.Threading.Tasks;
using OkPlayer.App.ViewModels;
using OkPlayer.App.Views;
using OkPlayer.Core;
using Windows.Media;
using Windows.Storage;
using Windows.Storage.Streams;

namespace OkPlayer.App.Services;

/// <summary>
/// Thin WinUI/SMTC projection over the existing player command and observed-state surfaces. Native callbacks
/// are marshalled to the XAML dispatcher; metadata and artwork reads stay asynchronous.
/// </summary>
internal sealed class SystemMediaControlsService : IDisposable
{
    private readonly PlayerView _player;
    private readonly SystemMediaTransportControls _controls;
    private readonly Microsoft.UI.Dispatching.DispatcherQueue _dispatcher;
    private CancellationTokenSource? _metadataCancellation;
    private int _metadataGeneration;
    private bool _disposed;

    private SystemMediaControlsService(PlayerView player, SystemMediaTransportControls controls)
    {
        _player = player;
        _controls = controls;
        _dispatcher = player.DispatcherQueue;

        _controls.IsEnabled = false;
        _controls.ButtonPressed += OnButtonPressed;
        _controls.PlaybackPositionChangeRequested += OnPlaybackPositionChangeRequested;
        _controls.PlaybackRateChangeRequested += OnPlaybackRateChangeRequested;
        _player.Vm.PropertyChanged += OnPlayerPropertyChanged;
        _player.MediaControlPlaylistChanged += OnPlaylistChanged;
        _player.MediaControlMetadataChanged += OnMetadataChanged;

        UpdateState();
        RefreshMetadataAndArtwork();
    }

    public static SystemMediaControlsService? TryCreate(IntPtr windowHandle, PlayerView player)
    {
        try
        {
            var controls = SystemMediaTransportControlsInterop.GetForWindow(windowHandle);
            return new SystemMediaControlsService(player, controls);
        }
        catch (Exception ex)
        {
            Log.Warn("SMTC unavailable: " + ex.Message);
            return null;
        }
    }

    private void OnButtonPressed(
        SystemMediaTransportControls sender,
        SystemMediaTransportControlsButtonPressedEventArgs args)
    {
        MediaControlCommand? command = MediaControls.CommandFromButtonName(args.Button.ToString());
        if (command is { } supported)
            _dispatcher.TryEnqueue(() =>
            {
                if (!_disposed)
                    _player.ExecuteMediaControlCommand(supported);
            });
    }

    private void OnPlaybackPositionChangeRequested(
        SystemMediaTransportControls sender,
        PlaybackPositionChangeRequestedEventArgs args)
    {
        double requested = args.RequestedPlaybackPosition.TotalSeconds;
        _dispatcher.TryEnqueue(() =>
        {
            if (_disposed || !_player.Vm.HasMedia)
                return;
            double seconds = MediaControls.NormalizePosition(requested, _player.Vm.Duration);
            _player.Vm.SeekToSeconds(seconds);
        });
    }

    private void OnPlaybackRateChangeRequested(
        SystemMediaTransportControls sender,
        PlaybackRateChangeRequestedEventArgs args)
    {
        double? requested = MediaControls.NormalizePlaybackRate(args.RequestedPlaybackRate);
        if (requested is not { } rate)
            return;
        _dispatcher.TryEnqueue(() =>
        {
            if (!_disposed && _player.Vm.HasMedia)
                _player.Vm.SetSpeed(rate);
        });
    }

    private void OnPlayerPropertyChanged(object? sender, PropertyChangedEventArgs args)
    {
        switch (args.PropertyName)
        {
            case nameof(PlayerViewModel.HasMedia):
                UpdateState();
                RefreshMetadataAndArtwork();
                break;
            case nameof(PlayerViewModel.IsPaused):
            case nameof(PlayerViewModel.Position):
            case nameof(PlayerViewModel.Duration):
            case nameof(PlayerViewModel.Speed):
                UpdateState();
                break;
            case nameof(PlayerViewModel.MediaTitle):
                RefreshMetadataAndArtwork();
                break;
        }
    }

    private void OnPlaylistChanged(object? sender, EventArgs args) => UpdateState();
    private void OnMetadataChanged(object? sender, EventArgs args) => RefreshMetadataAndArtwork();

    private void UpdateState()
    {
        if (_disposed)
            return;
        try
        {
            var projected = MediaControls.Project(new MediaControlSnapshot(
                _player.Vm.HasMedia,
                _player.Vm.IsPaused,
                _player.Vm.Position,
                _player.Vm.Duration,
                _player.Vm.Speed,
                _player.CanPlayNext,
                _player.CanPlayPrevious));

            _controls.IsPlayEnabled = projected.CanPlay;
            _controls.IsPauseEnabled = projected.CanPause;
            _controls.IsStopEnabled = projected.CanStop;
            _controls.IsNextEnabled = projected.CanGoNext;
            _controls.IsPreviousEnabled = projected.CanGoPrevious;
            _controls.PlaybackStatus = projected.PlaybackStatus switch
            {
                MediaControlPlaybackStatus.Playing => MediaPlaybackStatus.Playing,
                MediaControlPlaybackStatus.Paused => MediaPlaybackStatus.Paused,
                _ => MediaPlaybackStatus.Closed,
            };
            _controls.PlaybackRate = projected.PlaybackRate;
            _controls.UpdateTimelineProperties(new SystemMediaTransportControlsTimelineProperties
            {
                StartTime = TimeSpan.Zero,
                MinSeekTime = TimeSpan.Zero,
                Position = projected.Position,
                MaxSeekTime = projected.Duration,
                EndTime = projected.Duration,
            });
            _controls.IsEnabled = _player.Vm.HasMedia;
        }
        catch (Exception ex)
        {
            Log.Warn("SMTC state update failed: " + ex.Message);
        }
    }

    private void RefreshMetadataAndArtwork()
    {
        _metadataCancellation?.Cancel();
        _metadataCancellation?.Dispose();
        _metadataCancellation = null;
        int generation = ++_metadataGeneration;
        string? path = _player.CurrentMediaPath;

        if (!_player.Vm.HasMedia || string.IsNullOrEmpty(path))
        {
            ClearMetadata();
            return;
        }

        var cancellation = new CancellationTokenSource();
        _metadataCancellation = cancellation;
        _ = RefreshMetadataAndArtworkAsync(path, generation, cancellation.Token);
    }

    private async Task RefreshMetadataAndArtworkAsync(string path, int generation, CancellationToken cancellationToken)
    {
        try
        {
            // Publish text with the app-icon fallback first. Embedded covers and decoded video frames can take
            // seconds; they must not hold title/artist/album hostage while extraction catches up.
            MediaControlMetadata metadata = await _player.ReadMediaControlMetadataAsync(path, cancellationToken);
            RandomAccessStreamReference? fallback = await CreateThumbnailAsync(null, cancellationToken);
            cancellationToken.ThrowIfCancellationRequested();
            bool isAudio = MediaFormats.IsAudio(path);
            _dispatcher.TryEnqueue(() =>
            {
                if (_disposed || generation != _metadataGeneration || _player.CurrentMediaPath != path || !_player.Vm.HasMedia)
                    return;
                try
                {
                    var updater = _controls.DisplayUpdater;
                    updater.ClearAll();
                    updater.Type = isAudio ? MediaPlaybackType.Music : MediaPlaybackType.Video;
                    if (isAudio)
                    {
                        updater.MusicProperties.Title = metadata.Title;
                        updater.MusicProperties.Artist = metadata.Artist ?? string.Empty;
                        updater.MusicProperties.AlbumArtist = metadata.Artist ?? string.Empty;
                        updater.MusicProperties.AlbumTitle = metadata.Album ?? string.Empty;
                    }
                    else
                    {
                        updater.VideoProperties.Title = metadata.Title;
                        updater.VideoProperties.Subtitle = metadata.SecondaryText;
                    }
                    updater.Thumbnail = fallback;
                    updater.Update();
                }
                catch (Exception ex)
                {
                    Log.Warn("SMTC metadata update failed: " + ex.Message);
                }
            });

            string? artworkPath = await _player.ResolveMediaControlArtworkAsync(path, cancellationToken);
            if (artworkPath is null)
                return;
            RandomAccessStreamReference? artwork = await CreateThumbnailAsync(artworkPath, cancellationToken);
            cancellationToken.ThrowIfCancellationRequested();
            _dispatcher.TryEnqueue(() =>
            {
                if (_disposed || generation != _metadataGeneration || _player.CurrentMediaPath != path || !_player.Vm.HasMedia)
                    return;
                try
                {
                    _controls.DisplayUpdater.Thumbnail = artwork;
                    _controls.DisplayUpdater.Update();
                }
                catch (Exception ex)
                {
                    Log.Warn("SMTC artwork update failed: " + ex.Message);
                }
            });
        }
        catch (OperationCanceledException)
        {
            // A newer file/title/tag update superseded this async projection.
        }
        catch (Exception ex)
        {
            Log.Warn("SMTC metadata resolution failed: " + ex.Message);
        }
    }

    private static async Task<RandomAccessStreamReference?> CreateThumbnailAsync(
        string? artworkPath,
        CancellationToken cancellationToken)
    {
        string fallback = Path.Combine(AppContext.BaseDirectory, "Assets", "OkPlayer.ico");
        foreach (string path in new[] { artworkPath ?? string.Empty, fallback })
        {
            cancellationToken.ThrowIfCancellationRequested();
            if (string.IsNullOrEmpty(path) || !File.Exists(path))
                continue;
            try
            {
                StorageFile file = await StorageFile.GetFileFromPathAsync(path);
                return RandomAccessStreamReference.CreateFromFile(file);
            }
            catch
            {
                // Try the app icon when media artwork is unreadable; otherwise leave the thumbnail empty.
            }
        }
        return null;
    }

    private void ClearMetadata()
    {
        try
        {
            _controls.DisplayUpdater.ClearAll();
            _controls.DisplayUpdater.Update();
        }
        catch { }
    }

    public void Dispose()
    {
        if (_disposed)
            return;
        _disposed = true;
        _metadataCancellation?.Cancel();
        _metadataCancellation?.Dispose();
        _metadataCancellation = null;

        _controls.ButtonPressed -= OnButtonPressed;
        _controls.PlaybackPositionChangeRequested -= OnPlaybackPositionChangeRequested;
        _controls.PlaybackRateChangeRequested -= OnPlaybackRateChangeRequested;
        _player.Vm.PropertyChanged -= OnPlayerPropertyChanged;
        _player.MediaControlPlaylistChanged -= OnPlaylistChanged;
        _player.MediaControlMetadataChanged -= OnMetadataChanged;
        try
        {
            _controls.IsEnabled = false;
            _controls.PlaybackStatus = MediaPlaybackStatus.Closed;
            _controls.DisplayUpdater.ClearAll();
            _controls.DisplayUpdater.Update();
        }
        catch { }
    }
}
