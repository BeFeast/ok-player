# Compact Window Modes — Mini-Player (PiP) + Compact Music Mode

You are designing **two compact window modes for OK Player**, the most elegant media player on Windows — native Fluent/Mica refinement at macOS-utility grade, over libmpv. Produce the visual design (design-system extensions + screens + every state) for both modes **together, as one cohesive family**: the **Mini-Player (PiP)** and the **Compact Music Mode**.

This prompt **extends** — never contradicts — two sources of truth:
- **`docs/OK-Player-PRD.md`** (the product contract; §2.10 Mini-Player, §2.11 Compact Music Mode, §2.3 Compact OSC, §2.13 OSD toasts, §14.0 state matrix + visibility rule, §15 mouse/window behaviors, §16 visual language, P1-D9 OSC-never-overlaps-subtitles, P1-D13/§16.4 window-mode transitions).
- **`docs/claude-design-prompt.md`** (the main design brief — native Fluent/Mica, **Light + Auto/dark**, teal accent, calm and restrained, IINA-grade restraint + Paste motion).

Read both first. Match their voice, their tokens, and their taste. Where the implemented design system already names a brush, radius, duration, style, or glyph, **reuse it by name**; only invent a new `Ok*` token when nothing existing fits, and say why. Where the PRD's aspirational guidance and the *shipped* system diverge (icon family, timecode style), **extend the shipped reality** and note the divergence — this prompt is called out below at each such seam.

**The four pillars rank every decision when they collide (lower wins):**
1. **Most elegant design** — the reason someone chooses OK Player. Pillar 1 wins all conflicts.
2. **Best subtitle UX.**
3. **Beautiful chapters with thumbnail previews.**
4. **Convenient screenshots + precise frame/second navigation.**

When ambiguous, ship what a designer of **IINA, Elmedia, Paste, CleanMyMac, or Bartender** would ship. Refined > feature-dense. Calm > busy. Motion that *explains* > motion that decorates.

---

## Hard constraints (locked — bake these in, do not relitigate)

1. **Scope is exactly these two modes, designed as siblings.** Playlist/queue is a **separate future prompt** — out of scope here. The optional **Up Next** mini-list inside music mode *is* in scope (it is part of §2.11), but the full standalone queue surface is not.
2. **The Mini-Player is a custom borderless always-on-top window** — **not** the OS `CompactOverlay`. This **replaces** the native `AppWindowPresenterKind.CompactOverlay` shipped in PR #63. We render everything ourselves: rounded corners, our own hover-reveal control cluster over our own scrim, snap-to-corner, a min-size floor with aspect lock, and an animated expand-back to standard. **Design assuming full custom control of the window chrome** — there is no OS titlebar, no OS PiP frame, no native always-on-top affordance to defer to.
3. **Music mode is entered automatically AND manually.** Automatically for audio-only files (detected via `VideoWidth <= 0` — already wired). Manually via a toggle in the **Window submenu** and the **right-click context menu**. When a **video track plays while in music mode**, surface a **"Switch to video view"** affordance — do not silently swap layout.
4. **Mini-Player and Music Mode are distinct modes but one family.** Mini-Player = compact **video PiP**. Music Mode = **audio-first** view. They **share** the floating-control material recipe, the compact transport cluster, the corner-radius language, and the chrome motion. Design them so they read as siblings — same DNA, different job.
5. **Subtle now-playing motion is allowed in music mode, never a waveform.** Permitted: a gentle **artwork parallax / ken-burns drift** OR a **soft accent pulse near the timeline**. **Forbidden, explicitly and forever:** real-time waveform, audio-reactive visualizer, or any generated-from-audio graphic (PRD hard non-goal). The no-artwork case is a **static styled placeholder** (generic music glyph / gradient panel), not a synthesized visual.

---

## Solve first — the two foundations both modes inherit

Everything else hangs off these. Get them right before drawing a single screen.

### (a) The compact floating-control material recipe

Both modes float minimal controls over content that can be **pure-black letterbox one second and blown-out snow the next** — and, in music mode, over **album artwork of any color**. The control cluster must stay legible over *all three* without ever reading as heavy glass.

