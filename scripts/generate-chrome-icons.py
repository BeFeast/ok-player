#!/usr/bin/env python3
"""Generate the OK Player symbolic chrome icon set.

One 16x16 grid, fills only (GTK's symbolic recolour rewrites `fill`, never
`stroke`), 2px primary weight so a 1.5x scale lands on whole pixels.
"""
import math
import os
import sys

OUT = sys.argv[1] if len(sys.argv) > 1 else "icons"
os.makedirs(OUT, exist_ok=True)

FILL = "#222222"


def fmt(v):
    r = round(v, 3)
    if r == int(r):
        return str(int(r))
    return ("%.3f" % r).rstrip("0")


def pt(x, y):
    return "%s %s" % (fmt(x), fmt(y))


def circle(cx, cy, r):
    return '<circle cx="%s" cy="%s" r="%s"/>' % (fmt(cx), fmt(cy), fmt(r))


def rect(x, y, w, h, rx=None):
    extra = ' rx="%s"' % fmt(rx) if rx else ""
    return '<rect x="%s" y="%s" width="%s" height="%s"%s/>' % (
        fmt(x), fmt(y), fmt(w), fmt(h), extra)


def ring(cx, cy, r, w):
    """Annulus as one even-odd path: no stroke, so GTK recolours it."""
    ri = r - w
    d = ("M %s a %s %s 0 1 0 %s 0 a %s %s 0 1 0 %s 0 Z "
         "M %s a %s %s 0 1 1 %s 0 a %s %s 0 1 1 %s 0 Z") % (
        pt(cx - r, cy), fmt(r), fmt(r), fmt(2 * r), fmt(r), fmt(r), fmt(-2 * r),
        pt(cx - ri, cy), fmt(ri), fmt(ri), fmt(2 * ri), fmt(ri), fmt(ri), fmt(-2 * ri))
    return '<path fill-rule="evenodd" d="%s"/>' % d


def arc_band(cx, cy, r, w, a0, a1):
    """Band between radius r and r-w, from angle a0 to a1 (degrees, 0 = east)."""
    ri = r - w
    t0, t1 = math.radians(a0), math.radians(a1)
    large = 1 if abs(a1 - a0) > 180 else 0
    ox0, oy0 = cx + r * math.cos(t0), cy + r * math.sin(t0)
    ox1, oy1 = cx + r * math.cos(t1), cy + r * math.sin(t1)
    ix1, iy1 = cx + ri * math.cos(t1), cy + ri * math.sin(t1)
    ix0, iy0 = cx + ri * math.cos(t0), cy + ri * math.sin(t0)
    d = "M %s A %s %s 0 %d 1 %s L %s A %s %s 0 %d 0 %s Z" % (
        pt(ox0, oy0), fmt(r), fmt(r), large, pt(ox1, oy1),
        pt(ix1, iy1), fmt(ri), fmt(ri), large, pt(ix0, iy0))
    return '<path d="%s"/>' % d


def poly(points):
    return '<path d="M %s Z"/>' % " L ".join(pt(x, y) for x, y in points)


def bar(x0, y0, x1, y1, w):
    """A rectangle of width w whose spine runs from (x0,y0) to (x1,y1)."""
    dx, dy = x1 - x0, y1 - y0
    length = math.hypot(dx, dy)
    nx, ny = -dy / length * w / 2, dx / length * w / 2
    return poly([(x0 + nx, y0 + ny), (x1 + nx, y1 + ny),
                 (x1 - nx, y1 - ny), (x0 - nx, y0 - ny)])


def chevron(direction):
    """A 2px chevron. Drawn as two bars so the joint is a clean miter."""
    if direction == "next":
        a, b, c = (6, 3), (11, 8), (6, 13)
    elif direction == "previous":
        a, b, c = (10, 3), (5, 8), (10, 13)
    elif direction == "down":
        a, b, c = (3, 6), (8, 11), (13, 6)
    else:
        a, b, c = (3, 10), (8, 5), (13, 10)
    return (bar(a[0], a[1], b[0], b[1], 2) + bar(b[0], b[1], c[0], c[1], 2)
            + circle(b[0], b[1], 1) + circle(a[0], a[1], 1) + circle(c[0], c[1], 1))


def speaker():
    return poly([(2, 6), (5.5, 6), (9, 2.5), (9, 13.5), (5.5, 10), (2, 10)])


