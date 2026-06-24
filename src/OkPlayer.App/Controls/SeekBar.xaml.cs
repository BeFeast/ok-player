using System;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;

namespace OkPlayer.App.Controls;

/// <summary>
/// The over-video seek bar: track + accent played-fill + thumb, with click/drag-to-seek.
/// Binds <see cref="Fraction"/> (0..1) to the playhead and raises <see cref="SeekRequested"/> live.
/// </summary>
public sealed partial class SeekBar : UserControl
{
    private bool _dragging;

    public SeekBar()
    {
        InitializeComponent();
        SizeChanged += (_, _) => UpdateVisual();
    }

    public static readonly DependencyProperty FractionProperty = DependencyProperty.Register(
        nameof(Fraction), typeof(double), typeof(SeekBar),
        new PropertyMetadata(0.0, (d, _) => ((SeekBar)d).UpdateVisual()));

    /// <summary>Playhead position, 0..1.</summary>
    public double Fraction
    {
        get => (double)GetValue(FractionProperty);
        set => SetValue(FractionProperty, value);
    }

    /// <summary>Raised with the target fraction on press, drag, and release.</summary>
    public event Action<double>? SeekRequested;

    /// <summary>Raised true when scrubbing starts, false when it ends.</summary>
    public event Action<bool>? ScrubStateChanged;

    private void UpdateVisual()
    {
        double width = ActualWidth;
        if (width <= 0)
            return;
        double f = Math.Clamp(Fraction, 0, 1);
        FillBar.Width = f * width;
        ThumbDot.Margin = new Thickness(f * width - ThumbDot.Width / 2, 0, 0, 0);
    }

    private double FractionFromPointer(PointerRoutedEventArgs e)
    {
        double x = e.GetCurrentPoint(this).Position.X;
        return ActualWidth > 0 ? Math.Clamp(x / ActualWidth, 0, 1) : 0;
    }

    private void OnPointerPressed(object sender, PointerRoutedEventArgs e)
    {
        _dragging = true;
        Root.CapturePointer(e.Pointer);
        ScrubStateChanged?.Invoke(true);
        double f = FractionFromPointer(e);
        Fraction = f;
        SeekRequested?.Invoke(f);
        e.Handled = true;
    }

    private void OnPointerMoved(object sender, PointerRoutedEventArgs e)
    {
        if (!_dragging)
            return;
        double f = FractionFromPointer(e);
        Fraction = f;
        SeekRequested?.Invoke(f);
    }

    private void OnPointerReleased(object sender, PointerRoutedEventArgs e)
    {
        if (!_dragging)
            return;
        double f = FractionFromPointer(e);
        Fraction = f;
        SeekRequested?.Invoke(f);
        EndDrag(e.Pointer);
    }

    private void OnPointerCanceled(object sender, PointerRoutedEventArgs e) => EndDrag(e.Pointer);

    private void OnPointerCaptureLost(object sender, PointerRoutedEventArgs e)
    {
        if (_dragging)
        {
            _dragging = false;
            ScrubStateChanged?.Invoke(false);
        }
    }

    private void EndDrag(Pointer pointer)
    {
        if (!_dragging)
            return;
        _dragging = false;
        Root.ReleasePointerCapture(pointer);
        ScrubStateChanged?.Invoke(false);
    }
}
