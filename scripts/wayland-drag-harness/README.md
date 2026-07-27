# Wayland drag harness

Headless, human-free reproduction rig for Wayland window-drag defects
(#627: non-OSC drag SIGSEGVs the app on GNOME Wayland). It runs the player
under a real headless compositor, injects genuine pointer input through the
compositor, and captures a core dump + backtrace when the app dies. No X11
anywhere — the historical drag smokes force `GDK_BACKEND=x11` and therefore
cannot see this defect class at all.

## Modes

| Compositor | Input path | Notes |
|---|---|---|
| sway (headless) | `vptr.py` — zwlr_virtual_pointer_v1 | cancels interactive moves immediately (`player-window-move-cancel` right after begin); covers the refused-grab path only |
| mutter --headless | `mptr.py` — org.gnome.Mutter.RemoteDesktop | grants real interactive moves (begin → end, no cancel) — the same grab machinery as a live GNOME session |
| gnome-shell --headless | `mptr.py` | full shell, closest to a user session |

`setscales.py` applies the known crashing operator layout via
org.gnome.Mutter.DisplayConfig: primary 3840x2160 @ scale 2.0 at (0,0) plus
1920x1080 @ ~1.67 at logical (1920,432), with `scale-monitor-framebuffer`
enabled through a keyfile GSettings backend (no dconf pollution).

## Anatomy

Everything lives under a repro root (default `/tmp/okp-drag-repro`) with a
private `XDG_RUNTIME_DIR` (`<root>/xdg`, mode 700) and a private session bus
(`dbus-daemon --session --address=unix:path=<root>/bus --fork`), so nothing
touches the host's real session. Long-lived processes run as transient user
units so they survive SSH disconnects and get `LimitCORE=infinity`:

```sh
# compositor (pick one)
systemd-run --user --unit okp-shell \
  -p Environment=XDG_RUNTIME_DIR=<root>/xdg \
  -p Environment=HOME=<root>/home \
  -p Environment=XDG_CONFIG_HOME=<root>/home/.config \
  -p Environment=GSETTINGS_BACKEND=keyfile \
  -p Environment=DBUS_SESSION_BUS_ADDRESS=unix:path=<root>/bus \
  /usr/bin/gnome-shell --headless --wayland-display okp-wl \
    --virtual-monitor 3840x2160 --virtual-monitor 1920x1080

python3 setscales.py            # apply the operator scale layout

# app under test (packaged build preferred — it carries the bundled libmpv)
systemd-run --user --unit okp-app -p LimitCORE=infinity \
  -p WorkingDirectory=<root> \
  -p Environment=XDG_RUNTIME_DIR=<root>/xdg \
  -p Environment=WAYLAND_DISPLAY=okp-wl \
  -p Environment=GDK_BACKEND=wayland \
  -p Environment=OKP_DEBUG_INTERACTIONS=1 \
  -p Environment=DBUS_SESSION_BUS_ADDRESS=unix:path=<root>/bus \
  /usr/bin/ok-player <root>/long.mkv

./drag-hammer.sh 40             # varied drags (sway mode; use the -mutter sed
                                #  in drag-cross.sh header for mutter mode)
./drag-cross.sh 20              # cross-monitor drags on the scaled layout
```

Success criterion for a repro: unit leaves `active`, journal shows the last
`interaction: player-window-move-begin` with nothing after it, and a `core*`
file appears in the repro root (`gdb -batch -ex bt /usr/bin/ok-player core*`).

Media fixture: any long clip; `ffmpeg -f lavfi -i testsrc2=size=1920x1080:rate=30
-f lavfi -i sine -t 1800 -c:v libx264 -preset ultrafast -crf 30 -c:a aac long.mkv`.
The drag must happen over *playing* video — a held last frame exercises less of
the render path.

## What this rig has already established (2026-07-27, #627)

The packaged candidate `0.11.0-beta.0.187` SIGSEGVs on the operator's live
GNOME Wayland session during a granted non-OSC drag. In this rig the crash did
**not** reproduce in any of these configurations (40-60 granted drags each,
playing video, release-on-second-monitor variants included):

- sway headless, source build, EGL video path (grab always refused — wrong path);
- bare mutter, source build, EGL path, granted moves, single monitor;
- bare mutter, source build, EGL path, dual monitor + operator scales;
- gnome-shell headless (Debian 13 / GNOME 48), source build, EGL path, dual
  monitor + operator scales.

The one configuration this rig could NOT yet run is the packaged build's
`native-wayland-dmabuf` plane (bundled patched libmpv): the candidate `.deb`
does not start on Debian 13 at all (#662, bundled runtime requires
GLIBC_2.43), and on the Ubuntu QA host the GNOME 49 headless shell did not
deliver RemoteDesktop input to the app (open point). That makes the dmabuf
subsurface path the prime suspect for #627: it is exactly what the crashing
host runs and exactly what every surviving configuration lacks.

Order of work: fix #662 → rerun this harness with the packaged `.deb` on the
Debian QA host → expect the SEGV with a core → fix from the backtrace.
