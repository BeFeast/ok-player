using System;
using System.Globalization;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Media.Animation;
using OkPlayer.App.ViewModels;
using Windows.System;

namespace OkPlayer.App.Views;

/// <summary>The OSC volume control (design "Volume" v3): a resting speaker icon + a 4px level wick that
/// expands, on hover/focus, into a floating Acrylic capsule with the full slider, 100% boost marker and a
/// type-to-set readout. Talks to the <see cref="PlayerViewModel"/> (0–130%, mute remembers the level).</summary>
public sealed partial class VolumeControl : UserControl
{
    private const double Max = 130, Unity = 100, TrackW = 96, ThumbW = 14, WickW = 18;
    private static readonly double MarkerFrac = Unity / Max; // 100% sits at 76.9% of the track

    private PlayerViewModel? _vm;
    private bool _dragging;
    private bool _editing;
    private readonly Microsoft.UI.Dispatching.DispatcherQueueTimer _graceTimer;

    public VolumeControl()
    {
        InitializeComponent();
        _graceTimer = DispatcherQueue.CreateTimer();
        _graceTimer.Interval = TimeSpan.FromMilliseconds(220); // hover-out grace
        _graceTimer.IsRepeating = false;
        _graceTimer.Tick += (_, _) => Collapse();
        CapsuleBorder.SizeChanged += (_, _) => PositionCapsule();
        PointerWheelChanged += OnWheel;
    }

    /// <summary>The player VM this control reads/writes. Set by the host.</summary>
    public PlayerViewModel? Vm
    {
        get => _vm;
        set
        {
            if (_vm is not null) _vm.PropertyChanged -= OnVmChanged;
            _vm = value;
            if (_vm is not null) _vm.PropertyChanged += OnVmChanged;
            UpdateVisuals();
        }
    }

    private void OnVmChanged(object? sender, System.ComponentModel.PropertyChangedEventArgs e)
    {
        if (e.PropertyName is nameof(PlayerViewModel.Volume) or nameof(PlayerViewModel.IsMuted))
            UpdateVisuals();
    }

    private double Vol => _vm?.Volume ?? 100;
    private bool Muted => _vm?.IsMuted ?? false;
    private bool Boost => Vol > Unity && !Muted;

    private void UpdateVisuals()
    {
        double v = Math.Clamp(Vol, 0, Max);
        double frac = v / Max;

        Wick.Width = Muted ? 0 : WickW * frac;
        Wick.Background = SolidBrush(Boost ? 0xFFF0B840 : Muted ? 0x6628B3AA : 0xFF28B3AA);

        SpeakerIcon.Glyph = (Muted || v < 1) ? "" : v < 50 ? "" : "";
        SpeakerIcon.Foreground = SolidBrush(Boost ? 0xFFF0B840 : Muted ? 0x73FFFFFF : 0xFFFFFFFF);

        double markerX = TrackW * MarkerFrac;
        double thumbX = TrackW * frac;
        TealFill.Width = TrackW * Math.Min(v, Unity) / Max;
        TealFill.Opacity = Muted ? 0.4 : 1;
        if (v > Unity && !Muted)
        {
            AmberFill.Visibility = Visibility.Visible;
            Canvas.SetLeft(AmberFill, markerX);
            AmberFill.Width = Math.Max(0, thumbX - markerX);
        }
        else
        {
            AmberFill.Visibility = Visibility.Collapsed;
        }
        Canvas.SetLeft(Marker, markerX - 1);
        Canvas.SetLeft(Thumb, thumbX - ThumbW / 2);
        Thumb.Opacity = Muted ? 0.5 : 1;

        Readout.Text = Muted ? "Muted" : $"{v:0}%";
        Readout.Foreground = SolidBrush(Boost ? 0xFFF0B840 : Muted ? 0x8CFFFFFF : 0xFFFFFFFF);
    }

    private static SolidColorBrush SolidBrush(uint argb)
        => new(Windows.UI.Color.FromArgb((byte)(argb >> 24), (byte)(argb >> 16), (byte)(argb >> 8), (byte)argb));

    // ── hover expand / collapse ──

    private void OnRootEnter(object sender, PointerRoutedEventArgs e) => Expand();
    private void OnCapsuleEnter(object sender, PointerRoutedEventArgs e) => _graceTimer.Stop();
    private void OnRootExit(object sender, PointerRoutedEventArgs e)
    {
        if (!_dragging && !_editing)
            _graceTimer.Start();
    }

