# Claude Design Prompt — OK Player

You are designing **OK Player**, a Windows-native media player. Produce a cohesive **visual design system** and the **key MVP screens** (with their states). This document orients you; the **source of truth is [`OK-Player-PRD.md`](./OK-Player-PRD.md)** in this folder — read it.

## What it is

The most *elegant* media player on Windows — **macOS-utility-grade polish delivered in authentic Windows 11 Fluent + Mica**. A **pure player** (no library). Built on **WinUI 3 / C#** over the **mpv** engine (libmpv), the way IINA is on macOS.

## The four pillars (north star — in rank order; lower number wins on conflict)

1. **Most elegant design.**
2. **Best subtitle UX.**
3. **Beautiful chapters with thumbnail previews** (not bare timestamps).
4. **Convenient screenshots + precise frame/second navigation.**

## Aesthetic direction

- **Feel target:** Elmedia Player, IINA, Paste, CleanMyMac, Bartender — calm, deliberate, delightful — but expressed in **native Fluent/Mica**, *not* macOS chrome. Do not copy macOS window controls; make a Windows app feel that refined.
- **Single video plane with floating, auto-hiding chrome** — not a chrome-framed video box. Chrome = translucent Mica/acrylic layers *over* the video that fade away during playback.
- **Theme:** Light + Auto (dark arrives via system). **Mica subtle, not glassy.** One accent (system accent by default).
- **Motion:** mac-grade — smooth, eased show/hide of OSC and panels (animate **opacity *and* position**, never a hard toggle); must hold frame cadence during 4K hardware-decoded playback.
- **Restraint:** the primary surface shows only core controls; everything advanced lives in tucked-away menus. No badges, counters, telemetry nags, or decorative chrome.

## ⚠️ Solve this first — it constrains the entire overlay system

Define the **floating-control material recipe** (scrim + Acrylic) that keeps the **OSC, popovers, and toasts legible over both very-bright (snow) and very-dark (near-black) frames**. Every floating element depends on it, so lock it before laying out controls. *(PRD §16.2 / Open Question #4.)*

## Deliverables

### 1. Design system
Color (light + dark), type ramp, spacing grid, corner radii, elevation/material (Mica usage **per surface** + the floating-control recipe above), iconography direction, and motion specs (durations + easing curves).

### 2. MVP screens — each with the states defined in PRD §14
- **Main player surface** — idle (playing) / active / paused / fullscreen / loading / error / no-subs / no-chapters
- **Empty / "Continue watching"** surface
- **OSC** (on-screen controller) — with chapter markers, A–B brackets, and the **seek hover-thumbnail**
- **Chapter panel** — thumbnail + title + time; Chapters vs Bookmarks sections; metadata / interval-markers / scene-detected / generating / empty states
- **Subtitle quick-switcher** (incl. a permanent **"Generate subtitles…"** Scribe entry) + **Audio quick-switcher**
- **Playlist / queue panel**
- **Settings shell** — **8 panels**: Appearance, Playback, Subtitles, Video, Audio, Shortcuts, Integration, Advanced
- **Mini-player / PiP** (always-on-top, hover-reveals minimal controls)
- **Compact "music" mode** (artwork + transport; a **static placeholder** when no artwork — not a waveform)
- **OSD toasts** — volume / speed / subtitle-delay / on-seek **timecode + frame number** — one shared style
- **Context menu**

### 3. Shared components (design once, reuse everywhere)
- **Frame-preview** engine → seekbar hover, chapter-list thumbnails, timeline markers, bookmark thumbnails
- **Transient toast/indicator** → subtitle-offset, on-seek readout, screenshot confirmation
- Consistent **list rows** (chapter / bookmark / playlist / track)

## Constraints & non-goals

Windows 11 only. No media library here (separate companion app). No DVD / IPTV / casting / transcoding / video editing. No upscaling-shader UI. Keep it focused and quiet.

## Suggested order

1. Design system + the **floating-control material study**.
2. **Main player** surface (all states) and the **Empty / Continue-watching** state.
3. OSC + seek thumbnail, then Chapter panel, then the switchers, playlist, settings, and window modes.
