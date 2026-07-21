# OK Player — Product Requirements Document

> **Status:** Implemented and shipping (pre-1.0). This document is the **Windows** product spec — **Platform:** Windows 11 · **Engine:** libmpv (via render API) · **UI:** C#/.NET + WinUI 3 + Mica.
> **Cross-platform note:** OK Player also ships a **native Linux** shell (GTK4/Relm4 over libmpv) sharing the pure-Rust `okp-core`; the Linux shell tracks this same behavior natively rather than porting the UI. See the repository [README](../README.md) and [`docs/linux-release-acceptance.md`](linux-release-acceptance.md).
> **Audience for this doc:** the UI/UX designer (Claude Design) producing screens, plus the engineer prototyping the engine seam.
> **Priority legend:** `[MVP]` ship now · `[Day-2]` reserve the slot now, build next · `[Later]` design must not preclude it; do not build.

---

## Table of Contents

1. [Product Summary & The Four Pillars](#1-product-summary--the-four-pillars)
2. [Problem & Positioning](#2-problem--positioning)
3. [Personas](#3-personas)
4. [Goals & Guiding Principles](#4-goals--guiding-principles)
5. [Feature Priority Table](#5-feature-priority-table)
6. [Pillar 1 — Elegant Design](#6-pillar-1--elegant-design)
7. [Pillar 2 — Subtitles](#7-pillar-2--subtitles)
8. [Pillar 3 — Chapters with Thumbnails](#8-pillar-3--chapters-with-thumbnails)
9. [Pillar 4 — Screenshots & Precise Navigation](#9-pillar-4--screenshots--precise-navigation)
10. [Content, Sources & Playlist](#10-content-sources--playlist)
11. [Playback, Audio & Video Defaults](#11-playback-audio--video-defaults)
12. [Resume, Per-File Memory, History & Privacy](#12-resume-per-file-memory-history--privacy)
13. [Companion-Library Integration & Storage Model](#13-companion-library-integration--storage-model)
14. [Information Architecture — Surfaces & States](#14-information-architecture--surfaces--states)
15. [Interaction Model — Mouse, Keyboard, Window](#15-interaction-model--mouse-keyboard-window)
16. [Design System](#16-design-system)
17. [Windows Integration](#17-windows-integration)
18. [Power Features](#18-power-features)
19. [Non-Functional Requirements](#19-non-functional-requirements)
20. [Non-Goals](#20-non-goals)
21. [Open Questions](#21-open-questions)

---

## 1. Product Summary & The Four Pillars

**OK Player** is a Windows-native media player built for one expert user, designed to be the most refined playback experience on the platform.

> **One-line pitch:** *The most elegant media player on Windows — macOS-utility-grade polish, delivered in native Fluent/Mica.*

It is a **pure player**: it opens a file (or URL) and plays it beautifully, with exceptional subtitle handling, thumbnail-rich chapters, and precise frame-level control. It deliberately does **not** manage a library — a separate companion app owns that role, and the two integrate cleanly.

Technically, OK Player is a GUI wrapper over **libmpv** (the proven architecture of IINA and mpv.net), presented through **C#/.NET + WinUI 3** with a subtle **Mica** backdrop, targeting **Windows 11 only**.

### The Four Pillars (North Star)

These are the ranked differentiators. Every roadmap decision is measured against them; **when priorities collide, lower-numbered pillars win.** If a feature serves neither a pillar nor the everyday playback loop (open → watch → resume), it is a candidate for Non-Goals.

1. **Most elegant design.** Native Fluent/Mica refinement at macOS-utility grade. The reason someone chooses OK Player over anything else.
2. **Best subtitle UX.** Effortless loading, comfortable styling presets, outstanding sync — building toward Scribe-generated subtitles as a flagship.
3. **Beautiful chapters with thumbnail previews.** Chapters shown as **thumbnail + title + time**, not bare timestamps — including auto-generated chapters for files lacking metadata, plus user bookmarks.
4. **Convenient screenshots + precise frame/second navigation.** Clean-and-with-subs capture, plus frame-stepping, timecode jumps, and frame-accurate readouts.

---

## 2. Problem & Positioning

The Windows media-player landscape forces a trade-off OK Player refuses: you can have **power** or **polish**, but not both, and never with taste.

| Reference | What it gets right | Where it falls short for this user |
|---|---|---|
| **mpv.net** | mpv power, scriptable, fast | Spartan, config-file-driven; nothing "designed" |
| **MPC-HC** | Lightweight, classic, reliable | Dated Win32 UI; no modern Fluent feel; aging |
| **IINA** | The *feel* we admire — elegant, native, OSC done right | macOS only; unavailable on Windows |
| **Films & TV** | Native, clean-ish | Anemic feature set; weak format/subtitle support |

**The gap:** no Windows player pairs **IINA-grade elegance** with **mpv-grade capability**, expressed in **native Windows Fluent/Mica** rather than a ported or themed shell.

> **Positioning statement:** For a developer with refined taste who plays movies, series, and YouTube on Windows 11, **OK Player** is a native media player delivering macOS-utility-grade elegance and precise, power-user control — unlike mpv.net (powerful but unstyled), MPC-HC (capable but dated), and Films & TV (native but shallow), and unlike IINA which never comes to Windows.

The aesthetic and interaction north stars are **Elmedia Player, IINA, Paste, CleanMyMac, and Bartender** — apps that feel calm, deliberate, and delightful. OK Player brings that sensibility to Windows, in the platform's own design language rather than imitating macOS chrome.

---

## 3. Personas

### 3.1 Primary — *Oleg, the tasteful power-user (THE user)*

The product is optimized for exactly one person today.

- **Who:** A developer who runs a homelab, watches films/series and YouTube, cares deeply about craft. Comfortable with mpv-class power but unwilling to live in a config file.
- **Mindset:** "I want it to *just work* beautifully by default — and when I need to go deep, the depth is there, tucked away, not in my face."
- **Self-placement on the simple↔power scale: 3/10** — closer to simple. Smart defaults out front; advanced controls hidden but reachable.
- **Frequent actions:** Opens a file from Explorer; nudges subtitle sync; reads comfortable, well-styled subtitles; jumps by chapter (and *wants to see* it, not read a timestamp); grabs clean screenshots; steps frame-by-frame; resumes where he left off.
- **Frustrations:** Ugly chrome, clumsy subtitle workflows, chapter lists that are just numbers, screenshots that capture the OSD, "power" tools that make the simple case ceremonial.
- **Required escape hatch:** inject a raw `mpv.conf` snippet and remap keybindings — present, but out of the everyday path.

> **Design implication:** every default must be correct enough that the Advanced surface is *optional*, not *load-bearing*.

### 3.2 Secondary / future — *The discerning public user (possible, later)*

The project will be open-source and **may** go public. This persona is a **design constraint, not a current target.**

- **Who:** A like-minded Windows 11 enthusiast who would choose OK Player for its elegance and subtitle/chapter UX.
- **How it shapes today's work:** Keep the product **presentable** — no private hacks bleeding into the UI, sane empty/onboarding states, defaults that make sense for someone who is not Oleg.
- **What it does *not* mean today:** No design compromises to chase a broad audience, no settings sprawl, no invented feature requests. Optimize for the one expert user; keep the door open.

---

## 4. Goals & Guiding Principles

### 4.1 Goals

1. Be the **most elegant** media player on Windows — refinement is the headline feature, not a coat of paint.
2. Make the **everyday loop** (open → watch → resume) effortless and delightful.
3. Make **subtitles, chapters, screenshots, and precise navigation** best-in-class (the four pillars).
4. Stay a **focused, pure player** that integrates cleanly with the companion library app rather than absorbing its job.
5. Remain a **joy for one expert user** while being clean enough to show anyone.

### 4.2 Guiding principles

- **Curated simplicity (3/10).** Smart defaults forward; advanced tucked away. The simple case is never taxed by the existence of the complex case.
- **Native-first, not ported.** Authentic Windows 11 **Fluent/Mica** — subtle, not glassy. A *thin* shared design language with future Mac/Linux apps, but **never shared controls** (those are separate Rust apps; out of scope).
- **Mac-grade motion & restraint.** Smooth, well-eased transitions; clean modern typography and iconography with the reference apps' calm.
- **Defaults are the product.** Audio handling, hardware decode, resume logic, subtitle loading — all "just work" without exposure.
- **An escape hatch, not a control panel.** Power lives behind an **Advanced** door (`mpv.conf`, keybinding remap). No scripting/extensions.
- **Reserve design space for flagships before building them** — notably **Scribe auto-subtitles**: a clean "Generate subtitles" flow in the IA now, delivery later.
- **Player, not librarian.** When in doubt whether a capability belongs here, it probably belongs in the companion app.

---

## 5. Feature Priority Table

Single consolidated source of truth. IDs cross-reference the detailed requirements in §6–§13.

| Feature | Pillar / Area | Priority |
|---|---|---|
| Immersive fused Mica titlebar, edge-to-edge video (P1-D1, P1-D2) | P1 Design | MVP |
| Full chrome clears in playback / fullscreen (P1-D3) | P1 Design | MVP |
| Theme: Light + Auto (dark via auto) (P1-D4, §16.3) | P1 Design | MVP |
| Auto-hiding floating OSC + cursor hide (P1-D5–D8) | P1 Design | MVP |
| OSC never overlaps subtitles (P1-D9) | P1 Design | MVP |
| Mac-grade eased motion on all chrome (P1-D10–D12, §16.4) | P1 Design | MVP |
| Restraint: primary surface shows only core controls (P1-D14–D16) | P1 Design | MVP |
| Window-mode geometry morph transitions (P1-D13) | P1 Design | Day-2 |
| Embedded subtitle track enumeration/selection (P2-S1) | P2 Subtitles | MVP |
| External `.srt` auto-load by stem/lang match (P2-S2) | P2 Subtitles | MVP |
| 2–3 curated style presets, live-apply (P2-S5–S7) | P2 Subtitles | MVP |
| Hotkey delay nudge + visual offset indicator (P2-S8, P2-S9) | P2 Subtitles | MVP |
| Per-file subtitle offset memory (P2-S10) | P2 Subtitles | MVP |
| SRT read/display + sidecar save (P2-S17) | P2 Subtitles | MVP |
| Online subtitle search/download (P2-S3) | P2 Subtitles | Day-2 |
| **Scribe "Generate subtitles" (flagship)** (P2-S4, §13.2) | P2 Subtitles | Day-2 (reserve slot now) |
| Resync-from-line, auto-sync by audio, FPS stretch, per-show memory (P2-S11–S14) | P2 Subtitles | Later |
| Subtitle full-text search + prev/next-line seek (P2-S15, S16) | P2 Subtitles | Later |
| ASS/SSA, PGS/VobSub, WebVTT (P2-S18–S20) | P2 Subtitles | Later |
| Two simultaneous subtitle tracks (P2-S21) | P2 Subtitles | Later |
| Chapter panel: thumbnail + title + time, click-to-jump (P3-C1–C3) | P3 Chapters | MVP |
| Chapter markers on timeline (P3-C4) | P3 Chapters | MVP |
| Seekbar hover thumbnails (thumbfast-style, shared engine) (P3-C5, C6) | P3 Chapters | MVP / Day-1 |
| Tiered auto-chapters: metadata → interval markers → on-demand scene-detect (P3-C7–C9) | P3 Chapters | MVP (scene-detect = tech risk) |
| User bookmarks/custom chapters w/ thumbnails, sidecar persist (P3-C10–C12) | P3 Chapters | MVP |
| Screenshot: clean frame + with-subs hotkeys, save + clipboard (P4-X1–X5) | P4 Capture/Nav | MVP |
| Frame step, timecode jump, fine seek, on-seek readout (P4-N1–N4) | P4 Capture/Nav | MVP |
| A–B loop (P4-N5) | P4 Capture/Nav | MVP |
| Speed control, discrete steps (P4-N6) | P4 Capture/Nav | MVP |
| Clip/GIF export of A–B selection (P4-X6) | P4 Capture/Nav | Day-2 |
| Extra speed/seek variants, pitch handling (P4-N7) | P4 Capture/Nav | Day-2 |
| Local files incl. NFS/SMB as paths/UNC (§10) | Sources | MVP |
| Direct `http(s)://` stream/file URLs (§10) | Sources | MVP |
| YouTube in-app browse/play via yt-dlp (§10.2) | Sources | Day-2 (reserve slot now) |
| Audio: track switch, delay nudge, output device, >100% boost, normalization (§11) | Playback | MVP |
| Hardware decoding auto (§11) | Playback | MVP |
| Geometry (aspect/zoom/pan/rotate/deinterlace) in "rarely used" menu (§11) | Playback | MVP |
| HDR tone-map / passthrough (§11) | Playback | Later |
| Folder-as-playlist + queue + drag-reorder + `.m3u` (§10.3) | Playlist | MVP |
| Auto-advance / repeat / shuffle (§10.3) | Playlist | MVP |
| Gapless playback (§10.3) | Playlist | MVP-if-easy |
| Resume + per-file memory (position/track/offset/vol/speed/geometry) (§12) | State | MVP |
| History, configurable retention, private mode, clear history (§12) | State | MVP |
| Empty / "Continue watching" surface (§14 / 2.2) | State / UX | MVP |
| Launch-with-resume + progress report-back to library (§13.1) | Integration | MVP |
| `ok-player://` URI scheme registration (§13.2) | Integration | Reserve now / external control Later |
| JSON app index + media sidecars, no DB (§13.3) | Storage | MVP |
| Mini-player / PiP; compact "music" mode (§14 / 2.10, 2.11) | UX / Window | MVP |
| Settings (8 panels) + context menu + OSD toasts (§14) | UX | MVP |
| Keybinding remap editor (§15.2, §18.2) | Power | MVP / early |
| Custom `mpv.conf` escape hatch (§18.1) | Power | MVP-ish |
| OS media controls — Windows SMTC / Linux MPRIS (§17.1) | Integration | Nice-to-have — shipped on both platforms |
| Taskbar thumbnail toolbar + jump list (§17.2) | Win Integration | Nice-to-have if easy/robust |
| Discord Rich Presence (§17.3) | Win Integration | Nice-to-have if easy |
| Global hotkeys: play-pause / stop / mute only (§17.4) | Win Integration | Nice-to-have if easy |
| File associations + system-picker default flow (§17.5) | Win Integration | Nice-to-have (caveat mandatory if shipped) |

---

## 6. Pillar 1 — Elegant Design

"Elegant" is a set of enforceable behaviors, not a mood. The bar is macOS-utility-grade refinement delivered in native Fluent/Mica. The pillar is met only when *nothing on screen competes with the video* and *every transition reads as intentional.* (Visual language — material, theme, motion, type, icons, spacing — is fully specified in §16.)

### 6.1 Immersive Mica window
- **P1-D1 [MVP]** Custom titlebar fused with the Mica backdrop; video renders edge-to-edge *under* a translucent titlebar with integrated in-app window controls — no separate OS title strip.
- **P1-D2 [MVP]** Mica is subtle, never glassy. Applied only to chrome (titlebar, side panels, floating overlays). The video surface is never tinted/blurred by Mica.
- **P1-D3 [MVP]** During playback (standard/fullscreen), all chrome can fully clear. **Acceptance:** in fullscreen at rest, zero pixels of persistent UI over the video.
- **P1-D4 [MVP]** Theme: Light + Auto (follows system, includes dark). No manual dark toggle for MVP; dark ships via Auto. All pillar surfaces legible and on-brand in both.

### 6.2 Auto-hiding, IINA-style OSC
- **P1-D5 [MVP]** A single floating OSC bar — minimal, rounded, Mica-backed, floating over the bottom of the video, not docked chrome.
- **P1-D6 [MVP]** Auto-hides on inactivity (default idle-to-hide **~2.5 s**, internally tunable, not a user setting — this is the single canonical idle-timeout value used everywhere, see §14.0), reveals on pointer movement/interaction.
- **P1-D7 [MVP]** When the OSC hides during playback, the cursor hides too; both reappear together on movement.
- **P1-D8 [MVP]** OSC control set, one coherent bar: play/pause · timeline (chapter markers + hover thumbnail) · elapsed/remaining + timecode · volume · fullscreen · **quick subtitle-track switch** · **quick audio-track switch** · **playback speed** · **chapters** · **screenshot**. No control outside this set on the MVP OSC.
- **P1-D9 [MVP]** OSC never overlaps subtitles: subtitles shift up when the OSC is visible and return smoothly when it hides (no jump-cut).

### 6.3 Mac-grade motion
- **P1-D10 [MVP]** All chrome transitions use smooth, eased motion — no instant pop/snap, no linear/janky tweens.
- **P1-D11 [MVP]** OSC and panel show/hide animate opacity **and** position, not a hard visibility toggle.
- **P1-D12 [MVP]** Motion holds frame cadence during 4K hardware-decoded playback. **Acceptance:** opening the chapters panel mid-playback drops no video frames on target hardware.
- **P1-D13 [Day-2]** Window-mode transitions (standard ⇄ mini/PiP ⇄ compact) animate geometry where the OS permits.

### 6.4 Restraint (curated simplicity)
- **P1-D14 [MVP]** Default surfaces show only P1-D8 controls. Geometry, advanced audio, and `mpv.conf` injection live in tucked-away/Advanced menus — never on the primary OSC or first-level UI.
- **P1-D15 [MVP]** No badges, counters, ads, telemetry nags, or decorative chrome. Single accent color, consistent radii and spacing grid across all pillars.
- **P1-D16 [MVP]** The empty/idle "Continue watching" state is a designed first-class surface — recents with resume points, an Open button, a drag-drop hint, nothing more.

---

## 7. Pillar 2 — Subtitles

The #2 differentiator. Reserve design space now for the phased sync toolkit and the Scribe "Generate subtitles" flow even though those land later. The subtitle quick-switcher UI surface is specified in §14 (2.7); styling typography is governed by these presets, not the UI type ramp.

### 7.1 Sources
- **P2-S1 [MVP]** Embedded subtitle tracks enumerated and selectable.
- **P2-S2 [MVP]** External `.srt` auto-loaded on filename stem-match, including language-suffix forms (`Movie.en.srt`). Auto-loaded tracks appear in the same picker as embedded, labeled by source/language.
- **P2-S3 [Day-2]** Online auto-search/download (OpenSubtitles-class): search by hash/title, list, download, load. Reserve a "Find subtitles online…" entry now (disabled/coming-soon placeholder acceptable).
- **P2-S4 [Day-2] — Scribe auto-subtitles (flagship):** a "Generate subtitles" flow that sends the current title's audio to the user's Scribe service (`scribe.oklabs.uk`) and loads the returned transcript as a subtitle track. Reserve a clean, prominent entry point in the subtitle menu now; wire the action Day-2. Must surface progress and allow saving the generated track as a sidecar `.srt`. Once generated, a Scribe track is just another selectable track — same presets, offset memory, and persistence; no special-case rendering. (Full flow: §13.2.)

### 7.2 Style presets
- **P2-S5 [MVP]** Preset-based styling: **2–3 curated presets** — e.g. "Clean" (crisp white + subtle shadow), "Boxed" (semi-opaque background bar for busy footage), optional "Large" (across-the-room size). No per-property editor — presets are the *only* styling surface.
- **P2-S6 [MVP]** Preset choice is global with smart defaults; switching applies live without reload.
- **P2-S7 [MVP]** Vertical position respects the OSC (P1-D9) and never clips off-frame.

### 7.3 Sync toolkit (phased)
- **P2-S8 [MVP]** Hotkey delay nudge in fixed steps (default ±100 ms; coarse modifier for larger steps).
- **P2-S9 [MVP]** Transient on-screen indicator on each nudge (e.g. "Subtitle delay: +250 ms"), auto-dismissing.
- **P2-S10 [MVP]** Per-file offset memory persists via the per-file store, reapplied on reopen.
- **P2-S11 [Later]** "Resync from this line": click a subtitle line in the nav list to declare "this line = now."
- **P2-S12 [Later]** Auto-sync by audio (sushi-style).
- **P2-S13 [Later]** Timing stretch for FPS mismatch (23.976 ⇄ 25), linear re-timing.
- **P2-S14 [Later]** Per-show offset memory across episodes.

### 7.4 Subtitle navigation
- **P2-S15 [Later]** Full-text search across the active track with jump-to-line (seeks to that cue).
- **P2-S16 [Later]** Previous/next subtitle-line seek (hotkey jumps to adjacent cue start).

### 7.5 Formats
- **P2-S17 [MVP]** SRT (read + display + sidecar save for generated tracks).
- **P2-S18 [Later]** ASS/SSA with full native styling (overrides the preset model for these tracks).
- **P2-S19 [Later]** PGS / VobSub (image-based).
- **P2-S20 [Later]** WebVTT.

### 7.6 Two simultaneous subtitles
- **P2-S21 [Later]** Render two tracks at once (original + translation) with independent vertical placement. Keep the renderer track-count-agnostic; no UI for it in MVP.

---

## 8. Pillar 3 — Chapters with Thumbnails

The differentiator is **visual** chapters: thumbnails, not bare timestamps. This pillar shares the seekbar hover-thumbnail engine with the OSC and depends on a fast frame-extraction path (thumbfast-style). The chapter panel UI surface is specified in §14 (2.5).

### 8.1 Chapter panel/overlay
- **P3-C1 [MVP]** Lists each chapter as **thumbnail + title + start time**. Clicking seeks to chapter start.
- **P3-C2 [MVP]** Currently-playing chapter visually indicated and kept in view as playback progresses.
- **P3-C3 [MVP]** Reachable from the OSC chapters control (P1-D8); Mica-backed slide-in obeying §6.3 motion.

### 8.2 Timeline markers
- **P3-C4 [MVP]** Chapter boundaries render as markers on the OSC timeline. Hovering reveals the title (and thumbnail via the hover-preview path).

### 8.3 Seekbar hover thumbnails
- **P3-C5 [MVP / Day-1]** Hovering anywhere on the timeline shows a frame preview at that position (thumbfast-style) with the hovered timecode. **Shared engine** reused by chapter markers, chapter list, and bookmarks.
- **P3-C6 [MVP]** Preview generation is async and must not stall playback or the UI thread; a brief placeholder while the frame resolves is acceptable.

### 8.4 Scene-detection auto-chapters
- **P3-C7 [MVP intent — TECH RISK]** For files lacking chapter metadata, auto-generate chapters via scene/shot detection, each with a representative preview frame.
  - **Risk:** robust scene detection is compute-heavy and quality-variable; inline-on-open can block the experience, produce noisy/oversized lists, and jeopardize "instant startup / low idle CPU."
- **P3-C8 [MVP — pragmatic fallback]** Tiered strategy so the pillar never appears broken on metadata-less files:
  1. **Real chapter metadata present →** use it directly.
  2. **No metadata, detection not yet run →** present **fixed-interval thumbnail markers** (e.g. every N minutes) immediately on open — cheap, instant, always available.
  3. **Scene-detection →** an explicit **on-demand / background** "Detect chapters" action (not blocking open), off the UI thread, cached as a sidecar so it runs once per file. Cap detected chapters; merge ultra-short segments to avoid noise.
- **P3-C9 [MVP]** Detection progress shown non-modally; the user keeps watching. Results replace the interval pseudo-chapters when ready.

### 8.5 User bookmarks / custom chapters
- **P3-C10 [MVP]** Create a bookmark/custom chapter at the current position (or a timeline point), name it, get an auto-captured thumbnail.
- **P3-C11 [MVP]** Bookmarks render on the timeline and in the chapter panel, visually distinguished from source/auto chapters.
- **P3-C12 [MVP]** Bookmarks (and cached scene-detection results) persist as a **human-readable sidecar next to the media** — no database. Reopening restores them. This is the same sidecar the companion-library integration reads (§13.3).

---

## 9. Pillar 4 — Screenshots & Precise Navigation

Two tightly-coupled jobs: frictionless capture, and precise navigation to find the exact frame.

### 9.1 Screenshots
- **P4-X1 [MVP]** Two hotkeys: **(a) clean frame** — pure video, no subtitles, no OSD/OSC; **(b) with subtitles** — rendered frame including displayed subtitles. Neither ever includes OSC/cursor.
- **P4-X2 [MVP]** Each capture simultaneously saves to `~/Pictures/Screenshots` (configurable) **and** copies to clipboard.
- **P4-X3 [MVP]** Output format configurable: PNG / JPG / WEBP. PNG default for the clean/lossless case.
- **P4-X4 [MVP]** Filename = **title + timecode** (e.g. `Movie Title 01-23-45.png`), filesystem-sanitized; collisions de-duplicated, never silently overwritten.
- **P4-X5 [MVP]** A capture confirmation toast (transient, OSC-style) that does not obscure the frame and does not appear in the "with subtitles" capture.
- **P4-X6 [Day-2]** Export a short **clip or GIF** of the current **A–B selection** (§9.2), with format/quality options. Reserve the A–B model now so this is purely additive.

### 9.2 Frame / second navigation
- **P4-N1 [MVP]** Frame step forward/back on `.` / `,`, advancing exactly one frame and pausing.
- **P4-N2 [MVP]** Timecode jump: type `HH:MM:SS` (optionally `.mmm`) to seek to that absolute position.
- **P4-N3 [MVP]** Fine seek: arrows = ±5 s; Shift = larger jump. Jump-by-±N-seconds configurable in steps.
- **P4-N4 [MVP]** On-seek readout: while seeking/stepping, show **current timecode + current frame number** together, in the same transient-overlay style as the subtitle-offset indicator.
- **P4-N5 [MVP]** A–B loop: set A, set B, loop continuously; clear A–B to exit. The same A/B feed the clip/GIF export (P4-X6).
- **P4-N6 [MVP]** Speed control with discrete steps (e.g. 0.5×–2×), shown on the OSC speed control and adjustable by hotkey.
- **P4-N7 [Day-2]** Further speed/seek variants (additional steps, smart audio-pitch handling, finer/coarser configurable jump sets).

### 9.3 Cross-pillar shared components (design once, reuse everywhere)

1. **Frame-preview engine** (thumbfast-style) — powers seekbar hover (P3-C5), chapter-list thumbnails (P3-C1), timeline-marker previews (P3-C4), bookmark thumbnails (P3-C10).
2. **Transient overlay/toast** — one style serves the subtitle-offset indicator (P2-S9), the on-seek timecode/frame readout (P4-N4), and the screenshot confirmation (P4-X5).
3. **Sidecar persistence** (human-readable, no DB) — one format holds bookmarks, cached auto-chapters, and Scribe-generated subtitles (P3-C12, P2-S4); the surface the companion library integrates against.

---

## 10. Content, Sources & Playlist

Three input surfaces, all reaching libmpv through ordinary file paths or URLs. **No custom protocol handlers** for storage — NFS/SMB are mounted/UNC paths, nothing more. There is no "add source" plugin model.

| Source | Detail | Priority |
|---|---|---|
| Local files | Filesystem paths, incl. NFS/SMB as normal mounted or UNC paths (`\\nas\media\…`). Treated identically to local disk. | **MVP** |
| Direct network URLs | Arbitrary `http(s)://` stream/file URLs passed straight to the engine. | **MVP** |
| YouTube via yt-dlp | In-app browse/play window (search → pick → play in a native surface), resolved through yt-dlp. | **Day-2** |

### 10.1 Open paths
- File → Open File… and Open URL… (MVP).
- Drag-and-drop of files/folders onto any window or the empty state (MVP).
- Open from Explorer opens a **new window** by default (configurable — §15.3). The companion library can also launch the player (§13).

### 10.2 YouTube window [Day-2] — reservation only
Reserve IA for a dedicated in-app YouTube surface: a panel/window with search, result list, and native playback of the yt-dlp-resolved stream. **Do not build a generic web browser.** Incur no design debt now beyond a clean entry point (an "Open YouTube" command slot and a URL field that recognizes YouTube links). Quality/format selection is a yt-dlp concern surfaced minimally; defer detailed controls.

### 10.3 Playlist, queue & play modes
- **Folder-as-playlist [MVP]:** opening one file auto-loads its containing folder as the active playlist (natural/alphanumeric sort) — the primary playlist behavior.
- **Queue [MVP]:** "Add to queue" / "Play next"; **drag-to-reorder** within the panel.
- **Save / load `.m3u` [MVP]** so playlists are portable.
- **Auto-advance [MVP]** to next at end-of-file.
- **Repeat [MVP]:** one / all. **Shuffle [MVP]:** random, no immediate repeats.
- **Gapless [MVP-if-easy]** if achievable with the engine; otherwise defer. **No crossfade** (non-goal).
- Playlist is **per-window**: each window owns its own playlist/queue state. No tagging, ratings, smart playlists, or metadata grooming — those belong to the companion app.

---

## 11. Playback, Audio & Video Defaults

Audio and video are deliberately under-exposed. The user must rarely visit these settings; everything beyond the below lives behind the `mpv.conf` escape hatch (§18.1). Do not build a dedicated audio-filter UI.

### 11.1 Audio — smart defaults [MVP]
- **Track switching** — quick switch from OSC + context menu; the playback layer exposes track enumeration/selection and persists the choice per file (§12).
- **Audio delay / sync** — hotkey nudge with on-screen indicator (parallels subtitle delay).
- **Output device** — pick device; smart default = system default, follow system changes.
- **Volume boost > 100%** — soft-amplify above unity with a sensible ceiling; current volume persisted per file.
- **Normalization** — sensible loudness/dynamic-range handling via engine defaults. This is a **pure smart default with no exposed control** — not a toggle, not a slider, not a knob. It "just works" and is never surfaced in the UI.

### 11.2 Video — defaults & "rarely used" geometry

| Capability | Behavior | Priority |
|---|---|---|
| Hardware decoding | **Auto** — on by default, smooth-4K target. No user toggle on the main path (Advanced may override). | **MVP** |
| Geometry: aspect, zoom/pan, rotate, deinterlace | Functional but tucked into a **"rarely used" submenu** off the context menu / Video settings. Never on the OSC. Persisted per file (§12). | **MVP** |
| HDR | Tone-mapping to SDR and/or passthrough. Design space may be reserved but no HDR controls ship in MVP. | **Later** |
| Upscaling / shaders | **Explicitly declined. Do not design UI for this — ever.** | Non-goal |

---

## 12. Resume, Per-File Memory, History & Privacy

### 12.1 Resume [MVP]
- On reopen, **auto-resume** from last position.
- **Skip resume** if `< 5%` watched ("barely started") or **near the end** ("finished" — no resume 30 s before credits).
- A companion-app launch with an explicit "resume from X" **overrides** the player's remembered position (§13.1).

### 12.2 Per-file memory [MVP]
Persisted per media file (keyed to path; stable keying for moved files is a Later consideration) and reapplied on next open. These writes go to the **JSON app index**, *not* a sidecar (sidecars are reserved for content that belongs "with the media"). The remembered fields are exactly those in the brief's per-file memory list.

| Remembered | Notes |
|---|---|
| Playback position | Drives resume; subject to the 5% / near-end rule. |
| Subtitle track + offset | Aligns with P2-S10. |
| Audio track | Selected audio track only. |
| Volume | Including >100% boost level. |
| Playback speed | |
| Zoom / aspect (geometry) | From §11.2. |

> **Note:** audio *delay* is adjustable live (§11.1) but is **not** a remembered per-file field (it is not in the brief's per-file memory list); only the audio *track* selection persists.

### 12.3 History & privacy [MVP]
- History is **kept** and feeds Continue Watching + jump-list (if shipped).
- **Configurable retention** (count or age limit).
- **Private / "don't remember" mode** — a session/global toggle suppressing position, history, and recents recording while active.
- **Clear history** — one action wipes recents/history (optionally resume points); discoverable in Settings → Advanced.
- Privacy state gates every path that records *what was watched* (history, recents, resume index). Sidecar bookmarks/chapters are user-authored content and are **not** suppressed by private mode.

---

## 13. Companion-Library Integration & Storage Model

The companion library is a **separate app**; OK Player stays a pure player. Integration is intentionally thin for MVP.

### 13.1 MVP contract
- **Launch-with-resume:** the library launches the player with a target **file + "resume from X"** (optionally subtitle/audio preselection). The player opens and seeks to X, overriding its remembered position (§12.1).
- **Progress report-back:** the player reports **playback progress and "watched" state** back (periodic position updates + a watched flag when the near-end threshold is crossed) — the same signal it uses internally for resume.

```
Companion Library App                      OK Player
        │  launch(file, resumeFrom=X,        ┌──────────────┐
        │         [sub=…, audio=…])  ───────►│ open + seek  │
        │                                    │   playback   │
        │◄── progress(file, pos, %)  ────────│ report-back  │
        │◄── watched(file)           ────────│ (near-end)   │
        └────────────────────────────────────┴──────────────┘
   MVP: process invocation / CLI args + local report channel
   Later: ok-player:// URI · shared DB · cross-device (homelab)
```
Report-back cadence/channel is an implementation choice (CLI/callback/local IPC); keep it pluggable so the Later shared-DB model can replace it without UX change.

### 13.2 Scribe flagship flow (Day-2)
Scribe (`scribe.oklabs.uk`, `github.com/BeFeast/scribe-service`) is the user's own transcription service. Auto-generating subtitles from media audio is a flagship differentiator. **[Day-2]**, but the design reserves clean space **now** so the build is a fill-in, not a redesign.

- **Reserved hook (build now):** a **"Generate subtitles"** action in the subtitle menu / track switcher, alongside "Add subtitle file…" and future "Search online…". MVP may show it disabled/"coming soon" or behind Advanced — lay out the menu assuming this entry is permanent. Its result feeds the **same subtitle-track pipeline** as embedded/external SRT.
- **Target flow:** invoke → extract/stream the active audio to Scribe → non-modal progress (keep watching) → on completion, auto-load + select the track, **persist as a sidecar `.srt`** next to the media, register in per-file memory. Language / source-audio-track exposed minimally (smart default = current audio track, auto-detect language).
- **Constraints:** flag Scribe availability as a tech/integration risk; no on-device transcription model is in scope; Scribe is the only backend.

### 13.3 Storage model — JSON + sidecars, no DB
**No database** in MVP. SQLite is reserved *only* if a later need (e.g. a large shared library DB) forces it — out of scope now.

| Store | Contents | Location | Format |
|---|---|---|---|
| **App index** | Recents, resume points, per-file memory (§12), history, settings | App data (user profile) | **Human-readable JSON** (single coherent model — editable, diffable) |
| **Sidecars** | User bookmarks/chapters (incl. thumbnail refs), Scribe-generated subtitles | **Next to the media file** | JSON for bookmarks; SRT for subtitles |

Rules:
- **Sidecars travel with the media:** anything that belongs "to this file regardless of which machine plays it" (bookmarks/chapters, generated subs) is a sidecar, surviving installs and visible to the companion app.
- Per-file *playback memory* is app-index state, **not** a sidecar — it is player-local preference, not content.
- Writes must be **crash-safe** (atomic temp-then-rename) given multi-window concurrency on the shared JSON index.
- Privacy mode gates writes to the recents/resume/history portions of the index; sidecar writes are user-initiated content and proceed normally.

### 13.4 Topology & reserved seams
- **Local-only** for now — no cross-device sync; handoff and report-back are between two local processes.
- **Homelab door open:** keep the transport abstraction clean so a future homelab-hosted shared store / sync (and Scribe networking) is not blocked by a hard "localhost-only" assumption. Do not implement remote endpoints now.
- **`ok-player://` URI scheme** — **reserve the scheme now** (clean registration). MVP launch-with-resume uses process invocation / CLI args; external programmatic control via the URI is **[Later]**.

---

## 14. Information Architecture — Surfaces & States

### 14.0 Layering model (read first)

OK Player is a **single video plane with floating, auto-hiding chrome** — not a chrome-framed video box. Everything below renders as translucent Mica/acrylic layers *over* the video, never as opaque panels that shrink the picture (except docked-panel modes).

Z-order, bottom → top:
1. **Video plane** (mpv render surface; black when no media).
2. **Mica titlebar** (translucent, fused, always present; auto-hides in fullscreen).
3. **OSC** (bottom floating bar) + **seek thumbnail tooltip**.
4. **Overlays/panels** (Chapters, Playlist) — slide-in from edges.
5. **Quick-switchers** (Subtitle/Audio popovers) — anchored to their OSC buttons.
6. **OSD toasts** (volume, speed, subtitle-delay, seek readout) — transient, top-center or corner.
7. **Context menu** — at cursor.

Long-lived app-owned utilities such as Settings and Media Information are separate non-modal,
movable, resizable top-level windows. They never block player input, never force always-on-top,
reuse an existing instance when reopened, and close with their owning player. Confirmations,
destructive prompts, errors that require acknowledgement, and file pickers remain modal.

> **Global visibility rule (one rule everywhere):** chrome (titlebar + OSC + cursor) shows on pointer movement / keypress and auto-hides after the **canonical idle timeout (~2.5 s, the single value defined in P1-D6) only while playing**. While paused, chrome stays visible. Any open panel, popover, or context menu **pins** chrome and suspends the idle timer.

### 14.1 Surface inventory

| # | Surface | Tag | Layer |
|---|---|---|---|
| 2.1 | Main player surface | MVP | base |
| 2.2 | Empty / Continue-watching state | MVP | base (replaces video plane) |
| 2.3 | OSC | MVP | floating bottom |
| 2.4 | Seek thumbnail preview | MVP | tooltip over OSC |
| 2.5 | Chapter panel/overlay | MVP | slide-in panel |
| 2.6 | Playlist / queue panel | MVP | slide-in panel |
| 2.7 | Subtitle quick-switcher | MVP | popover |
| 2.8 | Audio quick-switcher | MVP | popover |
| 2.9 | Settings (8 panels) | MVP | window/overlay |
| 2.10 | Mini-player / PiP | MVP | window mode |
| 2.11 | Compact "music" mode | MVP | window mode |
| 2.12 | Context menu | MVP | menu |
| 2.13 | OSD toasts + indicators | MVP | transient |
| 2.14 | Generate-subtitles flow (Scribe) | Day-2 | reserve space now |

> **State matrix to honor everywhere:** every interactive surface must define **idle/active**, **playing/paused**, **hover**, **fullscreen**, **loading**, **error**, **no-subs**, **no-chapters** where applicable. Do not ship a surface missing its loading and error states.

#### 2.1 Main Player Surface — `[MVP]`
**Purpose:** default playback canvas; maximize picture; chrome floats and vanishes.
**Key elements:** video plane (edge-to-edge under translucent titlebar) · Mica titlebar (title text e.g. *Movie · Chapter*, integrated min/max/close, optional always-on-top pin) · OSC · transient OSD toasts.

| State | Visual |
|---|---|
| **Idle (playing)** | Pure video. No OSC, cursor hidden after the ~2.5 s idle timeout. |
| **Active (hover/move)** | Titlebar + OSC fade in (~150–200 ms ease); cursor shown. |
| **Playing / Paused** | Play/pause glyph reflects state; paused pins chrome with a subtle cue (no heavy overlay). |
| **Fullscreen** | Titlebar fully hidden; OSC is the only chrome; Esc / dbl-click exits. |
| **Loading / buffering** | Centered indeterminate Fluent ring over a dimmed frame; timeline shows a buffering shimmer on the unbuffered region (network/YT). |
| **Error** | Centered card: icon + human message + **Retry** / **Open another** / **Copy details** (mpv error behind "Details"). Non-modal — user can still open a new file. |
| **No-subs** | Subtitle button shows "off" glyph; switcher presents **Off** + **Generate subtitles…** + **Add subtitle file…**. |
| **No-chapters** | Chapter button present; no timeline markers; panel renders scene-detection result or its empty/affordance state. |

#### 2.2 Empty / Continue-Watching — `[MVP]`
**Purpose:** what the user sees with no media open; a re-entry point, not a library.
**Key elements:** app wordmark / subtle hero on Mica · **Continue watching** — horizontally scrollable cards of recents *with a resume point* (thumbnail/last-frame, title, **progress bar**, remaining-time label; click = open & resume) · **Open…** (primary) + **Open URL…** · **drag-drop hint** (dashed zone "Drop a video, or paste a URL") · footer: History link, Private-mode indicator, Settings gear.

| State | Visual |
|---|---|
| **Has recents** | Continue-watching row populated, most-recent first. |
| **No recents (first run / cleared)** | Hide the row; enlarge drop zone + Open; friendly one-liner. |
| **Private mode active** | Chip "Private mode — not recording history"; recents hidden/frozen per setting. |
| **Drag-over** | Drop zone highlights (accent border + fill); rest dims. |
| **URL paste detected** | Inline "Open this link" affordance. |
| **Loading a pick** | Card / Open shows spinner; transitions into Main Player. |

#### 2.3 OSC — `[MVP]`
**Layout (left → right), one bar:** Play/Pause · Prev/Next (hidden/disabled w/o playlist) · **Timeline** (elapsed + buffered + thumb; **chapter markers**; **A–B brackets** when set; hover → seek thumbnail + time/chapter; drag = scrub) · time readout (`elapsed / total`, click toggles remaining; **frame number** during scrub) · Volume (click = mute; **>100% boost** zone visually distinct) · Speed pill (`1.00×`) · Subtitle button → 2.7 · Audio button → 2.8 · Chapters → 2.5 · Screenshot (default clean; long-press/secondary = with-subs) · Fullscreen · Overflow "…" (geometry, loop, PiP, music mode, settings).

| State | Visual |
|---|---|
| **Hidden (idle, playing)** | Faded/off-screen; no hit-testing. |
| **Visible** | Mica/acrylic bar, slides up with shadow. |
| **Button hover** | Tooltip (label + keybind), subtle highlight. |
| **Disabled controls** | Prev/Next grey w/o playlist; chapter neutral w/o chapters; sub "off" w/o subs. |
| **Loading** | Timeline buffering shimmer; play-pause may show inline spinner. |
| **Live/URL unknown duration** | Total = `--:--`; timeline becomes progress-only / live indicator. |
| **Compact (mini-player)** | Collapses to play/pause + thin timeline (+ volume); overflow hides the rest. |

#### 2.4 Seek Thumbnail Preview — `[MVP / Day-1]`
Small rounded frame thumbnail + **timecode** caption + **chapter title** caption if the position is in a named chapter.

| State | Visual |
|---|---|
| **Generating** | Placeholder blur / skeleton. |
| **Ready** | Sharp frame + timecode (+ chapter). |
| **Unavailable** (network/YT, no index) | Fall back to **timecode-only** tooltip (no broken frame). |
| **Drag (scrubbing)** | Thumbnail tracks cursor live; main frame updates on release (or live if cheap). |

#### 2.5 Chapter Panel — `[MVP]` (Pillar 3)
Slide-in panel (right edge default). **Chapter rows:** thumbnail + title + start time (+duration); current highlighted; click = jump. Section split when both exist: **Chapters** (embedded/scene-detected) and **Bookmarks** (user). **+ Bookmark here** captures position + thumbnail + inline rename (→ sidecar). Bookmark rows: rename, delete, jump.

| State | Visual |
|---|---|
| **Has embedded chapters** | List with real thumbnails + titles. |
| **No metadata → interval markers** | Fixed-interval thumbnail markers shown immediately; header note + **Detect chapters** action. |
| **No metadata → scene-detected** | Same layout; header note "Auto-generated chapters" + **Re-scan**. *(Tech risk — show generating state.)* |
| **Generating (scene detect)** | Skeleton rows + progress; partial fill as frames resolve. |
| **No chapters & none detected** | Empty: "No chapters. Add a bookmark to mark moments." + Add. |
| **Bookmark rename** | Inline text field, confirm/cancel. |
| **Current position** | Active row highlighted; mini-marker syncs with timeline tick. |
| **Hover row** | Larger thumb peek + jump/edit icons. |

#### 2.6 Playlist / Queue Panel — `[MVP subset]`
Slide-in panel (right; co-exists with chapters via segmented header **Up Next** | **Chapters**). **Now playing** pinned top. List items = title + duration; current highlighted; watched items get subtle check/dimming (from per-file memory). Controls: Add to queue / Play next, drag-to-reorder, Save/Load `.m3u`. Mode toggles: Repeat (off/one/all), Shuffle, Auto-advance (default on).

| State | Visual |
|---|---|
| **Folder auto-playlist** | Populated from the opened file's folder, natural sort. |
| **Empty (single URL / no folder)** | Now-playing item + "Add files…" hint. |
| **Reordering (drag)** | Row lifts (shadow), insertion line shows drop target. |
| **Queued vs folder items** | Queued items grouped/badged "Next". |
| **Repeat/Shuffle active** | Toggles show active accent. |
| **Watched item** | Check glyph + reduced emphasis. |
| **Loading next** | Spinner on the incoming row. |

#### 2.7 Subtitle Quick-Switcher — `[MVP]` (Pillar 2 surface)
Popover anchored to the OSC sub button. Track list: **Off** + each embedded/external track (name + lang). Inline actions: **Add subtitle file…**, **Generate subtitles…** (Scribe; 2.14, Day-2), **Online search…** (Day-2, disabled/"coming soon" or hidden). Quick sync: **Delay −/+** with current offset; **Style** shortcut (cycles 2–3 presets). Footer: "More in Settings → Subtitles."

| State | Visual |
|---|---|
| **Has tracks** | Radio list, current selected. |
| **No-subs** | Only Off + Add + Generate (+ Online if enabled). |
| **External SRT auto-loaded** | Track shown with a small "external" badge. |
| **Delay nudging** | Inline offset updates live; mirrors the on-screen sync indicator (2.13). |
| **Generating (Scribe)** | Track row with progress/spinner; becomes selectable when ready. |

#### 2.8 Audio Quick-Switcher — `[MVP]`
Track list (name + lang + channels), current selected; **Audio delay −/+** with value; **Output device** quick pick (full options in Settings → Audio). Volume-boost stays in Settings; normalization is a non-exposed default (§11.1).
**States:** Single track · Multi-track (radio list) · Delay adjusting (live value + OSD echo) · Device changing (brief "switching output").

#### 2.9 Settings — `[MVP]`
**Shell:** left rail of 8 panels (Fluent nav) + right content pane; opens as its own non-modal,
movable, resizable window; search box at top of rail (nice-to-have). Reopening raises the existing
instance instead of stacking another. Curated-simplicity = 3/10 — smart defaults visible, power tucked.

| Panel | Contains | Notes |
|---|---|---|
| **Appearance** | Theme (Light / Auto), accent source (system accent vs. OK Player accent), Mica intensity/tint per surface. | Home for all theming — resolves the IA gap (§21 Q12); see §16.3. |
| **Playback** | Resume behavior (auto-resume; skip if <5%/near-end), auto-advance, repeat/shuffle defaults, default speed + step size, jump ±N value, A–B defaults, gapless toggle. | Smart defaults preselected. |
| **Subtitles** | Default track-language pref, auto-load external SRT (on), **2–3 style presets** (preview swatches), default delay, per-file offset memory (on), Scribe **Generate** defaults + Online-search source (Day-2 grouped, may be disabled). | Presets only — NOT a per-property editor. |
| **Video** | Hardware decoding (auto), HDR handling (Later: tone-map/passthrough — shown future/disabled), **"Rarely used" group** (aspect, zoom/pan, rotate, deinterlace). No upscaling/shaders (absent by design). | Geometry collapsed by default. |
| **Audio** | Output device, volume boost >100% cap, default audio delay. Normalization is a non-exposed sensible default (no control). | Lightly exposed. |
| **Shortcuts** | **Keybinding remap editor** (§15.2): searchable action list, current-binding chips, click-to-rebind, conflict detection, reset-to-defaults. | MVP / early. |
| **Integration** | Companion-library link/handshake, **URI scheme `ok-player://`** (reserved), SMTC/media keys, taskbar thumbnail toolbar + jump list, Discord Rich Presence, global hotkeys, **file associations** helper. | File-assoc routes through the **system picker** (Win11 25H2 caveat — guide, don't claim; §17.5). |
| **Advanced** | **Custom `mpv.conf` snippet** editor (monospace, validate/apply), screenshot format/path, history retention + **private mode** + **Clear history**, logs/diagnostics, reset all. | Developer escape hatch. |

**States:** Default · Modified (unsaved — apply/revert if not live-applied) · Invalid input (bad mpv.conf/path — inline error) · Search-filtered · Feature-disabled (Day-2/Later items shown muted with a "soon" tag to reserve the slot).

> **IA note:** the brief specified 7 functional panels (Playback, Subtitles, Video, Audio, Shortcuts, Integration, Advanced). A dedicated **Appearance** panel is added as the 8th to give theme/accent/Mica-intensity a coherent home (resolving the layout-blocking gap the brief's 7-panel IA left open — §21 Q12). All seven original panels are preserved unchanged.

#### 2.10 Mini-Player / PiP — `[MVP]`
Small, **always-on-top** floating window; rounded; video fills it; **on-hover minimal OSC** (play/pause, thin timeline, close, **expand-back**). Drag body to move; optional snap-to-corner.
**States:** Idle (just video, no chrome) · Hover (control cluster fades in over slight scrim) · Playing/Paused · Resize (maintains aspect, min-size floor) · Expand (animates back to standard, restoring full OSC/titlebar).

#### 2.11 Compact "Music" Mode — `[MVP]`
Audio-first layout: large **album/embedded artwork** (or a **static placeholder** when no artwork is embedded), title/artist/album, transport (prev/play-pause/next), timeline + time, volume, optional **Up Next** mini-list. Window shrinks to compact near-square/portrait.

> **Scope note:** the no-artwork case is a **static styled placeholder** (e.g. a generic music glyph / gradient panel) — **not** real-time waveform analysis. Live waveform generation is out of scope (it is not in the brief); if a richer no-artwork visual is ever wanted, scope it separately as a Later item.

**States:** Has artwork · No artwork (static placeholder + prominent title) · Playing/Paused (subtle now-playing motion acceptable) · Switch-to-video (offer "Switch to video view" if a video plays here).

#### 2.12 Context Menu (right-click) — `[MVP]`
Grouped: **Playback** (Play/Pause, Speed ▸, Jump ±N ▸, A–B loop) · **Open** (File…, URL…, Recent ▸) · **Subtitles** ▸ (tracks, Add, Generate, delay, style) · **Audio** ▸ (tracks, device, delay) · **Chapters / Bookmarks** ▸ (Add bookmark here) · **Video / Geometry** ▸ (the "rarely used" set) · **Screenshot** ▸ (clean / with subs) · **Window** ▸ (Fullscreen, Mini/PiP, Music mode, New window, Always-on-top) · **Settings…**. Items reflect current state (checkmarks for active track/loop/aspect; disabled when N/A).

#### 2.13 OSD Toasts & Indicators — `[MVP]`
Transient feedback for keyboard/mouse-driven changes; never persistent chrome. **Variants:** Volume, Speed (`1.25×`), **Subtitle delay** (the visual sync indicator, e.g. `+120 ms`), Audio delay, **Seek readout** (timecode **+ frame number**), Screenshot saved (mini-thumbnail "Saved + Copied"), A–B set, Mute.
**States:** Appearing/Fading (auto-dismiss ~1–1.5 s) · Sustained while adjusting (stays during rapid repeats, then fades) · Stacking rule (one slot; newest replaces, no pile-up).

#### 2.14 Generate-Subtitles (Scribe) Flow — `[Day-2], reserve space now`
Entry from subtitle quick-switcher + context menu + Settings → Subtitles. Flow: **Generate subtitles ▸** language/source-track pick → **progress (with cancel)** → on completion the new track appears selected, written as an external SRT sidecar. Empty/Error states mirror network-job patterns (retry, copy details). See §13.2 for the full flow.

---

## 15. Interaction Model — Mouse, Keyboard, Window

### 15.1 Mouse map [MVP]
Default bindings on the **video plane** (panels/controls keep standard widget behavior):

| Gesture | Action |
|---|---|
| Single click | Play / Pause |
| Double click | Toggle Fullscreen |
| Mouse wheel | Volume up/down (stepped; OSD toast) |
| Drag on timeline | Seek / scrub (live thumbnail) |
| Drag on video body (mini-player) | Move window |
| Right click | Context menu (2.12) |
| Hover (any) | Reveal chrome, show cursor, reset idle timer |
| Hover timeline | Seek thumbnail + time/chapter tooltip |

> **Feel detail to tune:** single-click vs double-click uses the system double-click interval; the pause toggle commits only after that window lapses, to avoid a pause-flash on a fullscreen double-click. Call this out for tuning.

### 15.2 Keyboard — defaults + remap editor [MVP]
Curated defaults (`?` opens a cheat overlay):

| Key | Action | Key | Action |
|---|---|---|---|
| Space / K | Play-Pause | S | Screenshot (clean) |
| F / dbl-click | Fullscreen | Shift+S | Screenshot (with subs) |
| Esc | Exit fullscreen / close panel | C | Chapters panel |
| ← / → | Seek ∓5 s | P | Playlist / Up Next |
| Shift+← / → | Larger seek | V | Cycle subtitle track |
| , / . | Frame step back / forward | Z / X | Subtitle delay ∓ |
| ↑ / ↓ (or wheel) | Volume | B | Add bookmark here |
| M | Mute | N / Shift+N | Next / Prev |
| [ / ] | Speed down / up | ? | Shortcut cheat overlay |
| L | A–B loop (set A, set B, clear) | J | Type timecode → jump |

**Remap editor (Settings → Shortcuts):** searchable action list; each row shows action + current binding chip(s); click chip → "Press a key…" capture; **conflict detection** (highlights collider, offers reassign); add secondary binding; reset-action / reset-all. States: *default*, *capturing*, *conflict*, *custom (modified badge)*.

### 15.3 Window behaviors [MVP]
- **Custom Mica titlebar:** translucent, fused with content; integrated min/max/close; video extends beneath; auto-hides in fullscreen. Title text uses the media display title with graceful truncation.
- **Multi-window / multi-instance:** every "New window" is an independent player (own media, own state).
- **Open-from-Explorer = new window** by default; **configurable** (Settings → Playback/Integration) to "reuse existing window / add to queue."
- **Window modes (all MVP):** Standard ↔ Fullscreen ↔ Mini-player/PiP ↔ Compact music; transitions animate (mac-style).
- **Always-on-top:** intrinsic to mini-player; manual toggle in standard mode.
- **Companion windows:** Settings and Media Information remain independent, non-modal, resizable
  utilities. Closing them never pauses or closes playback; closing the player cleans them up.

---

## 16. Design System

This is the heart of the product — Pillar 1 is "most elegant design." Mandate: **macOS-utility-grade refinement, delivered in native Windows Fluent/Mica.** Not a macOS skin — a Windows app a Mac user would call beautiful.

### 16.1 Reference-app feel — the north star

| Reference | What to steal (the *feeling*) |
|---|---|
| **Elmedia Player** | Immersive video-first chrome; the OSC floats *over* content and gets out of the way. |
| **IINA** | Modern player done with restraint; rounded floating controller, thumbnail-rich seekbar. |
| **Paste** | Buttery card/panel motion; depth via shadow + subtle material, never heavy glass; precise spacing. |
| **CleanMyMac** | Confident generous whitespace, large legible type, delightful-but-never-childish micro-animation. |
| **Bartender** | Quiet utility polish; settings that feel calm, not a wall of toggles. |

> **Synthesis rule:** when ambiguous, ship what a designer of these apps would ship. Refined > feature-dense. Calm > busy. Motion that *explains* > motion that decorates.

**"Mac-grade without copying macOS chrome":**
- ✅ Borrow: generous padding, restrained palette, soft depth, precise alignment, smooth easing, typographic hierarchy, *fewer, better* controls.
- ❌ Do NOT borrow: traffic-light buttons, macOS segmented controls, the menu bar, SF Pro / SF Symbols, Dock/sheet metaphors, any literal Aqua/Sonoma chrome.
- Must read as **unmistakably native Windows 11** to a Windows user and **unusually refined** to a Mac user. Fluent done with restraint *is* the bridge.

### 16.2 Material & backdrop — Mica, used quietly [MVP]
- **Mica** is the window backdrop — **subtle, not glassy** (desktop-tint material, low blur, wallpaper-tied). Do not default to aggressive Acrylic blur; it reads cheap and "glassy," which the brief rejects.
- **Where Mica appears:** custom titlebar, settings panels, side panels, overlay surfaces. **Never** on the video frame — video is opaque content under translucent chrome.
- **Acrylic** only for *transient, floating-over-video* surfaces where in-context blur aids legibility (OSC background, context menu, flyouts) — thin/subtle, never frosted. Back every floating control with enough material + a soft scrim that it stays readable over both black-letterbox and bright-snow frames.
- **Floating-control legibility recipe (must be resolved before broad overlay build):** the exact scrim + Acrylic recipe that guarantees OSC/popover/toast legibility over *arbitrary* video (very bright vs. very dark frames) is an open spike that **constrains the whole overlay system** — see §21 Q4. Until resolved, treat a soft scrim-behind-material as the working baseline.
- **Layering discipline:** a 3-tier elevation system — (0) window/Mica base, (1) panels/cards, (2) floating overlays (OSC, menus, toasts). Depth from **shadow + a one-step material/tint shift**, Paste-style, never from stacking blurs.

### 16.3 Theme — Light + Auto (dark via Auto) [MVP]
- Ship **Light** and **Auto (follow system)**. Dark ships, reachable only via Auto (no standalone manual dark toggle in MVP). A manual three-way can be added later (§21 Q1).
- All theme controls live in the **Settings → Appearance** panel (§14 / 2.9).
- **Design both light and dark fully** — Auto means dark *will* render most evenings; it is not second-class. Both equally polished.
- Honor the **system accent** by default for the accent role (selection, active track fill, focus), with an override to OK Player's own accent. Accent is a *spice* — seekbar fill, active states, focus — not splashed across chrome.
- Respect **reduce-transparency** and **contrast** settings: when transparency is disabled, fall back to solid Mica-tint surfaces gracefully.

### 16.4 Motion language — mac-style easing [MVP]
- **Easing:** gentle ease-out / ease-in-out spring rather than linear or harsh Fluent defaults. Reference: ease-out cubic-ish (`~cubic-bezier(0.25, 0.1, 0.25, 1)`) for entrances; a soft spring for settling surfaces. Signature = *decelerate-into-place*.
- **Durations:** ~**120–200 ms** for most transitions, **80–120 ms** for hover/press, up to ~**250 ms** for larger reveals (settings, full panels). Never laggy, never abrupt.
- **Animate:** OSC auto-hide/reveal (fade + few-px translate — the most-seen animation, must be exquisite) · panel open/close (slide + fade from edge) · window-mode transitions (signature physical morph where feasible) · seekbar hover thumbnail (quick fade, *faster* than general motion — must keep up with the cursor) · sync indicator / screenshot confirmation / toasts (brief, gentle) · hover/press states (subtle scale/opacity, never bouncy).
- **Do NOT animate:** the video frame, seeking response (instant), anything in the critical playback path.
- **Respect `prefers-reduced-motion` / system "animations off"** — degrade to instant or cross-fade-only. Required.

### 16.5 Typography [MVP]
- **Primary typeface: Segoe UI Variable** (the Windows 11 system font) — native and correct. **Do not** import SF Pro or other macOS fonts; refinement comes from *how* type is used.
- Lean on optical sizes: **Display** for large headers/empty state, **Text** for body, **Small** for dense metadata.
- **Hierarchy:** tight type ramp (empty-state title / panel header / list item / metadata / caption); favor generous line-height and clear weight contrast over many sizes.
- **Numerics: tabular / monospaced figures for all timecodes, frame numbers, durations, and the timecode-jump field** so digits don't jitter while playing. Non-negotiable for the precise-navigation pillar.
- Subtitle rendering typography is governed by the Pillar-2 presets, not this UI ramp.

### 16.6 Iconography [MVP]
- **Base set: Fluent System Icons** (native, dual light/dark, regular/filled) — covers most of the UI.
- **Style:** clean, modern, consistent line weight, slightly rounded. Custom glyphs (frame-step, A–B loop, "generate subtitles," chapter-thumbnail markers) drawn to Fluent's grid/stroke so they're indistinguishable from the system set.
- **Do not** use SF Symbols or emoji as UI icons. One coherent family, consistent optical size and stroke weight.
- **State convention:** outline = idle, filled / accent-tinted = active (active loop, muted, subtitle-on). Consistent everywhere.

### 16.7 Layout, spacing & shape [MVP]
- **8px spacing grid** (with a 4px half-step); generous padding over cramped density.
- **Corner radius:** Windows 11 rounding (≈8px on panels/cards, ≈4px on small controls; OSC and floating overlays use a larger pill/rounded-rect radius, IINA-style). Consistent across the family.
- **Focus & states:** every interactive element needs designed hover, pressed, focus (keyboard), and disabled states — keyboard focus visuals must be present and tasteful (this is a keyboard-driven power-user app).
- **Empty state ("Continue watching")** is a flagship canvas, not an afterthought — recents-with-resume cards, Open button, drag-drop hint, composed with CleanMyMac-grade whitespace. It is the first thing the user sees.

---

## 17. Windows Integration

> **Priority for this entire section:** *every* item is **"nice-to-have, only if easy & robust."** None is on the MVP critical path. **Do not ship a flaky or hacky version of any of these.** If an item can't be done robustly, cut it. File associations (17.5) carry a mandatory platform caveat if shipped.

### 17.1 SMTC `[nice-to-have if easy/robust]`
Integrate with **System Media Transport Controls** so hardware/keyboard media keys and the Windows now-playing flyout work. Populate title + artwork where available (cover in music mode; frame/thumbnail otherwise). WinUI 3 / Windows App SDK exposes this cleanly via `SystemMediaTransportControls` — one of the more robust integrations and a strong candidate to ship.

**Implementation status:** shipped. Windows projects the observed libmpv playback snapshot into SMTC and routes play, pause, stop, next, previous, seek, and supported rate requests back through the existing player command surface. The flyout uses the same display title/tag sources as the in-app now-playing surfaces, sidecar or embedded cover art for audio, and a decoded frame for video, with the app icon as the fallback. Linux provides the corresponding MPRIS session. Neither platform currently exposes a user-facing OS-media-controls toggle, so both integrations are enabled by default while media is loaded.

### 17.2 Taskbar thumbnail toolbar + jump list `[nice-to-have if easy/robust]`
- **Thumbnail toolbar:** play/pause, prev, next on the taskbar thumbnail.
- **Jump list:** recent files (mirroring the in-app recents/resume index), optional "Open file…".
- The legacy thumbnail-toolbar API is Win32-era and fiddly under WinUI 3 / packaged apps — ship only if robust. Jump-list recents are cleaner and can ship independently.

### 17.3 Discord Rich Presence `[nice-to-have if easy]`
"Now playing" presence: title, optional elapsed/remaining, paused state. Lowest-stakes; via Discord IPC/RPC. A single toggle in Settings → Integration; **recommended off by default** since it broadcasts viewing activity (privacy-conscious default — the brief did not mandate a default state, see §21 Q7). Cut silently if Discord isn't running.

### 17.4 Global hotkeys `[nice-to-have if easy]`
Scope deliberately tiny: **play-pause, stop, mute** as system-wide hotkeys — only if robust. Work when unfocused/background. Do not expand beyond these three (the full keybinding set is in-app only). Configurable in Settings → Shortcuts and **conflict-aware**. If robust capture isn't achievable, ship nothing here.

### 17.5 File associations — handle the Win11 25H2 caveat `[nice-to-have; caveat mandatory if shipped]`
- **Goal:** clean registration of supported media extensions so OK Player appears as a choice, and a smooth path to help the user *make it default.*
- **Hard platform reality:** **Windows 11 25H2 blocks programmatic default-app setting** (UserChoice hash is protected; the **UCPD** driver actively prevents silent default claims). **Do NOT forge UserChoice or fight UCPD.**
- **Required UX:** register association *capabilities* cleanly (OK Player becomes a legitimate listed candidate), then **guide the user through the system picker** — deep-link **Settings → Default apps** to the file type (or trigger the "Open with → Always" dialog) and walk them through. Frame it honestly: "Windows requires you to confirm this — we'll take you there." A polished, one-tap-then-confirm flow (Bartender-grade calm), **never** a silent claim that mysteriously fails or gets reverted.

---

## 18. Power Features

The product is intentionally curated-simple (3/10); power lives behind an **Advanced** escape hatch so the one expert user is never trapped, without cluttering the default experience. Exactly three power affordances are in scope — and **no scripting.**

### 18.1 Advanced `mpv.conf` escape hatch `[MVP-ish, low effort]`
A field under **Settings → Advanced** to **inject a custom `mpv.conf` snippet** merged into the libmpv configuration — the universal pressure-release valve for anything the curated GUI doesn't expose. **Design:** monospaced editor, clearly fenced as "advanced / unsupported," a note that bad input can break playback, and a **Reset to defaults** button. Surface mpv's parse errors rather than failing silently. Apply on save (hot-apply where libmpv allows).

### 18.2 Keybinding remap editor `[MVP / early]`
A proper remap editor in **Settings → Shortcuts**: curated sensible defaults (§15.2), rebind any action. **Design:** searchable action list, click-to-capture chord, **conflict detection/warning**, per-action reset, reset-all. First-class, not buried. Also configures the three global hotkeys (17.4), clearly marked system-wide.

### 18.3 Scripting / extensions — explicitly OUT
**No scripting, no Lua, no plugin/extension system.** Deliberately excluded to protect curated simplicity. Do not design affordances that imply it exists; the `mpv.conf` hatch is the sanctioned power surface — that's the line.

---

## 19. Non-Functional Requirements

### 19.1 Platform
**Windows 11 only.** No Windows 10, no down-level fallbacks. Free to depend on Win11-era APIs (Mica, WinUI 3 / Windows App SDK, modern SMTC).

### 19.2 Engine architecture — libmpv via the render API
- A **GUI wrapper over libmpv** (the IINA / mpv.net lineage), not a from-scratch decoder.
- On Linux Wayland, the video plane uses a desynchronized native EGL subsurface below the GTK
  chrome. This preserves the single-plane overlay composition while keeping video presents out of
  GTK/GSK; X11 retains the `GtkGLArea` compatibility path.
- **Use the mpv render API** so the app composites overlay controls (OSC, panels, thumbnails, indicators) *on top of* the video surface — this enables the immersive controls-over-video chrome.
- **Child-window (`wid`) embedding is an acceptable fallback** if render-API integration with WinUI 3 / Composition proves problematic, but the render-API path is strongly preferred (the overlay design depends on it). **Flag the WinUI3 ↔ mpv render-API seam as a known tech risk to prototype early — it gates the core UX.**

### 19.3 Performance targets
- **Instant startup:** cold launch to interactive (and to first frame on open) must feel immediate — a Pillar-1 cue. Defer/lazy-load anything non-essential off the launch path.
- **Low idle CPU:** when paused, in the empty state, or backgrounded, CPU/GPU and wake-ups must be minimal. Auto-hide, thumbnail generation, and animations must not spin the CPU when nothing is happening.
- **Smooth 4K with hardware decoding:** fluid via auto HW decode (`hwdec=auto`-class). No dropped frames / stutter on capable hardware; HW decode on by default.
- **Responsiveness:** all UI interaction (seek, panel open, mode switch) stays at display refresh and never blocks on I/O — seeking in particular must feel instant.

### 19.4 Resource & storage footprint
**No database** in MVP: human-readable JSON (recents/resume index) + sidecars next to media (bookmarks/chapters/generated subs). SQLite reserved for later only if scale demands. Keep on-disk state portable, inspectable, hand-editable.

### 19.5 Quality bar
The overarching non-functional requirement is **"mac-grade polish": refined, responsive, considered.** Jank, layout pops, animation stutter, audio/video desync on seek, and visible loading hitches are **defects against this bar (P1), not cosmetic nice-to-haves.** This is the differentiator; treat regressions in feel as P1.

---

## 20. Non-Goals

OK Player earns its elegance by **declining** scope. These are out of scope by design — not "later," but *not this product* (the companion library app and the OS own several).

**Hard non-goals (will not build):**
- **No media library / catalog / metadata management** — the companion app's job.
- **No DVD / Blu-ray.**
- **No IPTV.**
- **No casting / DLNA server.**
- **No transcoding.**
- **No video editing.**
- **No touchpad gestures** (explicitly declined).
- **No upscaling / custom shaders** (explicitly declined — design no UI for it, ever).
- **No Windows 10 support** — Windows 11 only.
- **No scripting / plugin / extension system** — power is the `mpv.conf` hatch + keybinding remap, not an extension API.
- **No custom URL/protocol stack for network shares** — NFS/SMB are ordinary file/UNC paths.
- **No cross-device sync today** — state is local. (The homelab makes this tempting later; the architecture keeps the door open — §13.4 — but it ships local-only.)
- **No crossfade** between playlist items.
- **No real-time music-mode waveform analysis** — the no-artwork compact view uses a static placeholder, not generated waveforms.

**Deliberately deferred (reserve space, don't build now):**
- **YouTube in-app browsing/playback** via yt-dlp — *[Day-2]*.
- **Scribe auto-generated subtitles** — flagship intent; reserve the "Generate subtitles" flow now, deliver *[Day-2]*.
- **Online subtitle search/download**, advanced sync (auto-sync by audio, FPS stretch, per-show memory), dual simultaneous subtitles, image/ASS/WebVTT subtitle formats — *[Day-2]/[Later]* per §7.
- **Clip/GIF export** of A–B selection — *[Day-2]*.
- **HDR** tone-map / passthrough — *[Later]*.
- **`ok-player://` external programmatic control** — scheme reserved now, control *[Later]*.

---

## 21. Open Questions

Decisions for the user before/during build. Each notes a recommended default so silence resolves sanely.

**Design system**
1. **Dark theme reachability** — also offer a manual Light/Dark/Auto three-way now, or strictly Light/Auto for MVP? *(Rec: Light/Auto only at MVP; manual Dark is trivial later — would slot into the Appearance panel.)*
2. **Accent source** — default to system accent, or ship a signature OK Player accent? *(Rec: follow system by default, with override in Appearance.)*
3. **App identity** — design a custom app icon / wordmark / signature accent up front (it sets the empty-state tone), or defer? Working name may change.
4. **Floating-control material over arbitrary video [DESIGNER-BLOCKING]** — confirm the scrim/Acrylic recipe guaranteeing OSC/popover/toast legibility over both very-bright and very-dark frames. This needs a designer + eng spike and **constrains the whole overlay system** (§16.2); OSC, popover, and toast backing cannot be finalized until it is resolved. *(Rec: prototype a soft scrim-behind-Acrylic baseline early and validate against snow-bright and near-black frames.)*

**Windows integration**
5. **Which integrations clear the "easy & robust" bar?** *(Rec: confirm ship-set as SMTC + jump-list recents; thumbnail toolbar, Discord RP, global hotkeys as stretch — user ratifies effort vs. cut.)*
6. **File-type scope** — exact extensions to register (movies/series first; where do music and less-common containers sit?).
7. **Discord RP default state** — the brief did not specify a default. *(Rec: off by default, since it broadcasts viewing activity; user confirms.)*

**Architecture / non-functional**
8. **Render API vs. `wid` fallback** — early prototype to confirm the render-API path integrates with WinUI 3 Composition (gates the overlay UX). How much effort before falling back to `wid`?
9. **Packaging / distribution** — MSIX (packaged; clean associations + identity) vs. unpackaged? Affects file-association and SMTC plumbing. *(Not specified — needs a call.)*
10. **"Mica subtle, not glassy" calibration** — agree the exact material/tint per surface (titlebar vs. panels vs. OSC) against the reference apps before broad UI build, so the look is locked once.

**Cross-cutting / future-facing (reserve space, don't build)**
11. **`ok-player://` URI scheme** — reserve now (cheap), even though external control is *[Later]*; confirm the user wants it registered at MVP for the companion-library handoff.

> **Resolved (was an open IA question):** *Where do theme/accent/Mica-intensity live in Settings?* — **Resolved** by adding a dedicated **Appearance** panel as the 8th Settings panel (§14 / 2.9, §16.3). The brief's seven functional panels are preserved; Appearance is additive so the designer can place all theming controls without ambiguity.