def frame(x, y, w, h, t=2, rx=2):
    """A rounded-rect outline of thickness t, as an even-odd two-rect path."""
    ri = max(rx - t, 0)
    return ('<path fill-rule="evenodd" d="'
            'M %s h %s a %s %s 0 0 1 %s %s v %s a %s %s 0 0 1 %s %s h %s '
            'a %s %s 0 0 1 %s %s v %s a %s %s 0 0 1 %s %s Z '
            'M %s h %s a %s %s 0 0 0 %s %s v %s a %s %s 0 0 0 %s %s h %s '
            'a %s %s 0 0 0 %s %s v %s a %s %s 0 0 0 %s %s Z"/>') % (
        pt(x + rx, y), fmt(w - 2 * rx), fmt(rx), fmt(rx), fmt(rx), fmt(rx),
        fmt(h - 2 * rx), fmt(rx), fmt(rx), fmt(-rx), fmt(rx),
        fmt(-(w - 2 * rx)), fmt(rx), fmt(rx), fmt(-rx), fmt(-rx),
        fmt(-(h - 2 * rx)), fmt(rx), fmt(rx), fmt(rx), fmt(-rx),
        pt(x + t + ri, y + t), fmt(w - 2 * t - 2 * ri) if ri else fmt(w - 2 * t),
        fmt(ri), fmt(ri), fmt(ri), fmt(ri),
        fmt(h - 2 * t - 2 * ri), fmt(ri), fmt(ri), fmt(-ri), fmt(ri),
        fmt(-(w - 2 * t - 2 * ri)) if ri else fmt(-(w - 2 * t)),
        fmt(ri), fmt(ri), fmt(-ri), fmt(-ri),
        fmt(-(h - 2 * t - 2 * ri)), fmt(ri), fmt(ri), fmt(ri), fmt(-ri))


def cog(teeth=8, r_out=7, r_in=5.2, hole=2):
    parts = []
    step = 360.0 / teeth
    for i in range(teeth):
        a = math.radians(i * step)
        parts.append(bar(8 + r_in * math.cos(a) * 0.6, 8 + r_in * math.sin(a) * 0.6,
                         8 + r_out * math.cos(a), 8 + r_out * math.sin(a), 3))
    parts.append(ring(8, 8, r_in, r_in - hole))
    return "".join(parts)


def cross(w=2, inset=3.5):
    a, b = inset, 16 - inset
    return (bar(a, a, b, b, w) + bar(a, b, b, a, w)
            + circle(a, a, w / 2) + circle(b, b, w / 2)
            + circle(a, b, w / 2) + circle(b, a, w / 2))


def bracket(cx, cy, dx, dy, length=4, w=2):
    """An L pointing away from (dx,dy)."""
    return (bar(cx, cy, cx + dx * length, cy, w) + bar(cx, cy, cx, cy + dy * length, w)
            + circle(cx, cy, w / 2)
            + circle(cx + dx * length, cy, w / 2) + circle(cx, cy + dy * length, w / 2))


def magnifier():
    # Ring on whole-pixel bounds (2..11) with a handle heavy enough to read as a
    # handle and not as a stub: this is the shape the report was about.
    return (ring(6.5, 6.5, 4.5, 2)
            + bar(9.9, 9.9, 14, 14, 2.6)
            + circle(14, 14, 1.3))


def note():
    return (circle(5, 12, 2.5) + rect(6.5, 3, 2, 9.2)
            + poly([(6.5, 2), (13.5, 3.6), (13.5, 6), (6.5, 4.4)]))


def dots(count=3, r=1.4, y=8):
    xs = {3: (3.6, 8, 12.4), 2: (5, 11)}[count]
    return "".join(circle(x, y, r) for x in xs)


def plus():
    return (rect(7, 2.5, 2, 11, 1) + rect(2.5, 7, 11, 2, 1))


def minus():
    return rect(2.5, 7, 11, 2, 1)


def check():
    return (bar(2.8, 8.6, 6.4, 12.2, 2.2) + bar(6.4, 12.2, 13.2, 4.4, 2.2)
            + circle(2.8, 8.6, 1.1) + circle(6.4, 12.2, 1.1) + circle(13.2, 4.4, 1.1))


def triangle_right(x=4, y=2.5, w=8.5, h=11):
    return poly([(x, y), (x + w, y + h / 2), (x, y + h)])


def clock():
    return (ring(8, 8, 6.5, 1.8) + rect(7.3, 4.2, 1.4, 4.3, 0.7)
            + rect(7.3, 7.1, 4.3, 1.4, 0.7))


def folder():
    """Tab strip and front face, parted by a seam so the silhouette reads as a
    folder rather than as a blob."""
    return ('<path d="M 1.7 4.1 a 1.4 1.4 0 0 1 1.4 -1.4 h 3.2 l 1.7 1.9 h 5.3 '
            'a 1.4 1.4 0 0 1 1.4 1.4 v 0.8 h -13 Z"/>'
            '<path d="M 1.7 7.4 h 13 v 4.9 a 1.4 1.4 0 0 1 -1.4 1.4 h -10.2 '
            'a 1.4 1.4 0 0 1 -1.4 -1.4 Z"/>')


