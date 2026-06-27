using System;
using System.IO;
using System.IO.Compression;

namespace OkPlayer.IntegrationTests;

/// <summary>Minimal PNG decoder used only by the subtitle render tests: it inflates the image and exposes
/// per-pixel luma so a test can locate the brightest rows (the burned-in subtitle text) without pulling in
/// System.Drawing or any other image package. Handles the 8-bit, non-interlaced grayscale/RGB(A) outputs
/// that mpv's image VO produces; anything else throws so an unexpected format surfaces loudly.</summary>
internal sealed class PngLuma
{
    public int Width { get; }
    public int Height { get; }
    private readonly byte[] _luma; // one byte (0..255) per pixel, row-major

    private PngLuma(int w, int h, byte[] luma) { Width = w; Height = h; _luma = luma; }

    public byte At(int x, int y) => _luma[y * Width + x];

    public static PngLuma Load(string path)
    {
        byte[] bytes = File.ReadAllBytes(path);
        // 8-byte PNG signature.
        if (bytes.Length < 8 || bytes[0] != 0x89 || bytes[1] != 0x50 || bytes[2] != 0x4E || bytes[3] != 0x47)
            throw new InvalidDataException($"Not a PNG: {path}");

        int pos = 8;
        int width = 0, height = 0, bitDepth = 0, colorType = 0, interlace = 0;
        using var idat = new MemoryStream();
        while (pos + 8 <= bytes.Length)
        {
            int len = ReadBE32(bytes, pos); pos += 4;
            string type = System.Text.Encoding.ASCII.GetString(bytes, pos, 4); pos += 4;
            if (type == "IHDR")
            {
                width = ReadBE32(bytes, pos);
                height = ReadBE32(bytes, pos + 4);
                bitDepth = bytes[pos + 8];
                colorType = bytes[pos + 9];
                interlace = bytes[pos + 12];
            }
            else if (type == "IDAT")
            {
                idat.Write(bytes, pos, len);
            }
            pos += len + 4; // skip data + CRC
            if (type == "IEND") break;
        }

        if (bitDepth != 8) throw new NotSupportedException($"PNG bit depth {bitDepth} not supported");
        if (interlace != 0) throw new NotSupportedException("Interlaced PNG not supported");
        int channels = colorType switch
        {
            0 => 1, // grayscale
            2 => 3, // RGB
            4 => 2, // grayscale + alpha
            6 => 4, // RGBA
            _ => throw new NotSupportedException($"PNG color type {colorType} not supported")
        };

        byte[] raw = Inflate(idat.ToArray());
        int bpp = channels;                  // bytes per pixel (8-bit)
        int stride = width * bpp;
        byte[] prev = new byte[stride];
        byte[] cur = new byte[stride];
        byte[] luma = new byte[width * height];

        int p = 0;
        for (int y = 0; y < height; y++)
        {
            byte filter = raw[p++];
            Buffer.BlockCopy(raw, p, cur, 0, stride);
            p += stride;
            Unfilter(filter, cur, prev, bpp);
            for (int x = 0; x < width; x++)
            {
                int o = x * bpp;
                byte l = channels switch
                {
                    1 or 2 => cur[o],                                                // gray (alpha ignored)
                    _ => (byte)((cur[o] * 299 + cur[o + 1] * 587 + cur[o + 2] * 114) / 1000) // Rec.601 luma
                };
                luma[y * width + x] = l;
            }
            (prev, cur) = (cur, prev);
        }
        return new PngLuma(width, height, luma);
    }

    private static void Unfilter(byte filter, byte[] cur, byte[] prev, int bpp)
    {
        int stride = cur.Length;
        switch (filter)
        {
            case 0: break; // None
            case 1: // Sub
                for (int i = bpp; i < stride; i++) cur[i] = (byte)(cur[i] + cur[i - bpp]);
                break;
            case 2: // Up
                for (int i = 0; i < stride; i++) cur[i] = (byte)(cur[i] + prev[i]);
                break;
            case 3: // Average
                for (int i = 0; i < stride; i++)
                {
                    int a = i >= bpp ? cur[i - bpp] : 0;
                    cur[i] = (byte)(cur[i] + ((a + prev[i]) >> 1));
                }
                break;
            case 4: // Paeth
                for (int i = 0; i < stride; i++)
                {
                    int a = i >= bpp ? cur[i - bpp] : 0;
                    int b = prev[i];
                    int c = i >= bpp ? prev[i - bpp] : 0;
                    cur[i] = (byte)(cur[i] + Paeth(a, b, c));
                }
                break;
            default: throw new InvalidDataException($"Unknown PNG filter {filter}");
        }
    }

    private static int Paeth(int a, int b, int c)
    {
        int p = a + b - c;
        int pa = Math.Abs(p - a), pb = Math.Abs(p - b), pc = Math.Abs(p - c);
        if (pa <= pb && pa <= pc) return a;
        return pb <= pc ? b : c;
    }

    // PNG wraps the deflate stream in a 2-byte zlib header (+ trailing adler32). DeflateStream wants raw
    // deflate, so skip the two header bytes.
    private static byte[] Inflate(byte[] zlib)
    {
        using var input = new MemoryStream(zlib, 2, zlib.Length - 2);
        using var deflate = new DeflateStream(input, CompressionMode.Decompress);
        using var output = new MemoryStream();
        deflate.CopyTo(output);
        return output.ToArray();
    }

    private static int ReadBE32(byte[] b, int o) => (b[o] << 24) | (b[o + 1] << 16) | (b[o + 2] << 8) | b[o + 3];
}
