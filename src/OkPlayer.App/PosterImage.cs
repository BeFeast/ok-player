using System;
using Microsoft.UI.Xaml.Media.Imaging;

namespace OkPlayer.App;

/// <summary>Loads a cached poster as a <see cref="BitmapImage"/> that bypasses the process-wide URI image
/// cache. WinUI caches decoded images by URI, so regenerating a poster at the same path (e.g. replacing a
/// black frame with a lit one) would otherwise keep showing the stale decode. <c>IgnoreImageCache</c>
/// forces a fresh decode every time, which is fine for the handful of small poster thumbnails.</summary>
internal static class PosterImage
{
    public static BitmapImage Load(string path, int decodePixelWidth = 0)
    {
        var bmp = new BitmapImage { CreateOptions = BitmapCreateOptions.IgnoreImageCache };
        if (decodePixelWidth > 0)
            bmp.DecodePixelWidth = decodePixelWidth; // must be set before UriSource
        bmp.UriSource = new Uri(path);
        return bmp;
    }
}