Extend the **existing implemented over-video recipe** — do not start fresh:
- Container fill: **`OkOverVideoFillBrush`** (`#80161619`, 50% dark) with a 1px top highlight of **`OkOverVideoHairlineBrush`** (`#24FFFFFF`).
- Toast/heavier fill: **`OkOverVideoToastBrush`** (`#99161619`, 60% dark).
- Text hierarchy: **`OkOverVideoTextBrush`** (95% white) / **`OkOverVideoTextDimBrush`** (80%) / **`OkOverVideoTextFaintBrush`** (45%); accent always **`OkOverVideoAccentBrush`** (`#28B3AA`, no alpha).
- Bottom/edge gradients: **`OkBottomScrimBrush`**, **`OkTopScrimBrush`**, **`OkPanelEdgeScrimBrush`**.
- Seekbar parts (over video / over artwork only — see substrate rule in foundation b): **`OkSeekTrackBrush`** / **`OkSeekBufferedBrush`** / **`OkSeekFillBrush`** / **`OkSeekThumbBrush`**; chapter ticks **`OkChapterTickBrush`**; A–B region **`OkAbRegionBrush`**.

**The challenge to solve:** the existing OSC leans on a tall bottom-screen scrim (220px) it can afford in standard mode. A mini-player is *tiny* and the cluster floats in the middle of frame on hover; music mode floats controls over saturated artwork. Specify the **compact floating-control material** as a self-contained, **theme-invariant** recipe (these brushes are deliberately dark-on-content in both themes):
- Define the **localized scrim** that backs a floating cluster when there is no full-width bottom gradient to rely on — e.g. a soft radial/elliptical darkening or a rounded scrim card behind the cluster, sized to the cluster, not the window. Give exact opacity and falloff.
- Prove legibility in your mockups over **(1) black letterbox, (2) bright-snow frame, (3) a saturated/light album-art panel**. This is the deliverable's acid test — show all three.
- Material is **not AcrylicBrush** here (the engine can't sample the SwapChainPanel for live blur). Depth comes from **scrim fill + soft drop shadow + the one-step hairline**, Paste-style — never stacked blurs. Where the PRD says "Acrylic over video," read it as *this scrim recipe*, not literal `AcrylicBrush`.

**Reduce-transparency / HighContrast fallback (required — do not skip).** The PRD normatively requires honoring reduce-transparency and contrast settings: when transparency is disabled, fall back to **solid Mica-tint surfaces**. The floating scrim is the most material-dependent surface in this whole prompt, so it must carry the fallback:
- **Reduce-transparency branch:** push the cluster fill toward full opacity (drive `OkOverVideoFillBrush`/`OkOverVideoToastBrush` to ~100% alpha, or substitute a solid **`OkPopoverBrush`** / **`OkSurfaceBase`** tint card behind the cluster). No translucency, no gradient falloff — a solid rounded card with the hairline and shadow.
- **HighContrast branch:** the theme-invariant dark scrim will fail contrast; design a HighContrast variant of the cluster on system HighContrast brushes with a visible 1px border and system-colored focus visuals.
- Add a matrix row for this (see the coverage checklist).

**Elevation/shadow token (new — flag for deliverable 1).** No shadow/elevation token exists in the shipped system today (only an ad-hoc "content shadow" note). You must introduce a new **`OkShadow*` / elevation token** covering: the borderless mini-player lift off the desktop, the compact toasts, and the compact overflow popover. Specify blur / spread / Y-offset / opacity and its Light/Dark/HighContrast behavior. Tie it explicitly to deliverable 1.

### (b) The compact OSC reduction

Define how the **full standard OSC collapses to the compact set** (§2.3), then reuse that compact cluster in *both* modes. The full OSC today is a single pill (token **`OkOscCornerRadius`** is 16px; the shipped pill renders at ~18px — treat the OSC-family radius as **~16–18px** and keep the compact family consistent with one chosen number; margin `16,0,16,18`, padding `18,11`, 16px column spacing) carrying play · prev/next · elapsed · seekbar · duration · volume · speed chip · subtitles · audio · chapters · screenshot · fullscreen · overflow.

**Compact reduction rule (normative):** surface only **play/pause + thin timeline (scrubable) + volume**. Everything else — chapters, subtitle quick-switch, audio quick-switch, speed, screenshot, fullscreen — moves into an **overflow "…" affordance** (popover or right-click context menu). Specify:
- What stays visible at the cluster's smallest size, and the **overflow threshold** — recommend volume demotes into overflow when the cluster falls below **~360px usable width**; tune from there.
- The **thin timeline** treatment: recommend a **3–4px visible track**, **~12px thumb**, with the **hit-target padded to ≥20px** (the visible track is thin; the mouse/touch target must not be). Tune these numbers — don't invent from zero.
- **Seekbar brush by substrate (named decision, not a blanket "reuse `OkSeek*`").** The `OkSeek*` / `OkChapterTick*` brushes are **theme-invariant over-video tokens** (e.g. `OkSeekTrackBrush` is `#40FFFFFF`, 25% white) and go invisible on a light Mica surface. So: **mini-player and any over-artwork timeline use `OkSeek*`** (over-video); the **music-mode-over-Mica timeline uses a themed treatment** — **`OkAccentRailBrush`** for the played fill, a themed track brush such as **`OkSubtleFillStrongBrush`**/**`OkStrokeBrush`** for the track — **or** seat the music timeline on a darker inset panel so the over-video `OkSeek*` brushes stay valid. Make this explicit in the redline.
- **Chapter ticks and A–B brackets in compact:** recommend ticks **yes** when present, A–B **yes** when set — cheap and on-pillar.
- **Seek hover-thumbnail disposition (per mode).** §15.1 maps *hover timeline → seek thumbnail + time/chapter tooltip*. The shipped seek-preview warm-up **bails for audio-only** (`VideoWidth <= 0` — the engine can't produce frames). So: the **Mini-Player** shows a **scaled-down hover thumbnail** (sized to the tiny window — specify max thumbnail size and whether it overflows the window bounds), while **Music Mode shows time/chapter tooltip only — no frame thumbnail** (audio-only), falling back to the timecode readout. State both.
- The **expand-back / close** glyphs as first-class members of the compact cluster (mini-player only — see Mini-Player glyph recommendations).

**Timecode typography (close the shipped TODO).** All elapsed/duration/seek-readout/frame-number figures in **both modes** use **`OkTimecodeTextStyle`** (12px Medium) — and its open *"TODO: tabular figures"* gap **is exactly the gap to close here**: enable tabular/monospaced figures so digits don't jitter while playing (non-negotiable per §16.5). **`OkMonoFontFamily`** ("Cascadia Code, Consolas") is the fallback figure source. The mini-player's time readout uses this style too — do not invent separate timecode typography.

---

## Shared components — design once, reuse in both modes

1. **Compact transport cluster** — play/pause (primary), prev/next where applicable, thin timeline, volume. Built on **`OkOscIconButtonStyle`** / **`OkOscSvgButtonStyle`** (34×34, 8px radius, transparent → `#1FFFFFFF` hover → `#2EFFFFFF` pressed). Define a **compact size variant** (recommend 28×28 with proportionally tighter spacing) and prove the hit-targets still satisfy comfortable mouse use.
2. **Compact floating-control material** (foundation a) — the localized scrim recipe, including the reduce-transparency/HighContrast fallback and the new `OkShadow*` token.
3. **Overflow "…" popover** — built on **`OkPopoverFlyoutStyle`** / **`OkMenuFlyoutPresenterStyle`**, rows on **`OkSwitcherRowStyle`**. Define its compact width (the standard `OkPopoverWidth` 288px is too wide for a mini-player — specify a narrower compact popover).
4. **Up-Next list rows** (music mode) — reuse the **chapters/Up-Next row language already shipped** (accent-teal selection via `OkAccentSelectionFillBrush`, `OkRowCornerRadius` 7px). Define a **compact row** for the near-square window: thumbnail/glyph + title + secondary artist, tabular time (`OkTimecodeTextStyle`) on the right.
5. **OSD toasts in compact context** (§2.13) — volume, speed (`1.25×`), subtitle/audio delay, seek readout, screenshot-saved, A–B set, mute. Reuse **`ToastShowSb`/`ToastHideSb`** and **`OkOverVideoToastBrush`**. **Sustain ~1.5–1.7s** (PRD §2.13 baseline ~1–1.5s; shipped storyboard ~1.7s — follow the shipped value), 150ms in / 250ms fade. **One slot only; newest replaces; no pile-up.** Specify toast placement *inside a tiny window* — a top-center toast that would cover the whole mini-player needs a rule (recommend: shrink-to-fit single-line toast, or suppress non-critical toasts below a min window size; justify the choice). **Per-mode subset:** the **seek readout in the Mini-Player is timecode + frame number** (tabular figures); in **Music Mode it is timecode-only** (no frame number — audio has no frame readout, and no frame-step/screenshot pillar-4 affordances apply). **Mouse-wheel-over-window still adjusts volume (stepped) and fires the volume OSD toast in both modes** even when the volume control is collapsed into overflow.
6. **Window-mode transition motion** — the signature physical morph (P1-D13 / §16.4). Standard ⇄ Mini-Player ⇄ Compact Music. Easing **CubicEase ease-out (entrances) / ease-in (exits)**, ~`cubic-bezier(0.25, 0.1, 0.25, 1)`. Durations: **120–200ms** typical, **up to 250ms** for the larger geometry reveals; hover/press **80–120ms** (reuse `OkMotionHover` 100ms, `OkMotionChrome` 180ms, `OkMotionPanel` 250ms). **P1-D13 is a Day-2 build priority — design the affordances now.** Note the lift: because the mini-player is now a **custom borderless window we own**, the geometry morph is **fully feasible** and the PRD's *"where the OS permits / where feasible"* hedge **no longer applies** — the locked-decision-2 architecture turns that caveat into a strength. **Never animate the video frame or seeking.** **Respect `prefers-reduced-motion`** — degrade to instant or cross-fade-only.

---

## Mini-Player (PiP) — §2.10

A small, **always-on-top**, **borderless rounded** floating window; **video fills it edge to edge**; controls are absent at rest and **hover-reveal**. This is a *pure video PiP* — no titlebar, no persistent chrome.

### Entry & exit
- **Entry affordance** mirrors music mode: design the **Window-submenu + right-click context-menu "Mini player" entry** (the shipped context menu already groups `Window → fullscreen / mini / fit / always-on-top`) and its **on/off (checked) state**. Selecting it triggers the **geometry-morph transition** into PiP (foundation 6).
- **Exit** via the same toggle, via the cluster's **expand-back** glyph, or via **double-click** (see Interactions) — all run the reverse morph back to Standard.

### Layout & geometry
- **Borderless rounded window** — define the corner radius (recommend the OSC-family radius, ~16–18px feel, tuned to window scale) and the **`OkShadow*` drop shadow** that lifts it off the desktop. Video is clipped to the rounded rect; no visible window border beyond a hairline if legibility demands it.
- **Default size, min-size floor, aspect lock derive from the real video aspect.** Do **not** hard-code a 16:9 box — the mode must handle 4:3, 2.39:1, and **vertical** video while keeping *video fills it edge to edge* (no letterboxing). Recommend a default sized so the **shorter edge is ~270px**, computed from the playing file's actual aspect ratio; apply the **min-size floor to the limiting (shorter) dimension — recommend ~160px**. Show at least one **non-16:9 (vertical) case** in the resize mockups.
- **Resize-initiation for a chromeless window (design this explicitly).** A borderless rounded window has no OS resize handles. Specify: **invisible edge/corner hit-zones** (recommend ~8px grab width), whether a **corner grip hover-reveals**, the **cursor change** on edge/corner hover, and how the **aspect-lock** is communicated *during* the drag (the window only grows along the locked ratio — show the visual). Resizing from any edge/corner maintains aspect; show the min-size floor being hit.
- **Snap-to-corner.** Design the snap affordance and its settle motion (gentle decelerate-into-place; reuse chrome easing). Show the four corner rests with a **screen-edge inset of ~16px (`OkSpace16`)**. Custom corner-snap is **Mini-Player-only**.

### Control cluster (hover-reveal)
On hover, a **minimal cluster fades in over the localized scrim** (foundation a), exactly: **play/pause · thin timeline · close · expand-back**. Volume optional inside the cluster or one tap into overflow — decide and justify for the smallest size. All non-essential controls live in the **overflow "…"** (foundation b). The cluster uses the **chrome reveal/hide motion** (`ChromeShowSb`/`ChromeHideSb` language: fade + few-px translate, 180–200ms).

**Glyph candidates (extend the shipped Segoe Fluent Icons set, keep outline=idle / filled-accent=active):** **close → `ChromeClose` `&#xE8BB;`**; **expand-back / restore-to-standard → `BackToWindow` `&#xE73F;`** (pairs with the in-use fullscreen `&#xE740;`). Draw to the same grid/stroke as the shipped glyphs (play `&#xE768;`, prev/next `&#xE892;`/`&#xE893;`, etc.).

### Interactions (full §15.1 video-plane map — resolve every gesture)
- **Drag on the video body → moves the window** (§15.1). The seekbar and buttons are the only non-drag regions — make that unmistakable.
- **Single click → play/pause**; the cluster's play button is the explicit affordance.
- **Double-click → expand-back to Standard** (NOT OS fullscreen). Justify the divergence from §15.1's *double-click → toggle fullscreen*: fullscreen and PiP are mutually exclusive (see Fullscreen), so in a PiP the useful destination is the standard window. Honor §15.1's **single-vs-double-click commit-after-interval tuning** so a double-click doesn't flash pause.
- **Mouse-wheel → stepped volume + volume OSD toast**, even when volume is collapsed into overflow.
- **Right-click → context menu** (overflow + Window controls).
- **Always-on-top is intrinsic** — no toggle inside the mini-player.
- **Subtitles in PiP + P1-D9.** Subtitles **render over the PiP video**; the hover-reveal cluster must honor **P1-D9 (OSC never overlaps subtitles)** — shift subtitles up when the cluster shows and return them smoothly on hide (no jump-cut), reusing the existing subtitle-margin mechanism (`SetSubtitleMargin`). The subtitle *quick-switch* control still lives in overflow.
- **Expand-back** → animated geometry morph back to standard window, restoring full OSC + titlebar (reverse of entry). Show the morph as a storyboard: window grows, video stays put, OSC expands from compact cluster to full pill.

### States (from §2.10 + the universal matrix — design every one)
- **Idle** — pure video, no chrome, cursor hidden after idle timeout.
- **Hover** — cluster fades in over scrim.
- **Playing / Paused** — glyph reflects state; **while paused, the cluster stays visible** (global visibility rule — idle-hide only while playing).
- **Resize** — aspect-locked, min-size floor enforced; resize-initiation affordance visible on edge hover.
- **Snap** — corner snap + settle, 16px inset.
- **Expand** — morph back to standard.
- **Fullscreen** — **N/A by design.** Mini-Player and fullscreen are **mutually exclusive** (per shipped MainWindow behavior, entering fullscreen exits mini-player); there is no fullscreen-within-PiP. State this rather than omit it.
- **Loading / buffering** — non-blocking indicator sized for a tiny window.
- **Error** — non-modal compact error card; user can still drag, close, or expand-back to open another file.

---

## Compact Music Mode — §2.11

An **audio-first** layout. The window reshapes to a **compact near-square or portrait**. This is the home for audio playback and for anyone who wants album-art-forward listening.

### Window chrome
Music Mode is a **real Mica-backed window**, not floating-over-video — so it needs an explicit window-control decision (unlike the borderless Mini-Player). **Recommend retaining the custom translucent Mica titlebar with integrated min/max/close** (§15.3) in a **reduced compact variant**: minimal height, media-title truncation, fused with the audio-first content. Specify how those controls coexist with the layout and the idle-hide rule (the titlebar may persist while transport idle-hides — see states). Justify if you drop them.

### Layout & sizing
Define a compact **near-square / portrait** window with a clear vertical rhythm on the **8px grid**:
- **Large album/embedded artwork** dominant (or static placeholder — see states). Specify artwork size relative to window and the safe inset.
- **Title / artist / album** — the now-playing **title is a hero**, so reach for **large legible type** (North Star): **`OkPanelHeaderTextStyle`** (18px) for the standard title, **`OkDisplayTextStyle`** for the prominent / no-artwork case. Reserve **`OkTitleTextStyle`** (16px SemiBold) / **`OkSmallTextStyle`** + secondary brush for the **artist/album** lines. Graceful truncation with ellipsis; never reflow-jitter.
- **Transport** — **prev · play/pause · next** (the compact cluster; prev/next present here since music implies a sequence).
- **Timeline + time** — thin seek with the **over-Mica themed treatment** (`OkAccentRailBrush` fill + themed track, per foundation b's substrate rule — **not** raw `OkSeek*`), and **`OkTimecodeTextStyle`** with tabular figures for elapsed/duration. No frame thumbnail on hover (audio-only) — time/chapter tooltip only.
- **Volume** — present (audio-first; do not bury it in overflow here).
- **Optional Up-Next mini-list** — the shared compact rows; collapsible. Design both the **with Up-Next** and **without** layouts.

This mode is **Mica-backed**: use **`OkSurfaceBaseBrush`** / **`OkSurfaceLayer1Brush`** and themed text brushes (`OkTextPrimaryBrush`, `OkTextSecondaryBrush`). Controls over the artwork use the **floating-control recipe** (`OkSeek*`, `OkOverVideo*`); controls over Mica use the **standard themed control styles**. Design both **Light and dark fully** — Auto means dark renders most evenings; it is not second-class.

### Window movement & snap
Compact Music Mode is an **ordinary movable/resizable window** that relies on **native Windows 11 snap layouts** — it does **not** use the Mini-Player's custom corner-snap.

### Entry & exit
- **Automatic** for audio-only files (`VideoWidth <= 0`).
- **Manual** via **Window submenu** and **right-click context menu** — design the menu entry + its on/off (checked) state.
- **"Switch to video view"** — when a video track plays while in music mode, surface a **calm, non-modal affordance** (recommend a small inline button near the artwork/title, not a blocking dialog). **Destination is explicit: morph to the Standard window with full OSC + titlebar restored** (video deserves full chrome) — specify the resulting geometry/reshape, the cross-fade + reshape motion (reduced-motion → instant), and that **music mode exits** on switch while the **Window-menu toggle remains available** to return.

### Interactions (full §15.1 disposition)
- **Single click on artwork → play/pause.**
- **Double-click on artwork → enter the fullscreen "now playing" variant** (see Fullscreen) if you commission it; otherwise no-op — state which. Reconcile with §15.1 and the commit-after-interval tuning.
- **Mouse-wheel → stepped volume + volume OSD toast.**
- **Right-click → context menu** (Window toggle, switch-to-video when applicable, etc.).

### Now-playing motion (subtle, optional, never a waveform)
Pick **one**: a gentle **artwork parallax / ken-burns drift** (slow, sub-pixel-smooth, pauses with playback) **or** a **soft accent pulse near the timeline**. Because the music timeline sits **over Mica**, the pulse uses the themed **`OkAccentRailBrush`** (or `OkAccentBrush`, which varies Light `#10938A` / Dark `#28B3AA`) — **not** the over-video `OkOverVideoAccentBrush` (reserve that for accents over the video plane / over artwork). Specify amplitude, period, and that it **honors `prefers-reduced-motion`** (off entirely). **No waveform, no audio-reactivity** — hard non-goal.

### States (from §2.11 + the universal matrix — design every one)
- **Has artwork** — embedded/generated art prominent.
- **No artwork** — **static styled placeholder**: generic music glyph on a soft gradient panel (teal-family tints; reuse the existing fallback-gradient sensibility). **Not** a waveform, not animated-from-audio. Title becomes more prominent (`OkDisplayTextStyle`) to compensate.
- **Playing / Paused** — transport reflects state; subtle motion runs only while playing; paused keeps controls visible.
- **Switch-to-video** — the affordance appears when video is detected; tapping morphs to Standard.
- **Fullscreen** — **explicit decision required, not omitted.** Either commission an **ambient full-screen "now playing" variant** (large artwork, hidden chrome, idle-hide transport, optional drift) reachable by double-click/menu, **or** scope it out with a one-line rationale. Record the decision.
- **Loading / buffering** — non-blocking; placeholder shimmer at most (not audio-reactive).
- **Error** — non-modal compact error card; can open another file.
- **Hover / active / idle** — controls follow the idle-hide rule (~2.5s while playing); decide whether **title + artwork persist** at idle while only transport hides (recommend: art + title persist, controls hide — justify). Resolve how the reduced titlebar participates (recommend: titlebar persists with content, transport idle-hides).

---

## Coverage checklist — honor every cell for each mode

The universal rule (§14.0): *do not ship a surface missing its loading and error states.* This is a **coverage recap**, not the primary spec — confirm each cell is designed for **each mode**:

| State | Mini-Player (PiP) | Compact Music Mode |
|---|---|---|
| **Idle** | pure video, cursor hidden | art + title persist, controls hidden (your call, justified) |
| **Active / hover** | cluster revealed over scrim | controls revealed |
| **Playing** | glyph = pause; idle-hide after ~2.5s | subtle motion on; idle-hide controls |
| **Paused** | cluster stays visible | controls stay visible; motion off |
| **Loading / buffering** | tiny non-blocking indicator | non-blocking; placeholder shimmer |
| **Error** | compact non-modal card | compact non-modal card |
| **Resize / floor** | aspect-locked from real ratio, min-size floor on shorter edge; chromeless resize hit-zones | native Win11 resize; reshape portrait ⇄ near-square |
| **Snap** | custom corner snap + settle, 16px inset | native Win11 snap only |
| **Fullscreen** | **N/A** — mutually exclusive with PiP (entering fullscreen exits mini-player) | ambient full-screen now-playing variant **or** explicit N/A + rationale |
| **Transition** | morph ⇄ Standard (Day-2 build; design now) | morph ⇄ Standard, ⇄ Standard on switch-to-video |
| **Subtitles (P1-D9)** | render over PiP; cluster shifts subs up, returns smoothly | less critical (audio-first); quick-switch in overflow |
| **No-subs / no-chapters** | overflow reflects absence gracefully | n/a / Up-Next reflects empty folder |
| **Reduced-motion** | morph → instant/crossfade | drift/pulse off; transitions instant |
| **Reduce-transparency / HighContrast** | opaque cluster fallback + HighContrast variant | opaque Mica-tint surfaces; HighContrast variant |
| **Light + dark** | scrim recipe theme-invariant; show both; themed fallbacks both | fully design both |

---

## Deliverables

1. **Design-system extensions** — any new `Ok*` tokens (brushes, radii, sizes, durations, and the **required `OkShadow*`/elevation token**) the compact modes need, named to match the existing convention and placed conceptually in `Colors.xaml` / `Brushes.xaml` / `Tokens.xaml` / `Typography.xaml` / `Controls/PlayerControls.xaml`. For each: value, theme behavior (Light/Dark/HighContrast or theme-invariant), and why nothing existing sufficed. Include the **tabular-figures fix to `OkTimecodeTextStyle`** and the **over-Mica seek treatment**. **Reuse before you add.**
2. **Mini-Player mockups** — all §2.10 + checklist states, at default size and at the min-size floor, **including a non-16:9 (vertical) case**, **over black letterbox, bright-snow, and a real frame**, in **Light and dark**. Include the hover-reveal cluster, overflow popover, snap, resize hit-zones, subtitle-shift, and the expand-back morph as a frame sequence.
3. **Compact Music Mode mockups** — all §2.11 + checklist states, in **near-square and portrait**, **with and without Up-Next**, **has-artwork and static-placeholder**, in **Light and dark**, plus the reduced titlebar, the "Switch to video view" affordance + layout-swap, and the fullscreen now-playing variant (or its recorded N/A).
4. **Shared-component sheet** — compact transport cluster, compact OSC reduction, localized scrim recipe (with the **three-background legibility proof** and the **reduce-transparency/HighContrast fallback**), `OkShadow*` token, overflow popover, compact Up-Next rows, compact OSD toasts (incl. per-mode seek-readout subset).
5. **Interaction / redline spec** — sizes, spacings (8px grid), radii, hit-targets (≥20px on the thin timeline), the resize grab-width and cursor map, hover/pressed/focus/disabled per control (keyboard focus visuals required — keyboard-driven app), motion durations + easing per transition, and the snap/resize/floor numbers (shorter-edge default ~270px, floor ~160px, inset 16px). Cite token names throughout, including the **named over-video-vs-over-Mica seek decision**.
6. **Rationale** — the load-bearing choices: localized-scrim approach + opaque fallback, overflow threshold (~360px), default + floor sizes derived from real aspect, double-click semantics, which now-playing motion and why, toast-in-tiny-window rule, switch-to-video destination, fullscreen dispositions, idle-persistence decision in music mode.

## Suggested order

1. **Foundation (a)** — localized floating-control material + reduce-transparency/HighContrast fallback + `OkShadow*`; prove legibility over the three backgrounds.
2. **Foundation (b)** — compact OSC reduction + shared transport cluster + substrate-split seek treatment + timecode style.
3. **Mini-Player** — geometry (real-aspect default/floor), entry/exit, hover-reveal, drag/double-click/wheel, chromeless resize, snap, subtitle-shift, expand-back morph; all states.
4. **Compact Music Mode** — chrome, layout, artwork vs placeholder, Up-Next, entry/exit + switch-to-video target, fullscreen decision, subtle motion; all states.
5. **Shared toasts + transition motion** — tie both modes together.
6. **Redline + rationale.**

---

## Constraints & non-goals

- **Windows 11 only.** Free to use Mica, WinUI 3 / Windows App SDK. No Windows 10 fallbacks.
- **libmpv render API** — controls composite *over* a single opaque video plane; **never** put Mica or Acrylic *on* the video frame.
- **Mica subtle, not glassy.** No aggressive Acrylic blur, no stacked blurs for depth — shadow + one-step material/tint shift, Paste-style. Honor **reduce-transparency / HighContrast** with solid Mica-tint fallbacks.
- **No real-time waveform or audio-reactive visual in music mode — ever.** Static placeholder only.
- **No macOS chrome** — no traffic-lights, no segmented controls, no SF Pro / SF Symbols / emoji as icons.
- **Iconography = `Segoe Fluent Icons` (as shipped — the Win11 system symbol font), not the PRD's aspirational "Fluent System Icons" library.** Reuse the existing in-use glyphs (play `&#xE768;`, prev/next `&#xE892;`/`&#xE893;`, subtitle `&#xE7F0;`, audio `&#xE8D6;`, chapters `&#xE8FD;`, screenshot `&#xE722;`, fullscreen `&#xE740;`); draw any new compact glyphs (close `&#xE8BB;`, expand-back `&#xE73F;`, switch-to-video) **to that grid/stroke**. Keep the **outline = idle, filled/accent = active** state convention. Extend the shipped font — do not spec from the open-source library.
- **Type = `Segoe UI Variable`** via the shipped ramp; **timecodes use `OkTimecodeTextStyle` with tabular figures** (`OkMonoFontFamily` fallback).
- **Teal accent** (`OkAccent*` over Mica, `OkOverVideoAccentBrush` over video/artwork — keep them on the right substrate).
- **No media library, no playlist/queue surface, no scripting, no upscaling/shader UI, no touchpad gestures** — all out of scope or hard non-goals.
- **No jank.** Layout pops, animation stutter, or a control that goes illegible over a bright frame are **P1 defects against the mac-grade-polish bar.** Restraint over feature-density; calm over busy; Pillar 1 wins.