def plane():
    return (poly([(14, 2), (2, 7.4), (6.2, 9.2), (14, 2)])
            + poly([(14, 2), (7.2, 10.2), (9.2, 14), (14, 2)]))


def bookmark():
    return poly([(4, 2), (12, 2), (12, 14), (8, 10.5), (4, 14)])


def pin():
    """A thumbtack seen from the side: head, plate, needle."""
    return (circle(8, 3.9, 2.9) + rect(5.9, 6.4, 4.2, 1.5, 0.6)
            + poly([(7.2, 7.9), (8.8, 7.9), (8, 14.8)]))


def film():
    return (frame(2, 3.5, 12, 9, 2, 2)
            + rect(4.2, 5.4, 1.6, 1.6, 0.4) + rect(4.2, 9, 1.6, 1.6, 0.4)
            + rect(10.2, 5.4, 1.6, 1.6, 0.4) + rect(10.2, 9, 1.6, 1.6, 0.4))


def picture():
    return (frame(2, 3, 12, 10, 2, 2)
            + circle(6, 6.4, 1.3)
            + poly([(4, 11), (7.2, 7.4), (9.4, 10), (11, 8.2), (12, 11)]))


def subtitles():
    return (frame(1.5, 2.5, 13, 11, 1.8, 2)
            + rect(4, 8, 4.6, 1.3, 0.65) + rect(9.6, 8, 2.4, 1.3, 0.65)
            + rect(4, 10.3, 2.4, 1.3, 0.65) + rect(7.4, 10.3, 4.6, 1.3, 0.65))


def server():
    return (frame(2, 2.8, 12, 4.6, 1.5, 1.2) + frame(2, 8.6, 12, 4.6, 1.5, 1.2)
            + circle(4.5, 5.1, 0.9) + circle(4.5, 10.9, 0.9))


def camera():
    return (rect(5.5, 2.5, 5, 2, 0.8)
            + frame(1.5, 4, 13, 9.5, 2, 2)
            + ring(8, 8.8, 3.2, 2))


def copy_pair():
    return (frame(5, 2, 9, 9, 2, 2)
            + '<path d="M 2 5.5 h 2 v 8 h 8 v 2 h -8 a 2 2 0 0 1 -2 -2 Z"/>')


def link():
    """A diagonal capsule outline broken in the middle: the chain-link glyph."""
    r, w, shaft = 3.0, 1.9, 3.5
    ax, ay, bx, by = 4.9, 11.1, 11.1, 4.9
    ux, uy = 1 / math.sqrt(2), -1 / math.sqrt(2)
    px, py = ux, -uy
    parts = [arc_band(ax, ay, r, w, 45, 225), arc_band(bx, by, r, w, 225, 45)]
    for sign in (1, -1):
        parts.append(bar(ax + sign * r * px, ay + sign * r * py,
                         ax + sign * r * px + shaft * ux,
                         ay + sign * r * py + shaft * uy, w))
        parts.append(bar(bx + sign * r * px, by + sign * r * py,
                         bx + sign * r * px - shaft * ux,
                         by + sign * r * py - shaft * uy, w))
    return "".join(parts)


def drag_handle():
    return "".join(rect(x, y, 2, 2, 0.5)
                   for y in (5, 9) for x in (3, 7, 11))


def list_rows():
    return "".join(circle(3.4, y + 0.9, 1.4) + rect(6.5, y, 7.5, 1.8, 0.9)
                   for y in (3.1, 7.1, 11.1))


def clear_all():
    return (rect(1.6, 3, 12.8, 1.9, 0.95) + rect(1.6, 6.6, 7, 1.9, 0.95)
            + bar(9.1, 9.1, 14.2, 14.2, 1.9) + bar(9.1, 14.2, 14.2, 9.1, 1.9))


def clear_entry():
    """GtkSearchEntry's clear button: a filled disc with the X knocked out."""
    x0, x1 = 5.2, 10.8
    dx, dy = 0.9, 0.9
    return ('<path fill-rule="evenodd" d="M 8 1.5 a 6.5 6.5 0 1 0 0 13 '
            'a 6.5 6.5 0 1 0 0 -13 Z '
            'M %s L %s L %s L %s L %s L %s L %s L %s L %s L %s L %s L %s Z"/>') % (
        pt(x0, x0 + dy), pt(x0 + dx, x0), pt(8, 8 - dx),
        pt(x1 - dx, x0), pt(x1, x0 + dy), pt(8 + dx, 8),
        pt(x1, x1 - dy), pt(x1 - dx, x1), pt(8, 8 + dx),
        pt(x0 + dx, x1), pt(x0, x1 - dy), pt(8 - dx, 8))