    private void Expand()
    {
        _graceTimer.Stop();
        if (Capsule.IsOpen)
            return;
        Capsule.IsOpen = true;
        PositionCapsule();
        Animate(0, 1, 6, 0, 0.96, 1, 150);
    }

    private void Collapse()
    {
        if (!Capsule.IsOpen || _dragging || _editing)
            return;
        var sb = Animate(1, 0, 0, 6, 1, 0.96, 120);
        sb.Completed += (_, _) => { if (!_dragging && !_editing) Capsule.IsOpen = false; };
    }

    private Storyboard Animate(double o0, double o1, double y0, double y1, double s0, double s1, int ms)
    {
        var dur = new Duration(TimeSpan.FromMilliseconds(ms));
        var ease = new CubicEase { EasingMode = EasingMode.EaseOut };
        var sb = new Storyboard();
        void Add(DependencyObject target, string prop, double from, double to)
        {
            var a = new DoubleAnimation { From = from, To = to, Duration = dur, EasingFunction = ease, EnableDependentAnimation = true };
            Storyboard.SetTarget(a, target);
            Storyboard.SetTargetProperty(a, prop);
            sb.Children.Add(a);
        }
        Add(CapsuleBorder, "Opacity", o0, o1);
        Add(CapsuleXform, "TranslateY", y0, y1);
        Add(CapsuleXform, "ScaleX", s0, s1);
        Add(CapsuleXform, "ScaleY", s0, s1);
        sb.Begin();
        return sb;
    }

    private void PositionCapsule()
    {
        if (CapsuleBorder.ActualWidth <= 0)
            return;
        Capsule.HorizontalOffset = (Root.ActualWidth - CapsuleBorder.ActualWidth) / 2;
        Capsule.VerticalOffset = -(CapsuleBorder.ActualHeight + 10);
    }

    // ── drag / click the track ──

    private void OnTrackPressed(object sender, PointerRoutedEventArgs e)
    {
        _dragging = true;
        Track.CapturePointer(e.Pointer);
        SetFromX(e.GetCurrentPoint(Track).Position.X);
    }

    private void OnTrackMoved(object sender, PointerRoutedEventArgs e)
    {
        if (_dragging)
            SetFromX(e.GetCurrentPoint(Track).Position.X);
    }

    private void OnTrackReleased(object sender, PointerRoutedEventArgs e)
    {
        _dragging = false;
        Track.ReleasePointerCapture(e.Pointer);
    }

    private void SetFromX(double x) => _vm?.SetVolume(Math.Clamp(x / TrackW, 0, 1) * Max);

    // ── scroll (Shift = fine) ──

    private void OnWheel(object sender, PointerRoutedEventArgs e)
    {
        int d = e.GetCurrentPoint(this).Properties.MouseWheelDelta;
        if (d == 0)
            return;
        bool fine = Microsoft.UI.Input.InputKeyboardSource.GetKeyStateForCurrentThread(VirtualKey.Shift)
            .HasFlag(Windows.UI.Core.CoreVirtualKeyStates.Down);
        _vm?.NudgeVolume((d > 0 ? 1 : -1) * (fine ? 0.1 : 1));
        e.Handled = true;
    }

    // ── mute ──

    private void OnMuteClick(object sender, RoutedEventArgs e) => _vm?.ToggleMute();

    // ── type-to-set ──

    private void OnReadoutTapped(object sender, TappedRoutedEventArgs e)
    {
        _editing = true;
        ReadoutInput.Text = ((int)Math.Round(Vol)).ToString(CultureInfo.InvariantCulture);
        ReadoutInput.Visibility = Visibility.Visible;
        Readout.Visibility = Visibility.Collapsed;
        ReadoutInput.SelectAll();
        ReadoutInput.Focus(FocusState.Programmatic);
    }

    private void OnReadoutKey(object sender, KeyRoutedEventArgs e)
    {
        if (e.Key != VirtualKey.Enter)
            return;
        ApplyTyped();
        e.Handled = true;
    }

    private void OnReadoutLostFocus(object sender, RoutedEventArgs e) => ApplyTyped();

    private void ApplyTyped()
    {
        if (!_editing)
            return;
        if (double.TryParse(ReadoutInput.Text, NumberStyles.Any, CultureInfo.InvariantCulture, out double v))
            _vm?.SetVolume(Math.Clamp(v, 0, Max));
        ReadoutInput.Visibility = Visibility.Collapsed;
        Readout.Visibility = Visibility.Visible;
        _editing = false;
    }
}
