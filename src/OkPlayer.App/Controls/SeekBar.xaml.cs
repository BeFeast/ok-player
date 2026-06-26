using System;
using System.Collections.Generic;
using Microsoft.UI;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media;
using Windows.UI;

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

    public static readonly DependencyProperty BufferedProperty = DependencyProperty.Register(
        nameof(Buffered), typeof(double), typeof(SeekBar),
        new PropertyMetadata(0.0, (d, _) => ((SeekBar)d).UpdateVisual()));

    /// <summary>Buffered/cached extent, 0..1 (lighter band behind the fill).</summary>
    public double Buffered
    {
        get => (double)GetValue(BufferedProperty);
        set => SetValue(BufferedProperty, value);
    }

    public static readonly DependencyProperty ChaptersProperty = DependencyProperty.Register(
        nameof(Chapters), typeof(object), typeof(SeekBar),
        new PropertyMetadata(null, (d, _) => ((SeekBar)d).RenderTicks()));

    /// <summary>Chapter start positions as 0..1 fractions (an IReadOnlyList&lt;double&gt;); rendered as ticks.</summary>
    public object? Chapters
    {
        get => GetValue(ChaptersProperty);
        set => SetValue(ChaptersProperty, value);
    }

    public static readonly DependencyProperty CurrentChapterProperty = DependencyProperty.Register(
        nameof(CurrentChapter), typeof(int), typeof(SeekBar),
        new PropertyMetadata(-1, (d, _) => ((SeekBar)d).RenderTicks()));

    /// <summary>Index of the current chapter (its tick is accent-colored).</summary>
    public int CurrentChapter
    {
        get => (int)GetValue(CurrentChapterProperty);
        set => SetValue(CurrentChapterProperty, value);
    }

    public static readonly DependencyProperty AbLoopAProperty = DependencyProperty.Register(
        nameof(AbLoopA), typeof(double), typeof(SeekBar),
        new PropertyMetadata(double.NaN, (d, _) => ((SeekBar)d).UpdateAbVisual()));

    public static readonly DependencyProperty AbLoopBProperty = DependencyProperty.Register(
        nameof(AbLoopB), typeof(double), typeof(SeekBar),
        new PropertyMetadata(double.NaN, (d, _) => ((SeekBar)d).UpdateAbVisual()));

    /// <summary>A–B loop start as a 0..1 fraction, or NaN when unset.</summary>
    public double AbLoopA
    {
        get => (double)GetValue(AbLoopAProperty);
        set => SetValue(AbLoopAProperty, value);
    }

    /// <summary>A–B loop end as a 0..1 fraction, or NaN when unset.</summary>
    public double AbLoopB
    {
        get => (double)GetValue(AbLoopBProperty);
        set => SetValue(AbLoopBProperty, value);
    }

    /// <summary>Raised with the target fraction on press, drag, and release.</summary>
    public event Action<double>? SeekRequested;

    /// <summary>Raised true when scrubbing starts, false when it ends.</summary>
    public event Action<bool>? ScrubStateChanged;

    /// <summary>Raised as the pointer moves over the bar (hovering or dragging): target fraction + pointer X within the bar.</summary>
    public event Action<double, double>? HoverChanged;

    /// <summary>Raised when the pointer leaves the bar (and isn't dragging).</summary>
    public event Action? HoverEnded;

    private void UpdateVisual()
    {
        double width = ActualWidth;
        if (width <= 0)
            return;
        double f = Math.Clamp(Fraction, 0, 1);
        FillBar.Width = f * width;
        BufferedBar.Width = Math.Clamp(Buffered, 0, 1) * width;
        ThumbDot.Margin = new Thickness(f * width - ThumbDot.Width / 2, 0, 0, 0);
        RenderTicks();
        UpdateAbVisual();
    }

    private void UpdateAbVisual()
    {
        double width = ActualWidth;
        bool aSet = !double.IsNaN(AbLoopA);
        bool bSet = !double.IsNaN(AbLoopB);
        if (width <= 0 || (!aSet && !bSet))
        {
            AbBand.Visibility = Visibility.Collapsed;
            AbMarkerA.Visibility = Visibility.Collapsed;
            AbMarkerB.Visibility = Visibility.Collapsed;
            return;
        }
        // The band marks an ACTIVE loop region, which mpv only engages once BOTH points are set; with just one
        // set, show its marker but not a misleading half-region band.
        if (aSet && bSet)
        {
            double left = Math.Min(AbLoopA, AbLoopB) * width;
            double right = Math.Max(AbLoopA, AbLoopB) * width;
            AbBand.Margin = new Thickness(Math.Clamp(left, 0, width), 0, 0, 0);
            AbBand.Width = Math.Clamp(right - left, 0, width);
            AbBand.Visibility = Visibility.Visible;
        }
        else
        {
            AbBand.Visibility = Visibility.Collapsed;
        }
        SetMarker(AbMarkerA, aSet, AbLoopA, width);
        SetMarker(AbMarkerB, bSet, AbLoopB, width);
    }

    private static void SetMarker(Border marker, bool set, double frac, double width)
    {
        if (!set)
        {
            marker.Visibility = Visibility.Collapsed;
            return;
        }
        marker.Margin = new Thickness(Math.Clamp(frac, 0, 1) * width - marker.Width / 2, 0, 0, 0);
        marker.Visibility = Visibility.Visible;
    }

    private static readonly Color TickColor = Color.FromArgb(0x8C, 0xFF, 0xFF, 0xFF);   // rgba(255,255,255,.55)
    private static readonly Color CurrentTickColor = Color.FromArgb(0xFF, 0x28, 0xB3, 0xAA); // over-video accent

    private void RenderTicks()
    {
        TickCanvas.Children.Clear();
        double width = ActualWidth;
        if (width <= 0 || Chapters is not IReadOnlyList<double> chapters)
            return;
        for (int i = 0; i < chapters.Count; i++)
        {
            double frac = Math.Clamp(chapters[i], 0, 1);
            if (frac <= 0.004 || frac >= 0.996)
                continue; // skip the 0:00 chapter and the very end
            var tick = new Border
            {
                Width = 2,
                Height = 9,
                CornerRadius = new CornerRadius(1),
                Background = new SolidColorBrush(i == CurrentChapter ? CurrentTickColor : TickColor),
            };
            Canvas.SetLeft(tick, frac * width - 1);
            TickCanvas.Children.Add(tick);
        }
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
        double x = e.GetCurrentPoint(this).Position.X;
        double f = ActualWidth > 0 ? Math.Clamp(x / ActualWidth, 0, 1) : 0;
        HoverChanged?.Invoke(f, x); // preview follows the cursor whether hovering or dragging
        if (!_dragging)
            return;
        Fraction = f;
        SeekRequested?.Invoke(f);
    }

    private void OnPointerExited(object sender, PointerRoutedEventArgs e)
    {
        if (!_dragging)
            HoverEnded?.Invoke();
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