def error_badge():
    return ring(8, 8, 6.5, 2) + cross(2, 5.2)


def info_badge():
    return ring(8, 8, 6.5, 2) + circle(8, 4.9, 1.1) + rect(7, 6.6, 2, 4.8, 1)


def warning_badge():
    return ('<path fill-rule="evenodd" d="M 8 1.2 L 15.4 14.8 L 0.6 14.8 Z '
            'M 8 5.4 L 3.4 13.2 L 12.6 13.2 Z"/>'
            + circle(8, 11.9, 1.05) + rect(7.05, 6.9, 1.9, 3.5, 0.95))


def fullscreen():
    return (bracket(3, 3, 1, 1, 3.5) + bracket(13, 3, -1, 1, 3.5)
            + bracket(3, 13, 1, -1, 3.5) + bracket(13, 13, -1, -1, 3.5))


def restore():
    # view-fullscreen turned inside out: the same four brackets, corners facing
    # the centre, so leaving fullscreen reads as the inverse of entering it.
    return (bracket(6, 6, -1, -1, 3) + bracket(10, 6, 1, -1, 3)
            + bracket(6, 10, -1, 1, 3) + bracket(10, 10, 1, 1, 3))


ICONS = {
    "audio-volume-high": speaker() + arc_band(9, 8, 4.6, 2, -55, 55)
                         + arc_band(9, 8, 7, 2, -50, 50),
    "audio-volume-low": speaker() + arc_band(9, 8, 4.6, 2, -55, 55),
    "audio-volume-muted": speaker() + bar(10.5, 5.5, 14.5, 10.5, 2)
                          + bar(10.5, 10.5, 14.5, 5.5, 2),
    "audio-x-generic": note(),
    "camera-photo": camera(),
    "dialog-error": error_badge(),
    "dialog-information": info_badge(),
    "dialog-warning": warning_badge(),
    "document-open-recent": clock(),
    "document-open": folder(),
    "document-send": plane(),
    "edit-clear-all": clear_all(),
    "edit-clear": clear_entry(),
    "edit-copy": copy_pair(),
    "edit-find": magnifier(),
    "emblem-system": cog(),
    "go-down": chevron("down"),
    "go-next": chevron("next"),
    "go-previous": chevron("previous"),
    "go-up": chevron("up"),
    "image-x-generic": picture(),
    "insert-link": link(),
    "list-add": plus(),
    "list-drag-handle": drag_handle(),
    "list-remove": minus(),
    "media-playback-pause": rect(4, 2.5, 3, 11, 1) + rect(9, 2.5, 3, 11, 1),
    "media-playback-start": triangle_right(),
    "media-seek-forward": poly([(1.5, 3), (7.5, 8), (1.5, 13)])
                          + poly([(8, 3), (14, 8), (8, 13)]),
    "media-skip-backward": poly([(13, 3), (5.5, 8), (13, 13)]) + rect(2.5, 3, 2, 10, 1),
    "media-skip-forward": poly([(3, 3), (10.5, 8), (3, 13)]) + rect(11.5, 3, 2, 10, 1),
    "media-view-subtitles": subtitles(),
    "network-server": server(),
    "object-select": check(),
    "pan-down": chevron("down"),
    "pan-end": chevron("next"),
    "pan-start": chevron("previous"),
    "pan-up": chevron("up"),
    "system-search": magnifier(),
    "user-bookmarks": bookmark(),
    "video-x-generic": film(),
    "view-fullscreen": fullscreen(),
    "view-list": list_rows(),
    "view-more": dots(3),
    "view-pin": pin(),
    "view-restore": restore(),
    "window-close": cross(2.2, 3.4),
}

HEADER = ('<?xml version="1.0" encoding="UTF-8"?>\n'
          '<!-- OK Player chrome icon. Original work, GPL-3.0-or-later.\n'
          '     Generated by scripts/generate-chrome-icons.py - edit that, not this. -->\n'
          '<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"'
          ' viewBox="0 0 16 16">\n  <g fill="%s">\n    ' % FILL)
FOOTER = "\n  </g>\n</svg>\n"

for name, body in sorted(ICONS.items()):
    path = os.path.join(OUT, "okp-%s-symbolic.svg" % name)
    with open(path, "w") as handle:
        handle.write(HEADER + body + FOOTER)

print("wrote %d icons to %s" % (len(ICONS), OUT))
