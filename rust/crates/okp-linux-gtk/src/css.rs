use super::*;

/// Corner radius of the rounded OK Player shell, logical px. The stylesheet
/// below states the same number literally (CSS is a static string); the
/// `shell_radius_matches_the_stylesheet` test pins the two together, and the
/// native video plane cuts matching transparent corner arcs (#778).
pub(crate) const SHELL_CORNER_RADIUS_PX: i32 = 14;

pub(crate) const OKP_STYLESHEET: &str = "
        /* Design tokens: the BeFeast palette, one coherent OK Player shell.
           Light surfaces lead with the identity navy as the interactive
           colour; everything that sits over the video plane, and the dark
           scheme, leads with the mark's cyan, because navy vanishes on navy. Every state colour below derives from these bases
           via alpha()/mix(), so the whole shell retints from a single edit.
           Green is reserved for the semantic ok state; there is no stray adwaita
           blue and no decorative teal. */
        @define-color okp_bg #05080f;
        @define-color okp_accent #39bef9;
        @define-color okp_accent_bright #7dd8ff;
        @define-color okp_accent_deep #1aa9ec;
        @define-color okp_accent_ink #061428;
        @define-color okp_navy #163065;
        @define-color okp_navy_hover #1d3f82;
        @define-color okp_navy_deep #0f2349;
        @define-color okp_blue #2563eb;
        @define-color okp_link #0284c7;
        @define-color okp_light_bg #f5f7fb;
        @define-color okp_light_card #ffffff;
        @define-color okp_light_rail #eef2f8;
        @define-color okp_light_inset #e3e9f2;
        @define-color okp_light_hairline #dfe5ee;
        @define-color okp_light_hairline_hi #c7d1de;
        @define-color okp_settings_light #f5f7fb;
        @define-color okp_settings_dark #070f22;
        @define-color okp_dark_card #0c1a38;
        @define-color okp_dark_rail #122549;
        @define-color okp_dark_inset #183160;
        @define-color okp_dark_hairline rgba(96, 144, 212, 0.20);
        @define-color okp_dark_hairline_hi rgba(96, 144, 212, 0.40);
        @define-color okp_ink #0c1424;
        @define-color okp_ink_dim #2a3547;
        @define-color okp_ink_mute #4b586c;
        @define-color okp_ink_faint #7f8a9c;
        @define-color okp_snow #e9f0fc;
        @define-color okp_snow_dim #c7d5ee;
        @define-color okp_snow_mute #98aecf;
        @define-color okp_snow_faint #5d7799;
        @define-color okp_ok #059669;
        @define-color okp_ok_dark #4ade80;
        @define-color okp_danger #b91c1c;
        /* A fill, not a text colour: the destructive hover behind white glyphs. */
        @define-color okp_danger_dark #dc2626;
        @define-color okp_danger_bright #fb7185;
        @define-color okp_warning #c2410c;
        @define-color okp_warning_dark #fbbf24;

        .okp-root {
            background: @okp_bg;
        }

        .okp-root.okp-native-video.has-active-video-plane,
        window.okp-player-window.okp-native-video.has-active-video-plane,
        .okp-video-plane.okp-native-video {
            background: transparent;
        }

        window.okp-player-window {
            background: @okp_bg;
            font-family: 'Noto Sans', 'Inter', 'Cantarell', sans-serif;
        }

        /* OK Player rounds its own windowed shell. The toplevel is undecorated,
           so GTK draws no decoration node and contributes no radius of its own:
           without this rule the window is square on every desktop, and where a
           desktop rounds it compositor-side the square video plane below shows
           through the rounding (#778). Maximized and fullscreen keep square
           corners - GTK sets those style classes itself. Keep the radius equal
           to SHELL_CORNER_RADIUS_PX. */
        window.okp-player-window:not(.maximized):not(.fullscreen),
        window.okp-player-window:not(.maximized):not(.fullscreen) .okp-root {
            border-radius: 14px;
        }

        window.okp-player-window.is-compact-mode,
        window.okp-player-window.is-compact-mode .okp-root {
            /* Keep in sync with COMPACT_SHELL_CORNER_RADIUS in mpv_bridge.rs:
               the native video plane cuts matching transparent corner arcs. */
            border-radius: 14px;
            background: @okp_bg;
        }

        /* The native Wayland video is a subsurface below GTK's parent surface.
           Compact chrome must keep that parent transparent or it paints an
           opaque black plane over paused and playing video. */
        window.okp-player-window.okp-native-video.is-compact-mode,
        window.okp-player-window.okp-native-video.is-compact-mode .okp-root.okp-native-video {
            background: transparent;
        }

        window.okp-player-window.is-compact-mode {
            box-shadow: 0 24px 64px rgba(0, 0, 0, 0.45);
        }

        .okp-window-chrome {
            min-height: 42px;
            background: transparent;
        }

        .okp-window-title-scrim {
            min-height: 42px;
            background: linear-gradient(to bottom, rgba(7, 15, 34, 0.55), rgba(7, 15, 34, 0));
        }

        .okp-window-drag-zone {
            min-height: 42px;
            background: transparent;
        }

        .okp-player-window-controls {
            min-height: 42px;
            background: transparent;
        }

        .okp-player-window-controls button,
        button.okp-player-window-control {
            min-width: 46px;
            min-height: 42px;
            padding: 0;
            border: none;
            border-radius: 0;
            background: transparent;
            box-shadow: none;
            color: rgba(255, 255, 255, 0.84);
            font-size: 15px;
            font-weight: 400;
        }

        .okp-window-media-icon {
            color: rgba(255, 255, 255, 0.90);
        }

        .okp-window-media-title {
            color: rgba(255, 255, 255, 0.95);
            font-size: 12.5px;
            font-weight: 500;
        }

        .okp-top-chrome-motion {
            opacity: 1;
            transform: translate(0, 0);
            transition: opacity 180ms ease, transform 180ms ease;
        }

        .okp-top-chrome-motion.is-hidden {
            opacity: 0;
            transform: translate(0, -10px);
        }

        .okp-root.is-reduced-motion .okp-top-chrome-motion,
        .okp-root.is-reduced-motion .okp-chrome-revealer {
            transition: none;
        }

        .okp-player-window-controls button:hover,
        button.okp-player-window-control:hover {
            background: rgba(255, 255, 255, 0.12);
            color: rgba(255, 255, 255, 0.96);
        }

        label.okp-player-window-control-glyph {
            color: rgba(255, 255, 255, 0.86);
            font-size: 15px;
            font-weight: 400;
        }

        button.okp-player-window-control:hover label.okp-player-window-control-glyph {
            color: rgba(255, 255, 255, 0.98);
        }

        .okp-player-window-controls button:active,
        button.okp-player-window-control:active {
            background: rgba(255, 255, 255, 0.18);
        }

        button.okp-player-window-pin.is-selected {
            background: alpha(@okp_accent, 0.22);
            color: @okp_accent_bright;
        }

        .okp-player-window-controls button:focus-visible,
        button.okp-player-window-control:focus-visible {
            outline: none;
            box-shadow: 0 0 0 3px alpha(@okp_accent, 0.30);
        }

        button.okp-player-window-close:hover {
            background: alpha(@okp_danger_dark, 0.86);
            color: #ffffff;
        }

        .okp-resize-handle {
            background: transparent;
        }

        .okp-resize-edge-horizontal {
            min-height: 6px;
        }

        .okp-resize-edge-vertical {
            min-width: 6px;
        }

        .okp-resize-corner {
            min-width: 16px;
            min-height: 16px;
        }

        .okp-resize-edge-horizontal.is-compact {
            min-height: 8px;
        }

        .okp-resize-edge-vertical.is-compact {
            min-width: 8px;
        }

        .okp-video-plane {
            background: @okp_bg;
        }

        .okp-empty-surface {
            background: alpha(@okp_bg, 0.97);
        }

        .okp-empty-surface.is-preview-substrate {
            background: @okp_bg;
        }

        .okp-empty-surface.is-preview-substrate.is-preview-bright {
            background: @okp_light_bg;
        }
        .okp-empty-surface.has-media {
            background: transparent;
        }
        /* Playback-state overlays stay in-canvas and never replace the player with
         * a modal. Loading/paused do not capture input; the error card enables only
         * its own recovery actions. */
        .okp-media-state-overlay {
            background: transparent;
        }

        .okp-paused-cue {
            padding: 6px 14px;
            border-radius: 999px;
            background: rgba(12, 20, 36, 0.56);
            border: 1px solid rgba(255, 255, 255, 0.12);
            color: rgba(255, 255, 255, 0.88);
            font-size: 11px;
            font-weight: 600;
            letter-spacing: 0.05em;
            text-transform: uppercase;
        }

        .okp-loading-state {
            color: rgba(255, 255, 255, 0.88);
        }

        .okp-loading-ring {
            min-width: 34px;
            min-height: 34px;
            color: @okp_accent;
        }

        .okp-loading-label {
            color: rgba(255, 255, 255, 0.72);
            font-size: 12px;
            font-weight: 600;
        }

        .okp-error-card {
            min-width: 340px;
            padding: 22px 24px;
            border-radius: 16px;
            background: rgba(12, 20, 36, 0.90);
            border: 1px solid rgba(255, 255, 255, 0.12);
            box-shadow: 0 4px 12px rgba(0, 0, 0, 0.18), 0 24px 60px rgba(0, 0, 0, 0.28);
        }

        .okp-error-icon {
            color: @okp_danger_bright;
            -gtk-icon-size: 24px;
        }

        .okp-error-title {
            color: rgba(255, 255, 255, 0.96);
            font-size: 17px;
            font-weight: 600;
            letter-spacing: -0.01em;
        }

        .okp-error-body {
            color: rgba(255, 255, 255, 0.66);
            font-size: 12.5px;
        }

        button.okp-error-primary,
        button.okp-error-secondary {
            min-height: 34px;
            padding: 0 18px;
            border-radius: 999px;
            box-shadow: none;
            font-size: 13px;
            font-weight: 600;
        }

        button.okp-error-primary {
            background: linear-gradient(135deg, @okp_blue, @okp_accent 60%, @okp_accent_bright);
            border: 1px solid transparent;
            color: @okp_accent_ink;
        }

        button.okp-error-secondary {
            background: rgba(255, 255, 255, 0.08);
            border: 1px solid rgba(255, 255, 255, 0.12);
            color: rgba(255, 255, 255, 0.88);
        }

        button.okp-error-primary:disabled {
            background: rgba(255, 255, 255, 0.08);
            border-color: rgba(255, 255, 255, 0.08);
            color: rgba(255, 255, 255, 0.34);
        }

        .okp-media-state-text {
            padding: 10px 22px;
            border-radius: 999px;
            background: rgba(12, 20, 36, 0.72);
            border: 1px solid rgba(255, 255, 255, 0.12);
            color: rgba(255, 255, 255, 0.92);
            font-size: 14px;
            font-weight: 600;
        }

        .okp-empty-actions {
            margin-top: 26px;
        }

        .okp-empty-primary-button,
        .okp-empty-secondary-button {
            min-height: 42px;
            padding: 0 24px;
            border-radius: 999px;
            border: 1px solid transparent;
            box-shadow: none;
            font-size: 14px;
            font-weight: 600;
        }

        .okp-empty-primary-button {
            background: linear-gradient(135deg, @okp_blue, @okp_accent 60%, @okp_accent_bright);
            color: @okp_accent_ink;
            box-shadow: 0 1px 2px alpha(@okp_navy, 0.30);
        }

        .okp-empty-primary-button:hover {
            background: linear-gradient(135deg, @okp_blue, @okp_accent_bright 60%, @okp_accent_bright);
        }

        .okp-empty-primary-button:active {
            background: @okp_accent_deep;
        }

        .okp-empty-primary-button:focus-visible,
        .okp-empty-secondary-button:focus-visible {
            outline: none;
            box-shadow: 0 0 0 3px alpha(@okp_accent, 0.30);
        }

        .okp-empty-secondary-button {
            background: rgba(255, 255, 255, 0.09);
            border-color: rgba(255, 255, 255, 0.14);
            color: rgba(255, 255, 255, 0.90);
        }

        .okp-empty-secondary-button:hover {
            background: rgba(255, 255, 255, 0.11);
            color: rgba(255, 255, 255, 0.96);
        }

        .okp-empty-secondary-button:active {
            background: rgba(255, 255, 255, 0.15);
        }

        .okp-empty-hint {
            margin-top: 20px;
            color: rgba(255, 255, 255, 0.52);
            font-size: 11.5px;
        }

        .okp-welcome-scroller,
        .okp-welcome-scroller > viewport {
            background: transparent;
        }

        .okp-welcome-first-run,
        .okp-welcome-private {
            min-width: 320px;
        }

        /* One Continue-watching card, which is the widest thing on this surface that cannot
         * be reflowed - everything else wraps or ellipsizes. A wider floor than that is a
         * minimum window width in disguise: the canvas gets it allocated inside a narrower
         * window and the overflow is cropped rather than compressed (#716). */
        .okp-welcome-recents {
            min-width: 194px;
        }

        .okp-welcome-recents-heading {
            min-height: 48px;
        }

        .okp-recents-mark {
            margin-top: 2px;
        }

        .okp-welcome-recents-title {
            color: rgba(255, 255, 255, 0.97);
            font-size: 22px;
            font-weight: 600;
            letter-spacing: -0.015em;
        }

        .okp-welcome-recents-subtitle {
            margin-top: 4px;
            color: rgba(255, 255, 255, 0.70);
            font-size: 13px;
        }

        .okp-recents-shelf {
            margin-top: 18px;
        }

        .okp-recents-shelf > flowboxchild {
            padding: 0;
            border: none;
            background: transparent;
        }

        button.okp-recent-card {
            min-width: 194px;
            padding: 0;
            border: none;
            border-radius: 12px;
            background: transparent;
            box-shadow: none;
            color: rgba(255, 255, 255, 0.94);
        }

        button.okp-recent-card:focus-visible {
            outline: none;
            box-shadow: 0 0 0 3px alpha(@okp_accent, 0.30);
        }

        .okp-history-thumbnail {
            border-radius: 8px;
            background: rgba(255, 255, 255, 0.05);
        }

        .okp-history-thumbnail-placeholder {
            border-radius: 8px;
            background: linear-gradient(135deg, @okp_dark_inset, @okp_dark_card);
            border: 1px solid rgba(255, 255, 255, 0.07);
        }

        .okp-history-thumbnail-icon {
            color: rgba(255, 255, 255, 0.30);
        }

        .okp-history-thumbnail-picture {
            border-radius: 8px;
        }

        progressbar.okp-recent-progress {
            min-height: 4px;
            margin: 0;
        }

        progressbar.okp-recent-progress trough {
            min-height: 4px;
            border: none;
            border-radius: 0 0 8px 8px;
            background: rgba(255, 255, 255, 0.24);
        }

        progressbar.okp-recent-progress progress {
            min-width: 0;
            min-height: 4px;
            border: none;
            border-radius: 0;
            background: @okp_accent_bright;
        }

        .okp-recent-time-left {
            padding: 2px 8px;
            border-radius: 999px;
            background: rgba(12, 20, 36, 0.72);
            color: #ffffff;
            font-family: 'JetBrains Mono', 'Cascadia Mono', monospace;
            font-size: 11px;
            font-weight: 600;
            font-feature-settings: 'tnum';
        }

        .okp-recent-title {
            margin-top: 8px;
            color: rgba(255, 255, 255, 0.94);
            font-size: 13px;
            font-weight: 650;
        }

        .okp-recent-location {
            margin-top: 2px;
            color: rgba(255, 255, 255, 0.60);
            font-size: 11px;
        }

        .okp-recent-context {
            margin-top: 3px;
            color: rgba(255, 255, 255, 0.68);
            font-size: 11px;
        }

        .okp-welcome-footer {
            min-height: 38px;
            margin-top: 18px;
            padding-top: 10px;
            border-top: 1px solid rgba(255, 255, 255, 0.08);
        }

        button.okp-welcome-footer-button {
            min-height: 30px;
            padding: 4px 10px;
            border: none;
            border-radius: 999px;
            background: transparent;
            box-shadow: none;
            color: rgba(255, 255, 255, 0.62);
        }

        button.okp-welcome-footer-button:hover {
            background: rgba(255, 255, 255, 0.08);
            color: rgba(255, 255, 255, 0.90);
        }

        .okp-welcome-footer-status {
            color: rgba(255, 255, 255, 0.54);
            font-size: 11px;
        }

        .okp-welcome-private-icon {
            margin-bottom: 12px;
            color: @okp_accent;
        }

        .okp-welcome-private-title {
            color: rgba(255, 255, 255, 0.96);
            font-size: 22px;
            font-weight: 600;
            letter-spacing: -0.015em;
        }

        .okp-welcome-private-body {
            margin-top: 8px;
            margin-bottom: 20px;
            color: rgba(255, 255, 255, 0.56);
            font-size: 13px;
            line-height: 1.45;
        }

        .okp-welcome-private .okp-empty-actions {
            margin-top: 12px;
        }

        .okp-history-page {
            background: transparent;
        }

        .okp-history-title {
            color: rgba(255, 255, 255, 0.97);
            font-size: 22px;
            font-weight: 600;
            letter-spacing: -0.015em;
        }

        entry.okp-history-search {
            min-width: 220px;
            min-height: 36px;
            border-radius: 8px;
            border: 1px solid rgba(255, 255, 255, 0.10);
            background: rgba(255, 255, 255, 0.06);
            color: rgba(255, 255, 255, 0.92);
            box-shadow: none;
        }

        entry.okp-history-search:focus-within {
            border-color: @okp_accent;
            box-shadow: 0 0 0 3px alpha(@okp_accent, 0.30);
        }

        .okp-history-subtitle {
            margin-top: 6px;
            color: rgba(255, 255, 255, 0.48);
            font-size: 13px;
        }

        .okp-history-private-banner {
            margin-top: 14px;
            padding: 10px 12px;
            border-radius: 8px;
            border: 1px solid rgba(255, 255, 255, 0.08);
            background: rgba(255, 255, 255, 0.045);
            color: rgba(255, 255, 255, 0.58);
            font-size: 12px;
        }

        .okp-history-result-caption {
            margin-top: 18px;
            margin-bottom: 8px;
            color: rgba(255, 255, 255, 0.40);
            font-size: 11px;
            font-weight: 650;
        }

        .okp-history-scroller,
        .okp-history-scroller > viewport {
            background: transparent;
        }

        .okp-history-rows {
            padding-bottom: 18px;
        }

        button.okp-history-row {
            padding: 9px 10px;
            border: none;
            border-radius: 8px;
            background: transparent;
            box-shadow: none;
            color: rgba(255, 255, 255, 0.92);
        }

        button.okp-history-row:hover {
            background: rgba(255, 255, 255, 0.06);
        }

        button.okp-history-row:focus-visible {
            outline: none;
            box-shadow: inset 0 0 0 3px alpha(@okp_accent, 0.30);
        }

        .okp-history-row-title {
            color: rgba(255, 255, 255, 0.92);
            font-size: 13px;
            font-weight: 650;
        }

        .okp-history-row-location {
            color: rgba(255, 255, 255, 0.39);
            font-size: 11px;
        }

        .okp-history-row-context,
        .okp-history-row-progress-label {
            color: rgba(255, 255, 255, 0.48);
            font-size: 11px;
        }

        .okp-history-row-progress-label {
            color: @okp_accent;
            font-family: 'JetBrains Mono', 'Cascadia Mono', monospace;
            font-weight: 600;
        }

        progressbar.okp-history-row-progress {
            min-height: 3px;
            margin-top: 2px;
        }

        progressbar.okp-history-row-progress trough {
            min-height: 3px;
            border: none;
            border-radius: 3px;
            background: rgba(255, 255, 255, 0.10);
        }

        progressbar.okp-history-row-progress progress {
            min-width: 0;
            min-height: 3px;
            border: none;
            border-radius: 3px;
            background: @okp_accent;
        }

        .okp-history-empty {
            min-height: 260px;
            color: rgba(255, 255, 255, 0.30);
        }

        .okp-history-empty-title {
            color: rgba(255, 255, 255, 0.56);
            font-size: 14px;
            font-weight: 650;
        }

        /* Canonical idle canvas and History takeover. The idle shell is neutral,
         * theme-aware, and full-window; playback remains on the dark video plane. */
        .okp-empty-surface {
            background: transparent;
        }

        .okp-idle-canvas {
            font-family: 'Noto Sans', 'Inter', 'Cantarell', sans-serif;
        }

        .okp-idle-canvas.is-light {
            background: linear-gradient(to bottom, #ffffff, @okp_light_bg 240px);
            color: @okp_ink;
        }

        .okp-idle-canvas.is-dark {
            background: linear-gradient(to bottom, #0a1630, @okp_settings_dark 240px);
            color: @okp_snow;
        }

        .okp-idle-canvas.is-preview-substrate {
            background: @okp_bg;
        }

        .okp-idle-canvas.is-preview-substrate.is-preview-bright {
            background: @okp_light_bg;
        }

        .okp-idle-titlebar {
            min-height: 34px;
            padding-left: 15px;
        }

        .is-light .okp-idle-titlebar-mark { color: @okp_navy; }
        .is-dark .okp-idle-titlebar-mark { color: @okp_accent; }

        .okp-idle-titlebar-text {
            font-size: 12.5px;
            font-weight: 600;
            letter-spacing: -0.01em;
        }

        .is-light .okp-idle-titlebar-text { color: @okp_ink; }
        .is-dark .okp-idle-titlebar-text { color: @okp_snow; }

        .okp-idle-stack,
        .okp-idle-scroller,
        .okp-idle-scroller > viewport,
        .okp-history-scroller,
        .okp-history-scroller > viewport {
            background: transparent;
        }

        .okp-brand-tile {
            padding: 0;
            border-radius: 11px;
            background: transparent;
            box-shadow: 0 8px 22px alpha(@okp_navy, 0.24);
        }

        .okp-welcome-first-run {
            padding: 34px 24px;
        }

        .okp-welcome-brand-tile { margin-bottom: 13px; }

        .okp-first-run-title {
            font-family: 'Noto Sans', 'Inter', 'Cantarell', sans-serif;
            font-size: 22px;
            font-weight: 600;
            letter-spacing: -0.015em;
        }

        .is-light .okp-first-run-title { color: @okp_ink; }
        .is-dark .okp-first-run-title { color: @okp_snow; }

        .okp-first-run-copy {
            margin-top: 6px;
            font-size: 13px;
        }

        .is-light .okp-first-run-copy { color: @okp_ink_mute; }
        .is-dark .okp-first-run-copy { color: @okp_snow_mute; }

        button.okp-first-run-drop-target,
        button.okp-welcome-drop-target {
            border: 1px dashed @okp_light_hairline_hi;
            box-shadow: 0 1px 0 rgba(12, 20, 36, 0.04), 0 8px 24px rgba(12, 20, 36, 0.05);
        }

        button.okp-first-run-drop-target {
            min-width: 280px;
            min-height: 70px;
            margin-top: 13px;
            padding: 12px 16px;
            border-radius: 16px;
        }

        /* The welcome drop target is allocated explicitly by the action row, so its width
         * at the canonical desktop layout comes from that allocation and not from this
         * floor. Keeping the first-run target's 280px here instead would make this button
         * the widest thing on the idle canvas and put a 378px floor under the whole window
         * - wider than a portrait fit on a 1366x768 laptop, which is how the canvas came to
         * be cropped inside its own window (#716). */
        button.okp-welcome-drop-target {
            min-width: 148px;
            min-height: 82px;
            padding: 0 16px;
            border-radius: 12px;
        }

        .is-light button.okp-first-run-drop-target,
        .is-light button.okp-welcome-drop-target {
            background: @okp_light_card;
            color: @okp_ink_mute;
        }

        .is-dark button.okp-first-run-drop-target,
        .is-dark button.okp-welcome-drop-target {
            background: @okp_dark_card;
            border-color: @okp_dark_hairline_hi;
            box-shadow: none;
            color: @okp_snow_mute;
        }

        .okp-empty-surface.is-drop-target button.okp-first-run-drop-target,
        .okp-empty-surface.is-drop-target button.okp-welcome-drop-target {
            border-color: @okp_accent;
            background: alpha(@okp_accent, 0.12);
        }

        .okp-drop-primary {
            font-size: 13px;
            font-weight: 600;
        }

        .is-light .okp-first-run-drop-target .okp-drop-primary { color: @okp_navy; }
        .is-dark .okp-first-run-drop-target .okp-drop-primary { color: @okp_accent; }

        .okp-drop-secondary {
            font-size: 12px;
        }

        .okp-welcome-recents {
            padding: 34px 32px 0;
        }

        .okp-welcome-recents-title {
            font-family: 'Noto Sans', 'Inter', 'Cantarell', sans-serif;
            font-size: 22px;
            font-weight: 600;
            letter-spacing: -0.015em;
        }

        .is-light .okp-welcome-recents-title { color: @okp_ink; }
        .is-dark .okp-welcome-recents-title { color: @okp_snow; }

        .okp-welcome-recents-subtitle {
            margin-top: 4px;
            font-size: 13px;
        }

        .is-light .okp-welcome-recents-subtitle { color: @okp_ink_mute; }
        .is-dark .okp-welcome-recents-subtitle { color: @okp_snow_mute; }

        .okp-recents-shelf {
            margin-top: 20px;
        }

        .okp-recents-shelf > flowboxchild {
            padding: 0;
            border: none;
            background: transparent;
        }

        /* The card keeps the 194px poster width the shelf is laid out by, so the
           three cards and the round History button still share one row: the
           poster bleeds to the card's edges, the hairline is a spread shadow
           rather than a border, and only the caption is inset. */
        button.okp-recent-card {
            min-width: 194px;
            padding: 0 0 12px;
            border: none;
            border-radius: 12px;
            background: @okp_light_card;
            box-shadow: 0 0 0 1px @okp_light_hairline, 0 1px 2px rgba(12, 20, 36, 0.04), 0 12px 32px rgba(12, 20, 36, 0.06);
        }

        button.okp-recent-card .okp-history-thumbnail,
        button.okp-recent-card .okp-history-thumbnail-placeholder,
        button.okp-recent-card .okp-history-thumbnail-picture {
            border-radius: 12px 12px 0 0;
        }

        button.okp-recent-card progressbar.okp-recent-progress trough {
            border-radius: 0;
        }

        button.okp-recent-card .okp-recent-title,
        button.okp-recent-card .okp-recent-location {
            margin-left: 10px;
            margin-right: 10px;
        }

        button.okp-recents-history-button {
            min-width: 36px;
            min-height: 36px;
            padding: 0;
            border: 1px solid @okp_light_hairline;
            border-radius: 18px;
            box-shadow: 0 1px 0 rgba(12, 20, 36, 0.04), 0 8px 24px rgba(12, 20, 36, 0.05);
        }

        .is-light button.okp-recents-history-button {
            background: @okp_light_card;
            color: @okp_ink_mute;
        }

        .is-dark button.okp-recents-history-button {
            background: @okp_dark_card;
            border-color: @okp_dark_hairline;
            box-shadow: none;
            color: @okp_snow_mute;
        }

        .is-light button.okp-recents-history-button:hover { background: @okp_light_rail; color: @okp_ink; }
        .is-dark button.okp-recents-history-button:hover { background: @okp_dark_rail; color: @okp_snow; }

        .is-light button.okp-recent-card { color: @okp_ink; }
        .is-dark button.okp-recent-card { color: @okp_snow; background: @okp_dark_card; box-shadow: 0 0 0 1px @okp_dark_hairline; }

        .is-light button.okp-recent-card:hover { box-shadow: 0 0 0 1px @okp_light_hairline_hi, 0 1px 2px rgba(12, 20, 36, 0.04), 0 12px 32px rgba(12, 20, 36, 0.06); }
        .is-dark button.okp-recent-card:hover { box-shadow: 0 0 0 1px @okp_dark_hairline_hi; }

        .okp-history-thumbnail { border-radius: 8px; }

        .is-light .okp-history-thumbnail-placeholder {
            background: @okp_light_inset;
            border: 1px solid @okp_light_hairline;
        }

        .is-dark .okp-history-thumbnail-placeholder {
            background: @okp_dark_inset;
            border: 1px solid @okp_dark_hairline;
        }

        .okp-history-thumbnail.is-finished { opacity: 0.55; }

        progressbar.okp-recent-progress,
        progressbar.okp-history-thumb-progress {
            min-height: 4px;
            margin: 0;
        }

        progressbar.okp-recent-progress { min-width: 4px; }
        progressbar.okp-history-thumb-progress { min-width: 0; }

        progressbar.okp-recent-progress trough,
        progressbar.okp-history-thumb-progress trough {
            min-width: 0;
            min-height: 4px;
            border: none;
            border-radius: 0 0 8px 8px;
            background: rgba(255, 255, 255, 0.50);
        }

        progressbar.okp-recent-progress progress,
        progressbar.okp-history-thumb-progress progress {
            min-width: 0;
            min-height: 4px;
            border: none;
            background: linear-gradient(to right, @okp_navy, @okp_blue 55%, @okp_accent);
        }

        .is-dark progressbar.okp-recent-progress progress,
        .is-dark progressbar.okp-history-thumb-progress progress {
            background: linear-gradient(to right, @okp_blue, @okp_accent 60%, @okp_accent_bright);
        }

        .okp-recent-time-left {
            padding: 2px 8px;
            border-radius: 999px;
            background: rgba(12, 20, 36, 0.72);
            color: #ffffff;
            font-family: 'JetBrains Mono', 'Cascadia Mono', monospace;
            font-size: 11px;
            font-weight: 600;
            font-feature-settings: 'tnum';
        }

        .okp-recent-title {
            margin-top: 10px;
            font-size: 13px;
            font-weight: 600;
        }

        .is-light .okp-recent-title { color: @okp_ink; }
        .is-dark .okp-recent-title { color: @okp_snow; }

        .okp-recent-location {
            margin-top: 2px;
            font-size: 12px;
        }

        .is-light .okp-recent-location { color: @okp_ink_faint; }
        .is-dark .okp-recent-location { color: @okp_snow_faint; }

        .okp-welcome-action-row {
            margin-top: 24px;
        }

        .okp-welcome-action-column {
            min-width: 132px;
            min-height: 84px;
        }

        button.okp-idle-primary-button,
        button.okp-idle-secondary-button {
            min-height: 36px;
            padding: 0 16px;
            border-radius: 999px;
            box-shadow: none;
            font-size: 13px;
            font-weight: 600;
        }

        button.okp-idle-primary-button {
            background: linear-gradient(135deg, @okp_navy, @okp_blue 55%, @okp_accent);
            border: 1px solid transparent;
            color: #ffffff;
            box-shadow: 0 1px 2px alpha(@okp_navy, 0.30);
        }

        button.okp-idle-primary-button:hover { background: linear-gradient(135deg, @okp_navy_hover, @okp_blue 55%, @okp_accent_bright); }
        .is-dark button.okp-idle-primary-button { background: linear-gradient(135deg, @okp_blue, @okp_accent 60%, @okp_accent_bright); color: @okp_accent_ink; }
        .is-dark button.okp-idle-primary-button:hover { background: linear-gradient(135deg, @okp_blue, @okp_accent_bright 60%, @okp_accent_bright); }

        .is-light button.okp-idle-secondary-button {
            background: @okp_light_card;
            border: 1px solid @okp_light_hairline_hi;
            color: @okp_ink;
        }

        .is-dark button.okp-idle-secondary-button {
            background: @okp_dark_card;
            border: 1px solid @okp_dark_hairline_hi;
            color: @okp_snow;
        }

        .is-light button.okp-idle-secondary-button:hover { background: @okp_light_rail; }
        .is-dark button.okp-idle-secondary-button:hover { background: @okp_dark_rail; }
        button.okp-idle-primary-button:focus-visible,
        button.okp-idle-secondary-button:focus-visible {
            outline: none;
            box-shadow: 0 0 0 3px alpha(@okp_accent, 0.30);
        }

        .okp-welcome-private {
            padding: 36px 24px;
        }

        .is-light .okp-private-hero-icon { color: @okp_navy; }
        .is-dark .okp-private-hero-icon { color: @okp_accent; }

        .okp-private-hero-title {
            margin-top: 12px;
            font-size: 22px;
            font-weight: 600;
            letter-spacing: -0.015em;
        }

        .okp-private-hero-copy {
            margin-top: 7px;
            font-size: 13px;
            line-height: 1.5;
        }

        .is-light .okp-private-hero-copy { color: @okp_ink_mute; }
        .is-dark .okp-private-hero-copy { color: @okp_snow_mute; }

        .okp-private-actions { margin-top: 20px; }

        .okp-idle-footer {
            min-height: 42px;
            padding: 0 24px;
            border-top: 1px solid @okp_light_hairline;
        }

        .is-dark .okp-idle-footer { border-top-color: @okp_dark_hairline; }

        button.okp-idle-footer-button {
            min-height: 30px;
            padding: 4px 10px;
            border: none;
            border-radius: 999px;
            background: transparent;
            box-shadow: none;
        }

        .is-light button.okp-idle-footer-button { color: @okp_ink_mute; }
        .is-dark button.okp-idle-footer-button { color: @okp_snow_mute; }
        .is-light button.okp-idle-footer-button:hover { background: alpha(@okp_navy, 0.06); color: @okp_ink; }
        .is-dark button.okp-idle-footer-button:hover { background: alpha(@okp_accent, 0.08); color: @okp_snow; }

        .okp-idle-footer-status {
            font-size: 11px;
        }

        .is-light .okp-idle-footer-status { color: @okp_ink_faint; }
        .is-dark .okp-idle-footer-status { color: @okp_snow_faint; }
        .is-light .okp-idle-footer-status:not(.is-private) { color: @okp_navy; }
        .is-dark .okp-idle-footer-status:not(.is-private) { color: @okp_accent; }

        .okp-history-page {
            padding: 30px 28px 40px;
        }

        .okp-history-header { min-height: 44px; }

        button.okp-history-back-button {
            min-width: 32px;
            min-height: 32px;
            padding: 0;
            border: 1px solid @okp_light_hairline;
            border-radius: 999px;
            box-shadow: none;
        }

        .is-light button.okp-history-back-button { background: @okp_light_card; color: @okp_ink; }
        .is-dark button.okp-history-back-button { background: @okp_dark_card; border-color: @okp_dark_hairline; color: @okp_snow; }

        .okp-history-title {
            font-family: 'Noto Sans', 'Inter', 'Cantarell', sans-serif;
            font-size: 22px;
            font-weight: 600;
            letter-spacing: -0.015em;
        }

        .is-light .okp-history-title { color: @okp_ink; }
        .is-dark .okp-history-title { color: @okp_snow; }

        entry.okp-history-search {
            min-width: 260px;
            min-height: 34px;
            border-radius: 8px;
            box-shadow: none;
        }

        .is-light entry.okp-history-search {
            background: @okp_light_card;
            border: 1px solid @okp_light_hairline_hi;
            color: @okp_ink;
        }

        .is-dark entry.okp-history-search {
            background: @okp_dark_card;
            border: 1px solid @okp_dark_hairline_hi;
            color: @okp_snow;
        }

        .is-light entry.okp-history-search:focus-within,
        .is-dark entry.okp-history-search:focus-within {
            border-color: @okp_accent;
            box-shadow: 0 0 0 3px alpha(@okp_accent, 0.30);
        }

        .okp-history-subtitle {
            margin-top: 4px;
            font-size: 13px;
        }

        .is-light .okp-history-subtitle { color: @okp_ink_mute; }
        .is-dark .okp-history-subtitle { color: @okp_snow_mute; }

        .okp-history-divider { margin-top: 16px; }

        .okp-history-private-banner {
            margin-top: 16px;
            padding: 10px 14px;
            border-radius: 12px;
            font-size: 12px;
            font-weight: 500;
        }

        .is-light .okp-history-private-banner {
            background: @okp_light_card;
            border: 1px solid @okp_light_hairline;
            color: @okp_ink_mute;
        }

        .is-dark .okp-history-private-banner {
            background: @okp_dark_card;
            border: 1px solid @okp_dark_hairline;
            color: @okp_snow_mute;
        }

        .okp-history-result-caption,
        .okp-history-bucket {
            margin-top: 20px;
            padding: 0 4px 8px;
            font-size: 11px;
            font-weight: 600;
            letter-spacing: 0.05em;
            text-transform: uppercase;
        }

        .is-light .okp-history-result-caption,
        .is-light .okp-history-bucket { color: @okp_ink_faint; }
        .is-dark .okp-history-result-caption,
        .is-dark .okp-history-bucket { color: @okp_snow_faint; }

        /* One card per bucket; the rows inside it share hairlines instead of
           each floating as its own tile. The card clips (set in history_view.rs)
           so a hovered row keeps the card's corners. */
        .okp-history-bucket-card {
            border-radius: 12px;
        }

        .is-light .okp-history-bucket-card {
            background: @okp_light_card;
            border: 1px solid @okp_light_hairline;
            box-shadow: 0 1px 2px rgba(12, 20, 36, 0.04), 0 12px 32px rgba(12, 20, 36, 0.06);
        }

        .is-dark .okp-history-bucket-card {
            background: @okp_dark_card;
            border: 1px solid @okp_dark_hairline;
        }

        .okp-history-bucket-card > overlay {
            border-top: 1px solid alpha(@okp_ink, 0.05);
        }

        .is-dark .okp-history-bucket-card > overlay {
            border-top-color: rgba(120, 160, 220, 0.05);
        }

        .okp-history-bucket-card > overlay:first-child {
            border-top: none;
        }

        button.okp-history-row {
            margin-bottom: 0;
            padding: 10px 16px;
            border: none;
            border-radius: 0;
            background: transparent;
            box-shadow: none;
        }

        menubutton.okp-history-row-actions > button {
            min-width: 32px;
            min-height: 32px;
            padding: 0;
            border-radius: 999px;
        }

        .okp-history-actions-popover {
            min-width: 190px;
            padding: 6px;
        }

        button.okp-history-menu-action {
            min-height: 34px;
            padding: 6px 10px;
            border: none;
            border-radius: 8px;
            box-shadow: none;
        }

        .is-light button.okp-history-row { color: @okp_ink; }
        .is-dark button.okp-history-row { color: @okp_snow; }
        .is-light button.okp-history-row:hover { background: alpha(@okp_navy, 0.06); }
        .is-dark button.okp-history-row:hover { background: alpha(@okp_accent, 0.08); }

        .okp-history-row-title { font-size: 13px; font-weight: 500; }
        .is-light .okp-history-row-title { color: @okp_ink; }
        .is-dark .okp-history-row-title { color: @okp_snow; }
        .okp-history-row-location { font-size: 12px; }

        .okp-history-row-when,
        .okp-history-progress-label,
        .okp-history-barely-label {
            font-family: 'JetBrains Mono', 'Cascadia Mono', monospace;
            font-size: 11px;
            font-feature-settings: 'tnum';
        }

        .is-light .okp-history-row-location,
        .is-light .okp-history-row-when,
        .is-light .okp-history-barely-label { color: @okp_ink_faint; }
        .is-dark .okp-history-row-location,
        .is-dark .okp-history-row-when,
        .is-dark .okp-history-barely-label { color: @okp_snow_faint; }

        .okp-history-progress-label { font-size: 12px; font-weight: 600; }
        .is-light .okp-history-progress-label { color: @okp_navy; }
        .is-dark .okp-history-progress-label { color: @okp_accent; }

        .okp-history-finished-chip {
            padding: 2px 8px;
            border-radius: 999px;
            border: 1px solid transparent;
            font-size: 11px;
            font-weight: 600;
        }

        .is-light .okp-history-finished-chip { background: alpha(@okp_ok, 0.10); border-color: alpha(@okp_ok, 0.42); color: @okp_ok; }
        .is-dark .okp-history-finished-chip { background: alpha(@okp_ok_dark, 0.12); border-color: alpha(@okp_ok_dark, 0.42); color: @okp_ok_dark; }

        .okp-history-end-cap {
            margin-top: 26px;
            padding-top: 18px;
            border-top: 1px solid @okp_light_hairline;
            font-size: 11px;
        }

        .is-light .okp-history-end-cap { color: @okp_ink_faint; }
        .is-dark .okp-history-end-cap { color: @okp_snow_faint; border-top-color: @okp_dark_hairline; }

        .okp-idle-canvas.is-high-contrast {
            background: #000000;
            color: #ffffff;
        }

        .is-high-contrast .okp-idle-titlebar-text,
        .is-high-contrast .okp-welcome-recents-title,
        .is-high-contrast .okp-welcome-recents-subtitle,
        .is-high-contrast .okp-first-run-title,
        .is-high-contrast .okp-first-run-copy,
        .is-high-contrast .okp-private-hero-title,
        .is-high-contrast .okp-private-hero-copy,
        .is-high-contrast .okp-private-hero-icon,
        .is-high-contrast .okp-idle-footer-status:not(.is-private),
        .is-high-contrast .okp-history-title,
        .is-high-contrast .okp-history-subtitle,
        .is-high-contrast .okp-history-result-caption,
        .is-high-contrast .okp-history-bucket,
        .is-high-contrast .okp-history-row-title,
        .is-high-contrast .okp-history-row-location,
        .is-high-contrast .okp-history-row-when,
        .is-high-contrast .okp-history-progress-label,
        .is-high-contrast .okp-history-barely-label,
        .is-high-contrast .okp-history-end-cap,
        .is-high-contrast .okp-idle-footer-status,
        .is-high-contrast button.okp-idle-footer-button {
            color: #ffffff;
        }

        .is-high-contrast button.okp-history-back-button,
        .is-high-contrast entry.okp-history-search,
        .is-high-contrast .okp-history-finished-chip,
        .is-high-contrast button.okp-history-row {
            background: #000000;
            border: 1px solid #ffffff;
            color: #ffffff;
        }

        .is-high-contrast button.okp-history-row:hover,
        .is-high-contrast button.okp-history-row:focus-visible {
            background: #ffffff;
            color: #000000;
            box-shadow: none;
        }

        .is-high-contrast button.okp-history-row:hover .okp-history-row-title,
        .is-high-contrast button.okp-history-row:hover .okp-history-row-location,
        .is-high-contrast button.okp-history-row:hover .okp-history-row-when,
        .is-high-contrast button.okp-history-row:hover .okp-history-progress-label,
        .is-high-contrast button.okp-history-row:hover .okp-history-barely-label,
        .is-high-contrast button.okp-history-row:hover .okp-history-finished-chip,
        .is-high-contrast button.okp-history-row:focus-visible .okp-history-row-title,
        .is-high-contrast button.okp-history-row:focus-visible .okp-history-row-location,
        .is-high-contrast button.okp-history-row:focus-visible .okp-history-row-when,
        .is-high-contrast button.okp-history-row:focus-visible .okp-history-progress-label,
        .is-high-contrast button.okp-history-row:focus-visible .okp-history-barely-label,
        .is-high-contrast button.okp-history-row:focus-visible .okp-history-finished-chip {
            color: #000000;
        }

        .is-high-contrast .okp-history-divider,
        .is-high-contrast .okp-history-end-cap,
        .is-high-contrast .okp-idle-footer {
            border-color: #ffffff;
        }

        .is-high-contrast .okp-history-divider {
            background: #ffffff;
        }

        .is-high-contrast .okp-history-thumbnail-placeholder {
            background: #000000;
            border: 1px solid #ffffff;
        }

        .is-high-contrast progressbar.okp-history-thumb-progress trough {
            background: #000000;
            border: 1px solid #ffffff;
        }

        .is-high-contrast progressbar.okp-history-thumb-progress progress {
            background: #ffffff;
        }

        /* The root keeps .is-light / .is-dark beside .is-high-contrast, so the
           bucket card has to be reset here or it keeps its themed fill under
           the black-and-white rows. */
        .is-high-contrast .okp-history-bucket-card {
            background: #000000;
            border: 1px solid #ffffff;
            box-shadow: none;
        }

        .is-high-contrast .okp-history-bucket-card > overlay {
            border-top-color: #ffffff;
        }

        /* Every other surface the light scheme paints as a white card on this
           canvas, with the labels inside it, since the canvas paints inherited
           text white under high contrast. */
        .is-high-contrast button.okp-recent-card,
        .is-high-contrast button.okp-recents-history-button,
        .is-high-contrast button.okp-idle-secondary-button,
        .is-high-contrast button.okp-first-run-drop-target,
        .is-high-contrast button.okp-welcome-drop-target,
        .is-high-contrast .okp-history-private-banner {
            background: #000000;
            border: 1px solid #ffffff;
            box-shadow: none;
            color: #ffffff;
        }

        .is-high-contrast .okp-recent-title,
        .is-high-contrast .okp-recent-location,
        .is-high-contrast .okp-drop-primary,
        .is-high-contrast .okp-drop-secondary {
            color: #ffffff;
        }

        .is-high-contrast button.okp-idle-primary-button,
        .is-high-contrast button.okp-idle-primary-button:hover {
            background: #ffffff;
            border-color: #ffffff;
            box-shadow: none;
            color: #000000;
        }

        .is-high-contrast progressbar.okp-recent-progress progress {
            background: #ffffff;
        }

        .okp-history-state-card {
            min-width: 360px;
            margin-top: 30px;
            padding: 34px 28px;
            border-radius: 16px;
        }

        .is-light .okp-history-state-card { background: @okp_light_card; border: 1px solid @okp_light_hairline; box-shadow: 0 1px 2px rgba(12, 20, 36, 0.04), 0 12px 32px rgba(12, 20, 36, 0.06); }
        .is-dark .okp-history-state-card { background: @okp_dark_card; border: 1px solid @okp_dark_hairline; }

        .okp-history-state-icon-wrap {
            min-width: 54px;
            min-height: 54px;
            border-radius: 16px;
            background: alpha(@okp_navy, 0.09);
            color: @okp_navy;
        }

        .is-dark .okp-history-state-icon-wrap {
            background: alpha(@okp_accent, 0.16);
            color: @okp_accent;
        }

        .okp-history-state-title { font-size: 17px; font-weight: 600; letter-spacing: -0.01em; }

        .okp-history-state-body { font-size: 13px; line-height: 1.5; }
        .is-light .okp-history-state-body { color: @okp_ink_mute; }
        .is-dark .okp-history-state-body { color: @okp_snow_mute; }

        button.okp-history-state-button {
            min-height: 34px;
            padding: 0 18px;
            border-radius: 999px;
            background: linear-gradient(135deg, @okp_navy, @okp_blue 55%, @okp_accent);
            border: none;
            color: #ffffff;
            font-size: 13px;
            font-weight: 600;
            box-shadow: 0 1px 2px alpha(@okp_navy, 0.30);
        }

        button.okp-history-state-button:hover { background: linear-gradient(135deg, @okp_navy_hover, @okp_blue 55%, @okp_accent_bright); }
        .is-dark button.okp-history-state-button { background: linear-gradient(135deg, @okp_blue, @okp_accent 60%, @okp_accent_bright); color: @okp_accent_ink; }
        .is-dark button.okp-history-state-button:hover { background: linear-gradient(135deg, @okp_blue, @okp_accent_bright 60%, @okp_accent_bright); }

        .okp-history-loading { margin-top: 18px; }
        .okp-history-skeleton-row { padding: 9px 12px; }
        .okp-history-skeleton-caption { min-width: 64px; min-height: 10px; margin: 0 12px 12px; border-radius: 4px; }
        .okp-history-skeleton-thumb { min-width: 64px; min-height: 36px; border-radius: 6px; }
        .okp-history-skeleton-line-1 { min-width: 280px; min-height: 11px; border-radius: 4px; }
        .okp-history-skeleton-line-2 { min-width: 170px; min-height: 9px; border-radius: 4px; }
        .is-light .okp-history-skeleton-caption,
        .is-light .okp-history-skeleton-thumb,
        .is-light .okp-history-skeleton-line-1,
        .is-light .okp-history-skeleton-line-2 { background: @okp_light_inset; }
        .is-dark .okp-history-skeleton-caption,
        .is-dark .okp-history-skeleton-thumb,
        .is-dark .okp-history-skeleton-line-1,
        .is-dark .okp-history-skeleton-line-2 { background: @okp_dark_inset; }

        /* The empty / no-results / error card is an opaque white card in the
           light scheme; under high contrast the canvas paints inherited text
           white, so the card, its tile and its copy are reset explicitly. This
           block has to follow the themed state-card rules above: the root keeps
           .is-light / .is-dark beside .is-high-contrast, and at equal
           specificity the later rule wins. */
        .is-high-contrast .okp-history-state-card,
        .is-high-contrast .okp-history-state-icon-wrap {
            background: #000000;
            border: 1px solid #ffffff;
            box-shadow: none;
            color: #ffffff;
        }

        .is-high-contrast .okp-history-state-title,
        .is-high-contrast .okp-history-state-body {
            color: #ffffff;
        }

        .is-high-contrast button.okp-history-state-button {
            background: #ffffff;
            border: 1px solid #ffffff;
            box-shadow: none;
            color: #000000;
        }

        /* Audio lyrics overlay — an Apple-Music-style sheet over the (black) audio plane. The scrim
           is translucent so any embedded cover art shows dimly behind, the lines dim away from the
           active one, and the brand teal lands on the header so it reads at a glance. */
        .okp-lyrics-surface {
            background: alpha(@okp_bg, 0.90);
        }

        .okp-lyrics-content {
            margin-top: 48px;
            margin-bottom: 96px;
        }

        .okp-lyrics-header {
            margin-bottom: 6px;
            color: @okp_accent;
            font-size: 11px;
            font-weight: 600;
            letter-spacing: 0.05em;
            text-transform: uppercase;
        }

        .okp-lyrics-scroller {
            min-width: 420px;
        }

        .okp-lyrics-list {
            padding: 40px 20px;
        }

        button.okp-lyrics-line {
            padding: 8px 14px;
            border: none;
            border-radius: 12px;
            background: transparent;
            box-shadow: none;
            color: rgba(255, 255, 255, 0.42);
            font-size: 18px;
            font-weight: 600;
        }

        button.okp-lyrics-line:hover {
            background: rgba(255, 255, 255, 0.06);
            color: rgba(255, 255, 255, 0.72);
        }

        button.okp-lyrics-line.is-gap {
            color: rgba(255, 255, 255, 0.24);
        }

        button.okp-lyrics-line.is-active {
            background: transparent;
            color: #ffffff;
            font-size: 21px;
            font-weight: 700;
        }

        button.okp-lyrics-line.is-active:hover {
            background: rgba(255, 255, 255, 0.06);
        }

        button.okp-lyrics-line:focus-visible {
            outline: none;
            box-shadow: 0 0 0 3px alpha(@okp_accent, 0.30);
        }

        .okp-lyrics-line-plain {
            padding: 7px 14px;
            color: rgba(255, 255, 255, 0.82);
            font-size: 16px;
            font-weight: 500;
        }

        .okp-lyrics-empty-icon {
            color: @okp_accent;
        }

        .okp-lyrics-empty-text {
            margin-top: 4px;
            color: rgba(255, 255, 255, 0.56);
            font-size: 14px;
            font-weight: 500;
        }

        .okp-chrome-revealer {
            opacity: 1;
            transform: translate(0, 0);
            transition: opacity 180ms ease, transform 200ms ease;
        }

        .okp-chrome-revealer.is-hidden {
            opacity: 0;
            transform: translate(0, 16px);
        }

        .okp-compact-motion {
            opacity: 1;
            transform: translate(0, 0);
            transition: opacity 180ms ease, transform 180ms ease;
        }

        .okp-compact-motion.is-hidden {
            opacity: 0;
            transform: translate(0, 8px);
        }

        .okp-compact-top-bar,
        .okp-compact-bottom-bar {
            min-height: 28px;
            padding: 6px 8px;
            border-radius: 999px;
            background: rgba(12, 20, 36, 0.56);
            border: 1px solid rgba(255, 255, 255, 0.12);
            box-shadow: 0 14px 40px rgba(0, 0, 0, 0.30);
        }

        button.okp-compact-button {
            min-width: 28px;
            min-height: 28px;
            padding: 0;
            border: 1px solid transparent;
            border-radius: 999px;
            background: transparent;
            box-shadow: none;
            color: rgba(255, 255, 255, 0.95);
            -gtk-icon-size: 16px;
        }

        button.okp-compact-button:hover {
            background: rgba(255, 255, 255, 0.12);
        }

        button.okp-compact-button:active {
            background: rgba(255, 255, 255, 0.18);
        }

        button.okp-compact-button:focus-visible {
            outline: none;
            box-shadow: 0 0 0 3px alpha(@okp_accent, 0.30);
        }

        button.okp-compact-close:hover {
            background: alpha(@okp_danger_dark, 0.86);
        }

        button.okp-compact-play {
            min-width: 42px;
            min-height: 42px;
            border-radius: 999px;
            background: rgba(12, 20, 36, 0.56);
            border: 1px solid rgba(255, 255, 255, 0.14);
            box-shadow: 0 6px 20px rgba(0, 0, 0, 0.40);
            -gtk-icon-size: 21px;
        }

        .okp-compact-title {
            color: rgba(255, 255, 255, 0.95);
            font-size: 12px;
            font-weight: 600;
        }

        .okp-compact-time {
            color: rgba(255, 255, 255, 0.85);
            font-family: 'JetBrains Mono', 'Cascadia Mono', monospace;
            font-size: 11px;
            font-weight: 500;
            font-feature-settings: 'tnum';
        }

        scale.okp-compact-seek {
            min-width: 80px;
            min-height: 20px;
            margin: 0;
            padding: 0;
        }

        scale.okp-compact-seek trough {
            min-height: 3px;
            border: none;
            border-radius: 999px;
            background: rgba(255, 255, 255, 0.22);
            box-shadow: none;
        }

        scale.okp-compact-seek highlight {
            min-width: 2px;
            min-height: 3px;
            border: none;
            border-radius: 999px;
            background: @okp_accent;
        }

        scale.okp-compact-seek slider {
            min-width: 10px;
            min-height: 10px;
            margin: 0;
            border: none;
            border-radius: 999px;
            background: #ffffff;
            box-shadow: 0 0 0 3px alpha(@okp_accent, 0.30), 0 1px 4px rgba(0, 0, 0, 0.45);
        }

        window.okp-player-window.is-high-contrast .okp-compact-top-bar,
        window.okp-player-window.is-high-contrast .okp-compact-bottom-bar,
        window.okp-player-window.is-reduced-transparency .okp-compact-top-bar,
        window.okp-player-window.is-reduced-transparency .okp-compact-bottom-bar {
            background: @okp_dark_card;
            border-color: #ffffff;
            box-shadow: none;
        }

        .okp-root.is-reduced-motion .okp-compact-motion {
            transition: none;
        }

        .okp-bottom-scrim {
            background: linear-gradient(to top, rgba(7, 15, 34, 0.50), rgba(7, 15, 34, 0));
        }

        .okp-controls {
            min-height: 32px;
            /* The adaptive OscBar owns the 7px/14px content inset in its own
             * allocation (issue #328), so the CSS padding is zeroed to avoid
             * double-insetting the controls. */
            padding: 0;
            border-radius: 16px;
            /* GTK 4 has no portable backdrop-filter. This navy-tinted 56% glass
             * plus the localized scrim is the deterministic fallback for the
             * blur/saturation material used on platforms that support it. */
            background: rgba(12, 20, 36, 0.56);
            border: 1px solid rgba(255, 255, 255, 0.12);
            box-shadow: 0 4px 12px rgba(0, 0, 0, 0.18), 0 24px 60px rgba(0, 0, 0, 0.28);
        }

        button.okp-control-button,
        menubutton.okp-control-button > button {
            min-width: 32px;
            min-height: 32px;
            padding: 0;
            border-radius: 999px;
            border: 1px solid transparent;
            background: transparent;
            box-shadow: none;
            color: rgba(255, 255, 255, 0.88);
            font-size: 12.5px;
            font-weight: 600;
        }

        button.okp-control-button:hover,
        menubutton.okp-control-button > button:hover {
            background: rgba(255, 255, 255, 0.12);
            color: rgba(255, 255, 255, 0.98);
        }

        button.okp-control-button:active,
        menubutton.okp-control-button > button:active,
        button.okp-control-button:checked,
        menubutton.okp-control-button > button:checked {
            background: alpha(@okp_accent, 0.22);
            border-color: alpha(@okp_accent, 0.45);
            color: @okp_accent_bright;
        }

        button.okp-control-button:disabled,
        menubutton.okp-control-button > button:disabled {
            background: transparent;
            border-color: transparent;
            color: rgba(255, 255, 255, 0.50);
        }

        button.okp-control-button:focus-visible,
        menubutton.okp-control-button > button:focus-visible {
            outline: none;
            box-shadow: 0 0 0 3px alpha(@okp_accent, 0.30);
        }

        button.okp-play-button {
            min-width: 32px;
            border-radius: 999px;
            background: rgba(255, 255, 255, 0.14);
            color: #ffffff;
            -gtk-icon-size: 20px;
        }

        button.okp-play-button:hover {
            background: rgba(255, 255, 255, 0.24);
        }

        button.okp-play-button:disabled {
            background: transparent;
            color: rgba(255, 255, 255, 0.68);
        }

        button.okp-transport-button {
            min-width: 32px;
            -gtk-icon-size: 17px;
        }

        button.okp-icon-button,
        menubutton.okp-icon-button > button {
            min-width: 32px;
            padding: 0;
        }

        button.okp-utility-button,
        menubutton.okp-utility-button > button {
            -gtk-icon-size: 19px;
        }

        menubutton.okp-speed-chip > button {
            min-width: 50px;
            padding: 0 10px;
            border-radius: 999px;
            background: rgba(255, 255, 255, 0.12);
            color: rgba(255, 255, 255, 0.92);
            font-family: 'JetBrains Mono', 'Cascadia Mono', monospace;
            font-size: 12px;
            font-weight: 600;
            font-feature-settings: 'tnum';
        }

        .okp-control-button.is-selected {
            background: alpha(@okp_accent, 0.22);
        }

        .okp-time-label {
            color: rgba(255, 255, 255, 0.88);
            font-family: 'JetBrains Mono', 'Cascadia Mono', monospace;
            font-size: 12px;
            font-weight: 500;
            font-feature-settings: 'tnum';
        }

        .okp-elapsed-time {
            min-width: 54px;
        }

        .okp-remaining-time {
            min-width: 62px;
        }

        .okp-status-toast {
            padding: 9px 14px;
            border-radius: 12px;
            background: rgba(12, 20, 36, 0.62);
            border: 1px solid rgba(255, 255, 255, 0.10);
            box-shadow: 0 4px 12px rgba(0, 0, 0, 0.18), 0 24px 60px rgba(0, 0, 0, 0.28);
            color: rgba(255, 255, 255, 0.92);
            font-size: 13px;
            font-weight: 600;
        }

        .okp-status-toast-thumbnail {
            min-width: 64px;
            min-height: 36px;
            border-radius: 6px;
            background: @okp_bg;
        }

        button.okp-status-toast-path {
            min-width: 0;
            min-height: 0;
            padding: 1px 2px;
            border: none;
            border-radius: 4px;
            background: transparent;
            box-shadow: none;
            color: @okp_accent_bright;
            text-decoration-line: underline;
            text-decoration-color: alpha(@okp_accent_bright, 0.72);
        }

        button.okp-status-toast-path:hover {
            background: rgba(255, 255, 255, 0.08);
            color: #ffffff;
        }

        button.okp-status-toast-path:focus-visible {
            outline: none;
            box-shadow: 0 0 0 3px alpha(@okp_accent, 0.30);
        }

        button.okp-status-toast-path:disabled {
            color: rgba(255, 255, 255, 0.58);
            text-decoration-color: rgba(255, 255, 255, 0.34);
        }

        .okp-update-action-surface {
            padding: 12px 14px;
            border-radius: 12px;
            background: @okp_light_rail;
            border: 1px solid transparent;
        }

        .okp-update-action-title {
            color: @okp_ink;
            font-family: 'Noto Sans', 'Inter', 'Cantarell', sans-serif;
            font-size: 13px;
            font-weight: 600;
        }

        .okp-update-action-detail {
            color: @okp_ink_mute;
            font-family: 'Noto Sans', 'Inter', 'Cantarell', sans-serif;
            font-size: 12px;
        }

        /* The repository setup commands (#725). Monospaced because they are
           commands to be read and pasted, not prose, and boxed so it is
           obvious where the block a user has to copy begins and ends. */
        .okp-update-setup-commands {
            color: @okp_ink_dim;
            font-family: 'JetBrains Mono', 'Cascadia Mono', monospace;
            font-size: 11px;
            padding: 8px 10px;
            margin-top: 4px;
            border-radius: 8px;
            background: @okp_light_card;
            border: 1px solid @okp_light_hairline;
        }

        button.okp-update-primary-button {
            min-height: 30px;
            padding: 0 16px;
            border-radius: 999px;
            border: none;
            background: linear-gradient(135deg, @okp_navy, @okp_blue 55%, @okp_accent);
            color: #ffffff;
            font-family: 'Noto Sans', 'Inter', 'Cantarell', sans-serif;
            font-size: 12px;
            font-weight: 600;
            box-shadow: 0 1px 2px alpha(@okp_navy, 0.30);
        }

        button.okp-update-primary-button:hover {
            background: linear-gradient(135deg, @okp_navy_hover, @okp_blue 55%, @okp_accent_bright);
        }

        button.okp-update-primary-button:focus-visible {
            outline: none;
            box-shadow: 0 0 0 3px alpha(@okp_accent, 0.30);
        }

        button.okp-update-primary-button:disabled {
            opacity: 0.64;
        }

        .okp-persistent-update {
            padding: 12px 14px;
            border-radius: 16px;
            background: rgba(12, 20, 36, 0.90);
            border: 1px solid rgba(255, 255, 255, 0.12);
            box-shadow: 0 4px 12px rgba(0, 0, 0, 0.18), 0 24px 60px rgba(0, 0, 0, 0.28);
        }

        .okp-persistent-update .okp-update-action-title {
            color: rgba(255, 255, 255, 0.94);
        }

        .okp-persistent-update .okp-update-action-detail {
            color: rgba(255, 255, 255, 0.66);
        }

        .okp-persistent-update button.okp-settings-button {
            color: rgba(255, 255, 255, 0.88);
            background: rgba(255, 255, 255, 0.08);
            border-color: rgba(255, 255, 255, 0.12);
        }

        .okp-persistent-update button.okp-settings-button:hover {
            background: rgba(255, 255, 255, 0.14);
        }

        /* The seek bar's floor is the whole pill's floor: it is the only control
           that absorbs slack, so a wide minimum here is what pushed the bar past
           a portrait-fit window and got its tail clipped (#729). Keep this in
           step with TIMELINE_MIN_WIDTH in controls.rs. */
        .okp-timeline,
        .okp-seek {
            min-width: 48px;
            min-height: 20px;
        }

        .okp-timeline-rail {
            min-width: 48px;
            min-height: 20px;
        }

        scale.okp-seek trough {
            min-height: 4px;
            background: transparent;
            border: none;
            box-shadow: none;
        }

        scale.okp-seek highlight {
            min-height: 4px;
            background: transparent;
            border: none;
            box-shadow: none;
        }

        scale.okp-seek slider {
            min-width: 12px;
            min-height: 12px;
            margin: 0;
            background: transparent;
            border: none;
            box-shadow: none;
        }

        .okp-seek-preview {
            padding: 8px 10px;
            border-radius: 12px;
            background: rgba(12, 20, 36, 0.94);
            border: 1px solid rgba(255, 255, 255, 0.10);
            box-shadow: 0 4px 12px rgba(0, 0, 0, 0.18), 0 24px 60px rgba(0, 0, 0, 0.28);
        }

        .okp-seek-preview-thumb {
            margin-bottom: 6px;
            border-radius: 6px;
            background: rgba(255, 255, 255, 0.08);
        }

        .okp-seek-preview-time {
            color: rgba(255, 255, 255, 0.92);
            font-family: 'JetBrains Mono', 'Cascadia Mono', monospace;
            font-size: 12px;
            font-weight: 600;
            font-feature-settings: 'tnum';
        }

        .okp-seek-preview-chapter {
            margin-top: 2px;
            color: rgba(255, 255, 255, 0.62);
            font-size: 11px;
        }

        .okp-volume-control,
        button.okp-volume-button {
            min-width: 34px;
            min-height: 34px;
        }

        button.okp-volume-button {
            padding: 0;
            margin: 0;
            border: none;
            border-radius: 999px;
            background: transparent;
            box-shadow: none;
        }

        button.okp-volume-button:hover,
        button.okp-volume-button:focus-visible {
            background: rgba(255, 255, 255, 0.10);
        }

        .okp-volume-icon {
            color: rgba(255, 255, 255, 0.86);
            -gtk-icon-size: 18px;
        }

        .okp-volume-wick {
            min-width: 18px;
            min-height: 3px;
            margin-top: -2px;
        }

        .okp-volume-control.is-boosted .okp-volume-icon,
        .okp-volume-control.is-muted .okp-volume-icon {
            color: @okp_warning_dark;
        }

        popover.okp-volume-popover,
        popover.okp-volume-popover > contents {
            padding: 0;
            background: transparent;
            border: none;
            box-shadow: none;
        }

        popover.okp-volume-popover > arrow {
            min-width: 0;
            min-height: 0;
            background: transparent;
            border: none;
            box-shadow: none;
        }

        .okp-volume-capsule {
            padding: 9px 13px;
            border-radius: 16px;
            background: rgba(12, 20, 36, 0.72);
            border: 1px solid rgba(255, 255, 255, 0.13);
            box-shadow: 0 4px 12px rgba(0, 0, 0, 0.18), 0 24px 60px rgba(0, 0, 0, 0.28);
            opacity: 0;
            transform: translate(0, 6px) scale(0.96);
            transition: opacity 150ms cubic-bezier(0.25, 0.1, 0.25, 1), transform 150ms cubic-bezier(0.25, 0.1, 0.25, 1);
        }

        .okp-volume-capsule.is-open {
            opacity: 1;
            transform: translate(0, 0) scale(1);
        }

        .okp-volume-capsule.is-closing {
            transition: opacity 120ms cubic-bezier(0.25, 0.1, 0.25, 1), transform 120ms cubic-bezier(0.25, 0.1, 0.25, 1);
        }

        .okp-volume-capsule.reduce-motion {
            transition: none;
        }

        .okp-volume-track-stack,
        .okp-volume-track,
        scale.okp-volume-slider {
            min-width: 122px;
            min-height: 14px;
        }

        scale.okp-volume-slider {
            padding: 0;
            margin: 0;
        }

        scale.okp-volume-slider trough,
        scale.okp-volume-slider highlight {
            min-width: 0;
            min-height: 6px;
            padding: 0;
            margin: 0;
            background: transparent;
            border: none;
            box-shadow: none;
        }

        scale.okp-volume-slider slider {
            min-width: 14px;
            min-height: 14px;
            margin: 0;
            border: none;
            border-radius: 999px;
            background: rgba(255, 255, 255, 0.98);
            box-shadow: 0 0 0 3px alpha(@okp_accent, 0.30), 0 1px 4px rgba(0, 0, 0, 0.40);
        }

        .okp-volume-capsule.is-boosted scale.okp-volume-slider slider {
            background: @okp_warning_dark;
        }

        .okp-volume-capsule.is-muted scale.okp-volume-slider slider {
            opacity: 0.54;
        }

        .okp-volume-readout-stack,
        button.okp-volume-readout,
        entry.okp-volume-readout-input {
            min-width: 48px;
            min-height: 26px;
        }

        button.okp-volume-readout {
            padding: 0 4px;
            border: none;
            border-radius: 999px;
            background: transparent;
            box-shadow: none;
            color: rgba(255, 255, 255, 0.94);
            font-family: 'JetBrains Mono', 'Cascadia Mono', monospace;
            font-size: 12px;
            font-weight: 600;
            font-feature-settings: 'tnum';
        }

        button.okp-volume-readout:hover,
        button.okp-volume-readout:focus-visible {
            background: rgba(255, 255, 255, 0.09);
        }

        .okp-volume-capsule.is-boosted button.okp-volume-readout,
        .okp-volume-capsule.is-muted button.okp-volume-readout {
            color: @okp_warning_dark;
        }

        entry.okp-volume-readout-input {
            padding: 2px 6px;
            border-radius: 8px;
            background: rgba(255, 255, 255, 0.10);
            border: 1px solid @okp_accent;
            color: rgba(255, 255, 255, 0.96);
            font-family: 'JetBrains Mono', 'Cascadia Mono', monospace;
            font-size: 12px;
            font-feature-settings: 'tnum';
        }

        .okp-up-next-panel {
            border-radius: 16px 0 0 16px;
            background: @okp_light_card;
            border-style: solid;
            border-color: @okp_light_hairline;
            border-width: 1px 0 1px 1px;
            box-shadow: 0 4px 12px rgba(12, 20, 36, 0.08), 0 24px 60px rgba(12, 20, 36, 0.18);
        }

        .okp-side-panel-header {
            padding: 14px 8px 6px 14px;
        }

        .okp-side-panel-tabs {
            padding: 3px;
            border-radius: 999px;
            background: @okp_light_rail;
            border: 1px solid @okp_light_hairline;
        }

        button.okp-side-panel-tab {
            min-height: 28px;
            padding: 0 14px;
            border-radius: 999px;
            border: none;
            background: transparent;
            box-shadow: none;
            color: @okp_ink_mute;
            font-size: 12px;
            font-weight: 600;
        }

        button.okp-side-panel-tab:hover {
            background: transparent;
            color: @okp_ink;
        }

        button.okp-side-panel-tab.is-selected {
            background: @okp_light_card;
            color: @okp_ink;
            box-shadow: 0 1px 0 rgba(12, 20, 36, 0.04), 0 8px 24px rgba(12, 20, 36, 0.05);
        }

        button.okp-side-panel-tab:focus-visible {
            outline: none;
            box-shadow: 0 0 0 3px alpha(@okp_accent, 0.30);
        }

        button.okp-side-panel-close {
            min-width: 30px;
            min-height: 30px;
            padding: 0;
            border: none;
            border-radius: 999px;
            background: transparent;
            color: @okp_ink_mute;
            box-shadow: none;
        }

        button.okp-side-panel-close:hover {
            background: alpha(@okp_navy, 0.06);
            color: @okp_ink;
        }

        button.okp-side-panel-close:focus-visible {
            outline: none;
            box-shadow: 0 0 0 3px alpha(@okp_accent, 0.30);
        }

        .okp-up-next-list {
            margin: 0 8px 10px 8px;
            background: transparent;
        }

        .okp-up-next-list row {
            background: transparent;
        }

        .okp-panel-heading-row {
            padding: 6px 12px 4px 12px;
        }

        .okp-panel-heading {
            color: @okp_ink_faint;
            font-size: 11px;
            font-weight: 600;
            letter-spacing: 0.05em;
            text-transform: uppercase;
        }

        .okp-up-next-list row.okp-panel-empty-row {
            min-height: 88px;
            margin: 4px 6px 8px 6px;
            padding: 20px 18px;
            border-radius: 12px;
            background: @okp_light_rail;
            border: 1px dashed @okp_light_hairline_hi;
        }

        .okp-panel-empty {
            color: @okp_ink_mute;
            font-size: 12.5px;
        }

        .okp-panel-caption-row {
            padding: 0 12px 6px 12px;
        }

        .okp-panel-caption {
            color: @okp_ink_faint;
            font-size: 11.5px;
        }

        .okp-up-next-row {
            min-height: 40px;
            margin: 1px 0;
            padding: 7px 8px;
            border-radius: 8px;
            border: 1px solid transparent;
            background: transparent;
            color: @okp_ink_dim;
        }

        .okp-chapter-row {
            min-height: 42px;
            padding: 5px 0;
        }

        .okp-chapter-current-rail {
            min-width: 4px;
            min-height: 34px;
            border-radius: 0 2px 2px 0;
            background: linear-gradient(to bottom, @okp_navy, @okp_blue 55%, @okp_accent);
            opacity: 0;
        }

        .okp-chapter-current-rail.is-current {
            opacity: 1;
        }

        .okp-chapter-thumb {
            min-width: 56px;
            min-height: 32px;
            border-radius: 6px;
            background: @okp_light_inset;
            border: 1px solid @okp_light_hairline;
        }

        .okp-chapter-thumb.is-pending {
            background: @okp_light_rail;
            border-style: dashed;
            border-color: @okp_light_hairline_hi;
        }

        .okp-chapter-thumb-placeholder {
            color: @okp_ink_faint;
        }

        .okp-bookmark-row .okp-bookmark-icon {
            color: @okp_blue;
        }

        .okp-up-next-list row.okp-interval-row {
            background: alpha(@okp_navy, 0.05);
            border-style: dashed;
            border-color: @okp_light_hairline_hi;
        }

        .okp-interval-row .okp-interval-icon {
            color: @okp_ink_mute;
        }

        .okp-interval-row .okp-up-next-marker {
            color: @okp_ink_mute;
        }

        .okp-up-next-list row.okp-detect-row {
            min-height: 52px;
            margin: 2px 6px 8px 6px;
            border-radius: 12px;
            background: alpha(@okp_navy, 0.09);
            border-color: alpha(@okp_navy, 0.30);
            color: @okp_navy;
        }

        .okp-up-next-list row.okp-detect-row:hover {
            background: alpha(@okp_navy, 0.14);
            border-color: alpha(@okp_navy, 0.50);
            color: @okp_navy;
        }

        .okp-detect-row .okp-detect-icon {
            color: @okp_navy;
        }

        .okp-detect-subtitle {
            color: @okp_ink_mute;
            font-size: 11px;
        }

        .okp-detect-status-row {
            margin: 2px 6px 8px 6px;
            padding: 9px 10px;
            border-radius: 12px;
            background: @okp_light_rail;
            border: 1px dashed @okp_light_hairline_hi;
        }

        .okp-detect-status-icon {
            color: @okp_ink_faint;
        }

        .okp-detect-status {
            color: @okp_ink_mute;
            font-size: 12px;
        }

        .okp-up-next-list row.okp-interval-row:focus-visible,
        .okp-up-next-list row.okp-detect-row:focus-visible {
            outline: none;
            box-shadow: inset 0 0 0 3px alpha(@okp_accent, 0.30);
        }

        .okp-up-next-list row.okp-add-bookmark-row {
            margin-top: 6px;
            border-radius: 999px;
            background: linear-gradient(135deg, @okp_navy, @okp_blue 55%, @okp_accent);
            border-color: transparent;
            color: #ffffff;
            font-weight: 600;
            box-shadow: 0 1px 2px alpha(@okp_navy, 0.30);
        }

        .okp-up-next-list row.okp-add-bookmark-row:hover {
            background: linear-gradient(135deg, @okp_navy_hover, @okp_blue 55%, @okp_accent_bright);
            border-color: transparent;
            color: #ffffff;
        }

        .okp-add-bookmark-row .okp-add-bookmark-icon {
            color: #ffffff;
        }

        .okp-up-next-list row.okp-add-files-row {
            min-height: 70px;
            margin: 6px;
            border-radius: 12px;
            background: @okp_light_card;
            border-style: dashed;
            border-color: @okp_light_hairline_hi;
            color: @okp_navy;
            font-weight: 600;
        }

        .okp-up-next-list row.okp-add-files-row:hover {
            background: alpha(@okp_navy, 0.06);
            border-color: @okp_navy;
            color: @okp_navy;
        }

        .okp-add-files-row .okp-add-files-icon {
            color: @okp_navy;
        }

        /* The lone now-playing card at the top of a short queue has no reorder /
           remove controls, so give it a touch more breathing room than a regular
           queue row so it reads as a pinned card rather than a stripped row. */
        .okp-up-next-list row.okp-now-playing-pinned-row {
            min-height: 50px;
            margin: 6px 6px 10px 6px;
            padding: 8px;
            border-radius: 12px;
            background: alpha(@okp_navy, 0.09);
            color: @okp_navy;
        }

        .okp-now-playing-thumb {
            min-width: 54px;
            min-height: 34px;
            border-radius: 5px;
            background: linear-gradient(135deg, @okp_navy_deep, @okp_blue);
            color: rgba(255, 255, 255, 0.56);
        }

        .okp-now-playing-title {
            color: @okp_navy;
            font-size: 12.5px;
            font-weight: 600;
        }

        .okp-now-playing-state {
            color: @okp_link;
            font-size: 10.5px;
            font-weight: 600;
        }

        .okp-up-next-row.is-current .okp-chapter-thumb {
            border-color: alpha(@okp_navy, 0.35);
        }

        .okp-up-next-row:hover {
            background: alpha(@okp_navy, 0.06);
        }

        .okp-up-next-row.is-current {
            background: alpha(@okp_navy, 0.09);
            border-color: transparent;
            color: @okp_navy;
        }

        .okp-up-next-row.is-behind {
            background: transparent;
            color: @okp_ink_faint;
        }

        .okp-up-next-row.is-behind:hover {
            background: alpha(@okp_navy, 0.06);
            color: @okp_ink_mute;
        }

        .okp-up-next-row.is-behind .okp-up-next-source-icon {
            opacity: 0.55;
        }

        .okp-up-next-row.is-drop-target {
            background: alpha(@okp_accent, 0.16);
            border-color: alpha(@okp_accent, 0.60);
        }

        .okp-up-next-drag-handle {
            min-width: 18px;
            color: alpha(@okp_ink, 0.28);
        }

        .okp-up-next-drag-handle-icon {
            -gtk-icon-size: 16px;
        }

        .okp-up-next-row:hover .okp-up-next-drag-handle,
        .okp-up-next-row.is-drop-target .okp-up-next-drag-handle {
            color: @okp_ink_mute;
        }

        .okp-up-next-lane {
            min-width: 42px;
        }

        .okp-up-next-index {
            min-width: 22px;
            color: @okp_ink_faint;
            font-family: 'JetBrains Mono', 'Cascadia Mono', monospace;
            font-size: 11px;
            font-weight: 600;
            font-feature-settings: 'tnum';
        }

        .okp-up-next-watched-icon {
            color: @okp_ok;
        }

        .okp-up-next-source-icon {
            color: @okp_ink_mute;
        }

        .okp-up-next-row.is-current .okp-up-next-source-icon {
            color: @okp_navy;
        }

        .okp-now-badge {
            padding: 1px 7px;
            border-radius: 999px;
            background: @okp_accent;
            color: @okp_accent_ink;
            font-size: 9.5px;
            font-weight: 800;
            letter-spacing: 0;
        }

        .okp-next-badge {
            padding: 1px 7px;
            border-radius: 999px;
            background: alpha(@okp_navy, 0.09);
            color: @okp_navy;
            font-size: 9.5px;
            font-weight: 700;
            letter-spacing: 0;
        }

        .okp-up-next-marker {
            color: @okp_ink_mute;
            font-family: 'JetBrains Mono', 'Cascadia Mono', monospace;
            font-size: 11px;
            font-weight: 500;
            font-feature-settings: 'tnum';
        }

        .okp-up-next-row.is-current .okp-up-next-marker {
            color: @okp_navy;
        }

        .okp-up-next-file {
            color: inherit;
            font-size: 13px;
        }

        menubutton.okp-up-next-actions-menu > button {
            min-width: 26px;
            min-height: 26px;
            padding: 0;
            border: none;
            border-radius: 999px;
            background: transparent;
            color: @okp_ink_mute;
            box-shadow: none;
        }

        menubutton.okp-up-next-actions-menu > button:hover {
            background: alpha(@okp_navy, 0.06);
            color: @okp_ink;
        }

        .okp-up-next-actions-popover {
            min-width: 142px;
            padding: 6px;
            background: @okp_light_card;
        }

        button.okp-up-next-menu-action {
            min-height: 30px;
            padding: 5px 10px;
            border: none;
            border-radius: 8px;
            background: transparent;
            color: @okp_ink_dim;
            box-shadow: none;
        }

        button.okp-up-next-menu-action:hover {
            background: alpha(@okp_navy, 0.06);
        }

        button.okp-up-next-action-button {
            min-width: 24px;
            min-height: 24px;
            padding: 0;
            border: none;
            border-radius: 999px;
            background: transparent;
            box-shadow: none;
            color: @okp_ink_mute;
        }

        button.okp-up-next-action-button:hover {
            background: alpha(@okp_navy, 0.06);
            color: @okp_ink;
        }

        button.okp-up-next-action-button:disabled {
            color: alpha(@okp_ink, 0.18);
        }

        .okp-up-next-panel scrolledwindow {
            background: transparent;
        }

        .okp-up-next-panel scrollbar,
        .okp-up-next-panel scrollbar trough {
            background: transparent;
            border: none;
        }

        .okp-up-next-panel scrollbar slider {
            min-width: 4px;
            border-radius: 999px;
            background: alpha(@okp_ink, 0.22);
        }

        .okp-track-popover-content {
            padding: 8px;
            background: @okp_light_card;
            color: @okp_ink;
        }

        .okp-track-popover-content.okp-speed-popover {
            padding: 6px;
        }

        popover.okp-track-popover {
            padding: 0;
            background: transparent;
            border: none;
            box-shadow: none;
        }

        popover.okp-track-popover > contents,
        popover.okp-track-popover contents {
            padding: 0;
            border-radius: 12px;
            background: @okp_light_card;
            border: 1px solid @okp_light_hairline;
            box-shadow: 0 16px 50px rgba(12, 20, 36, 0.18);
        }

        popover.okp-track-popover arrow {
            min-width: 0;
            min-height: 0;
            background: transparent;
            border: none;
            box-shadow: none;
        }

        /* Dark scheme for the chapters / up-next panel and the track popover
           shell. The design themes these surfaces with the same surface tokens
           as the idle shell; the panel and each track popover carry .is-dark
           from the idle theme, and every light rule above with a scheme-bound
           value has its dark counterpart here. */
        .okp-up-next-panel.is-dark {
            background: @okp_dark_card;
            border-color: @okp_dark_hairline;
            box-shadow: 0 4px 12px rgba(0, 0, 0, 0.20), 0 24px 60px rgba(0, 0, 0, 0.45);
        }

        .is-dark .okp-side-panel-tabs {
            background: @okp_dark_rail;
            border: 1px solid @okp_dark_hairline;
        }

        .is-dark button.okp-side-panel-tab {
            color: @okp_snow_mute;
        }

        .is-dark button.okp-side-panel-tab:hover {
            background: transparent;
            color: @okp_snow;
        }

        .is-dark button.okp-side-panel-tab.is-selected {
            background: @okp_dark_card;
            color: @okp_snow;
            box-shadow: 0 1px 0 rgba(0, 0, 0, 0.10), 0 8px 24px rgba(0, 0, 0, 0.12);
        }

        .is-dark button.okp-side-panel-tab:focus-visible {
            box-shadow: 0 0 0 3px alpha(@okp_accent, 0.30);
        }

        .is-dark button.okp-side-panel-close {
            color: @okp_snow_mute;
        }

        .is-dark button.okp-side-panel-close:hover {
            background: alpha(@okp_accent, 0.08);
            color: @okp_snow;
        }

        .is-dark button.okp-side-panel-close:focus-visible {
            box-shadow: 0 0 0 3px alpha(@okp_accent, 0.30);
        }

        .is-dark .okp-panel-heading {
            color: @okp_snow_faint;
        }

        .is-dark .okp-up-next-list row.okp-panel-empty-row {
            background: @okp_dark_rail;
            border: 1px dashed @okp_dark_hairline_hi;
        }

        .is-dark .okp-panel-empty {
            color: @okp_snow_mute;
        }

        .is-dark .okp-panel-caption {
            color: @okp_snow_faint;
        }

        .is-dark .okp-up-next-row {
            color: @okp_snow_dim;
        }

        .is-dark .okp-chapter-current-rail {
            background: linear-gradient(to bottom, @okp_blue, @okp_accent 60%, @okp_accent_bright);
        }

        .is-dark .okp-chapter-thumb {
            background: @okp_dark_inset;
            border: 1px solid @okp_dark_hairline;
        }

        .is-dark .okp-chapter-thumb.is-pending {
            background: @okp_dark_rail;
            border-color: @okp_dark_hairline_hi;
        }

        .is-dark .okp-chapter-thumb-placeholder {
            color: @okp_snow_faint;
        }

        .is-dark .okp-up-next-list row.okp-interval-row {
            background: alpha(@okp_accent, 0.07);
            border-color: @okp_dark_hairline_hi;
        }

        .is-dark .okp-interval-row .okp-interval-icon {
            color: @okp_snow_mute;
        }

        .is-dark .okp-interval-row .okp-up-next-marker {
            color: @okp_snow_mute;
        }

        .is-dark .okp-up-next-list row.okp-detect-row {
            background: alpha(@okp_accent, 0.11);
            border-color: alpha(@okp_accent, 0.32);
            color: @okp_accent_bright;
        }

        .is-dark .okp-up-next-list row.okp-detect-row:hover {
            background: alpha(@okp_accent, 0.16);
            border-color: alpha(@okp_accent, 0.52);
            color: @okp_accent_bright;
        }

        .is-dark .okp-detect-row .okp-detect-icon {
            color: @okp_accent_bright;
        }

        .is-dark .okp-detect-subtitle {
            color: @okp_snow_mute;
        }

        .is-dark .okp-detect-status-row {
            background: @okp_dark_rail;
            border: 1px dashed @okp_dark_hairline_hi;
        }

        .is-dark .okp-detect-status-icon {
            color: @okp_snow_faint;
        }

        .is-dark .okp-detect-status {
            color: @okp_snow_mute;
        }

        .is-dark .okp-up-next-list row.okp-interval-row:focus-visible,
        .is-dark .okp-up-next-list row.okp-detect-row:focus-visible {
            box-shadow: inset 0 0 0 3px alpha(@okp_accent, 0.30);
        }

        .is-dark .okp-up-next-list row.okp-add-bookmark-row {
            background: linear-gradient(135deg, @okp_blue, @okp_accent 60%, @okp_accent_bright);
            box-shadow: 0 1px 2px alpha(@okp_accent, 0.32);
        }

        .is-dark .okp-up-next-list row.okp-add-bookmark-row:hover {
            background: linear-gradient(135deg, @okp_blue, @okp_accent_bright 60%, @okp_accent_bright);
            border-color: transparent;
            color: @okp_accent_ink;
        }

        .is-dark .okp-up-next-list row.okp-add-files-row {
            background: @okp_dark_card;
            border-color: @okp_dark_hairline_hi;
            color: @okp_accent_bright;
        }

        .is-dark .okp-up-next-list row.okp-add-files-row:hover {
            background: alpha(@okp_accent, 0.08);
            border-color: @okp_accent_bright;
            color: @okp_accent_bright;
        }

        .is-dark .okp-add-files-row .okp-add-files-icon {
            color: @okp_accent_bright;
        }

        .is-dark .okp-up-next-list row.okp-now-playing-pinned-row {
            background: alpha(@okp_accent, 0.11);
            color: @okp_accent_bright;
        }

        .is-dark .okp-now-playing-thumb {
            background: linear-gradient(135deg, @okp_accent, @okp_blue);
        }

        .is-dark .okp-now-playing-title {
            color: @okp_accent_bright;
        }

        .is-dark .okp-now-playing-state {
            color: @okp_accent_bright;
        }

        .is-dark .okp-up-next-row.is-current .okp-chapter-thumb {
            border-color: alpha(@okp_accent, 0.37);
        }

        .is-dark .okp-up-next-row:hover {
            background: alpha(@okp_accent, 0.08);
        }

        .is-dark .okp-up-next-row.is-current {
            background: alpha(@okp_accent, 0.11);
            border-color: transparent;
            color: @okp_accent_bright;
        }

        .is-dark .okp-up-next-row.is-behind {
            background: transparent;
            color: @okp_snow_faint;
        }

        .is-dark .okp-up-next-row.is-behind:hover {
            background: alpha(@okp_accent, 0.08);
            color: @okp_snow_mute;
        }

        .is-dark .okp-up-next-row.is-drop-target {
            background: alpha(@okp_accent, 0.16);
            border-color: alpha(@okp_accent, 0.60);
        }

        .is-dark .okp-up-next-drag-handle {
            color: alpha(@okp_snow, 0.28);
        }

        .is-dark .okp-up-next-row:hover .okp-up-next-drag-handle,
        .is-dark .okp-up-next-row.is-drop-target .okp-up-next-drag-handle {
            color: @okp_snow_mute;
        }

        .is-dark .okp-up-next-index {
            color: @okp_snow_faint;
        }

        .is-dark .okp-up-next-source-icon {
            color: @okp_snow_mute;
        }

        .is-dark .okp-up-next-row.is-current .okp-up-next-source-icon {
            color: @okp_accent_bright;
        }

        .is-dark .okp-next-badge {
            background: alpha(@okp_accent, 0.11);
            color: @okp_accent_bright;
        }

        .is-dark .okp-up-next-marker {
            color: @okp_snow_mute;
        }

        .is-dark .okp-up-next-row.is-current .okp-up-next-marker {
            color: @okp_accent_bright;
        }

        .is-dark menubutton.okp-up-next-actions-menu > button {
            color: @okp_snow_mute;
        }

        .is-dark menubutton.okp-up-next-actions-menu > button:hover {
            background: alpha(@okp_accent, 0.08);
            color: @okp_snow;
        }

        .is-dark .okp-up-next-actions-popover {
            background: @okp_dark_card;
        }

        .is-dark button.okp-up-next-menu-action {
            color: @okp_snow_dim;
        }

        .is-dark button.okp-up-next-menu-action:hover {
            background: alpha(@okp_accent, 0.08);
        }

        .is-dark button.okp-up-next-action-button {
            color: @okp_snow_mute;
        }

        .is-dark button.okp-up-next-action-button:hover {
            background: alpha(@okp_accent, 0.08);
            color: @okp_snow;
        }

        .is-dark button.okp-up-next-action-button:disabled {
            color: alpha(@okp_snow, 0.18);
        }

        .okp-up-next-panel.is-dark scrollbar slider {
            background: alpha(@okp_snow, 0.22);
        }

        .is-dark .okp-track-popover-content {
            background: @okp_dark_card;
            color: @okp_snow;
        }

        popover.okp-track-popover.is-dark > contents,
        popover.okp-track-popover.is-dark contents {
            background: @okp_dark_card;
            border: 1px solid @okp_dark_hairline;
            box-shadow: 0 16px 50px rgba(0, 0, 0, 0.45);
        }

        .okp-command-surface {
            padding: 6px;
            background: @okp_light_card;
        }

        entry.okp-command-search {
            min-height: 24px;
            margin: 0 0 6px 0;
            padding: 4px 10px 4px 0;
            border-radius: 8px;
            border: 1px solid @okp_light_hairline_hi;
            background: @okp_light_card;
            color: @okp_ink;
            box-shadow: none;
            font-size: 12px;
        }

        entry.okp-command-search > image {
            margin: 0 6px 0 8px;
            color: @okp_ink_faint;
        }

        entry.okp-command-search > text > placeholder {
            color: @okp_ink_faint;
        }

        entry.okp-command-search:hover {
            background: @okp_light_bg;
            border-color: @okp_light_hairline_hi;
        }

        entry.okp-command-search:focus-within {
            border-color: @okp_accent;
            box-shadow: 0 0 0 3px alpha(@okp_accent, 0.30);
            background: @okp_light_card;
        }

        entry.okp-command-search:disabled {
            background: @okp_light_rail;
            border-color: @okp_light_hairline;
            color: alpha(@okp_ink, 0.42);
        }

        entry.okp-command-search:disabled > image {
            color: alpha(@okp_ink, 0.30);
        }

        .okp-command-scroll,
        .okp-command-results {
            background: transparent;
        }

        .okp-command-group-title {
            margin: 10px 8px 4px 8px;
            color: @okp_ink_faint;
            font-size: 11px;
            font-weight: 600;
            letter-spacing: 0.05em;
            text-transform: uppercase;
        }

        separator.okp-command-separator {
            min-height: 1px;
            margin: 4px 8px;
            background: @okp_light_hairline;
        }

        button.okp-command-row {
            min-height: 28px;
            padding: 3px 10px;
            border-radius: 8px;
            border: 1px solid transparent;
            background: transparent;
            box-shadow: none;
            color: @okp_ink_dim;
            font-size: 13px;
        }

        button.okp-command-row:hover {
            background: alpha(@okp_navy, 0.06);
            color: @okp_ink;
        }

        button.okp-command-row:active {
            background: alpha(@okp_navy, 0.10);
        }

        button.okp-command-row.is-selected {
            background: alpha(@okp_navy, 0.09);
            border-color: transparent;
            color: @okp_navy;
            font-weight: 600;
        }

        button.okp-command-row:disabled {
            background: transparent;
            border-color: transparent;
            color: alpha(@okp_ink, 0.34);
        }

        button.okp-command-row:focus-visible {
            outline: none;
            box-shadow: inset 0 0 0 3px alpha(@okp_accent, 0.30);
        }

        button.okp-command-back-row {
            margin-bottom: 4px;
            font-weight: 700;
        }

        .okp-command-submenu-arrow {
            color: @okp_ink_faint;
        }

        .okp-command-row-label {
            color: inherit;
        }

        .okp-command-shortcut {
            color: @okp_ink_faint;
            font-family: 'JetBrains Mono', 'Cascadia Mono', monospace;
            font-size: 11px;
            font-feature-settings: 'tnum';
        }

        .okp-command-no-results {
            min-height: 116px;
            padding: 28px 18px;
        }

        .okp-command-no-results-title {
            color: @okp_ink;
            font-size: 13px;
            font-weight: 600;
        }

        .okp-command-no-results-hint {
            color: @okp_ink_faint;
            font-size: 11.5px;
        }

        popover.okp-command-popover.is-dark > contents,
        popover.okp-command-popover.is-dark contents,
        popover.okp-command-popover.is-dark .okp-command-surface {
            background: @okp_dark_card;
            border-color: @okp_dark_hairline_hi;
        }

        popover.okp-command-popover.is-dark entry.okp-command-search {
            background: @okp_settings_dark;
            border-color: @okp_dark_hairline_hi;
            color: @okp_snow;
        }

        popover.okp-command-popover.is-dark entry.okp-command-search > image {
            color: @okp_snow_faint;
        }

        popover.okp-command-popover.is-dark entry.okp-command-search > text > placeholder {
            color: @okp_snow_faint;
        }

        popover.okp-command-popover.is-dark entry.okp-command-search:hover {
            background: @okp_dark_rail;
            border-color: @okp_dark_hairline_hi;
        }

        popover.okp-command-popover.is-dark entry.okp-command-search:focus-within {
            border-color: @okp_accent;
            box-shadow: 0 0 0 3px alpha(@okp_accent, 0.30);
            background: @okp_settings_dark;
        }

        popover.okp-command-popover.is-dark entry.okp-command-search:disabled {
            background: @okp_dark_card;
            border-color: @okp_dark_hairline;
            color: @okp_snow_faint;
        }

        popover.okp-command-popover.is-dark entry.okp-command-search:disabled > image {
            color: alpha(@okp_snow_faint, 0.60);
        }

        popover.okp-command-popover.is-high-contrast entry.okp-command-search {
            min-height: 24px;
            border: 1px solid #000000;
            background: #ffffff;
            color: #000000;
        }

        popover.okp-command-popover.is-high-contrast entry.okp-command-search > image {
            color: #000000;
        }

        popover.okp-command-popover.is-high-contrast entry.okp-command-search > text > placeholder {
            color: #000000;
        }

        popover.okp-command-popover.is-high-contrast entry.okp-command-search:hover {
            background: #f0f0f0;
        }

        popover.okp-command-popover.is-high-contrast entry.okp-command-search:focus-within {
            border-color: #000000;
            box-shadow: inset 0 0 0 1px #000000;
            background: #ffffff;
        }

        popover.okp-command-popover.is-high-contrast entry.okp-command-search:disabled {
            background: #e8e8e8;
            border-color: #000000;
            color: #000000;
        }

        popover.okp-command-popover.is-dark.is-high-contrast entry.okp-command-search,
        popover.okp-command-popover.is-high-contrast.is-dark entry.okp-command-search {
            background: #000000;
            border-color: #ffffff;
            color: #ffffff;
        }

        popover.okp-command-popover.is-dark.is-high-contrast entry.okp-command-search > image,
        popover.okp-command-popover.is-high-contrast.is-dark entry.okp-command-search > image {
            color: #ffffff;
        }

        popover.okp-command-popover.is-dark.is-high-contrast entry.okp-command-search > text > placeholder,
        popover.okp-command-popover.is-high-contrast.is-dark entry.okp-command-search > text > placeholder {
            color: #ffffff;
        }

        popover.okp-command-popover.is-dark.is-high-contrast entry.okp-command-search:hover,
        popover.okp-command-popover.is-high-contrast.is-dark entry.okp-command-search:hover {
            background: @okp_ink;
        }

        popover.okp-command-popover.is-dark.is-high-contrast entry.okp-command-search:focus-within,
        popover.okp-command-popover.is-high-contrast.is-dark entry.okp-command-search:focus-within {
            border-color: #ffffff;
            box-shadow: inset 0 0 0 1px #ffffff;
            background: #000000;
        }

        popover.okp-command-popover.is-dark .okp-command-group-title,
        popover.okp-command-popover.is-dark .okp-command-shortcut,
        popover.okp-command-popover.is-dark .okp-command-no-results-hint {
            color: @okp_snow_faint;
        }

        popover.okp-command-popover.is-dark button.okp-command-row {
            color: @okp_snow_dim;
        }

        popover.okp-command-popover.is-dark button.okp-command-row:hover {
            background: alpha(@okp_accent, 0.08);
            color: @okp_snow;
        }

        popover.okp-command-popover.is-dark button.okp-command-row:active {
            background: alpha(@okp_accent, 0.14);
        }

        popover.okp-command-popover.is-dark button.okp-command-row.is-selected {
            background: alpha(@okp_accent, 0.16);
            border-color: transparent;
            color: @okp_accent;
        }

        popover.okp-command-popover.is-dark button.okp-command-row:disabled {
            color: alpha(@okp_snow, 0.30);
        }

        popover.okp-command-popover.is-dark separator.okp-command-separator {
            background: @okp_dark_hairline;
        }

        popover.okp-command-popover.is-dark .okp-command-submenu-arrow {
            color: @okp_snow_faint;
        }

        popover.okp-command-popover.is-dark .okp-command-no-results-title {
            color: @okp_snow;
        }

        .okp-track-popover-scroll {
            background: @okp_light_card;
        }

        .okp-track-popover-title {
            min-height: 28px;
            margin: 0 8px 4px 8px;
            color: @okp_ink;
            font-size: 14px;
            font-weight: 600;
        }

        .okp-track-group-title {
            margin: 10px 8px 4px 8px;
            color: @okp_ink_faint;
            font-size: 11px;
            font-weight: 600;
            letter-spacing: 0.05em;
            text-transform: uppercase;
        }

        .okp-track-subgroup-title {
            margin: 6px 8px 2px 8px;
            color: @okp_ink_faint;
            font-size: 10.5px;
            font-weight: 600;
        }

        button.okp-track-row {
            min-height: 32px;
            padding: 5px 10px;
            border-radius: 8px;
            background: transparent;
            border: 1px solid transparent;
            box-shadow: none;
            color: @okp_ink_dim;
            font-size: 13px;
        }

        button.okp-track-row:hover {
            background: alpha(@okp_navy, 0.06);
            color: @okp_ink;
        }

        button.okp-track-row:active {
            background: alpha(@okp_navy, 0.10);
        }

        button.okp-track-row.is-selected {
            background: alpha(@okp_navy, 0.09);
            border-color: transparent;
            color: @okp_navy;
        }

        button.okp-track-row.is-selected .okp-track-row-label {
            font-weight: 600;
        }

        button.okp-audio-track-row {
            min-height: 42px;
        }

        .okp-audio-track-name {
            color: inherit;
            font-size: 12px;
        }

        .okp-audio-track-detail {
            color: @okp_ink_faint;
            font-size: 10.5px;
        }

        button.okp-audio-track-row.is-selected .okp-audio-track-name {
            font-weight: 600;
        }

        button.okp-track-row:disabled {
            background: transparent;
            border-color: transparent;
            color: alpha(@okp_ink, 0.38);
        }

        button.okp-track-row:focus-visible {
            outline: none;
            box-shadow: inset 0 0 0 3px alpha(@okp_accent, 0.30);
        }

        .okp-track-check {
            min-width: 14px;
            min-height: 14px;
        }

        .okp-speed-popover button.okp-track-row {
            min-height: 30px;
            padding-left: 5px;
            padding-right: 5px;
        }

        button.okp-scribe-row {
            color: @okp_link;
        }

        button.okp-scribe-row:disabled {
            color: alpha(@okp_link, 0.54);
        }

        .okp-scribe-spinner {
            min-width: 16px;
            min-height: 16px;
            color: alpha(@okp_link, 0.72);
        }

        .okp-subtitle-action-icon {
            min-width: 16px;
            min-height: 16px;
        }

        .okp-subtitle-action-badge {
            min-height: 16px;
            padding: 0 6px;
            border-radius: 999px;
            background: @okp_light_rail;
            color: @okp_ink_mute;
            font-size: 9px;
            font-weight: 700;
        }

        button.okp-scribe-row .okp-subtitle-action-badge {
            background: alpha(@okp_link, 0.12);
            color: @okp_link;
        }

        button.okp-online-subtitle-row:disabled {
            color: alpha(@okp_ink, 0.38);
        }

        .okp-track-empty {
            min-height: 32px;
            margin: 0;
            padding: 5px 8px 5px 32px;
            border-radius: 8px;
            border: 1px solid transparent;
            background: transparent;
            color: @okp_ink_faint;
            font-size: 13px;
        }

        .okp-track-divider {
            margin: 6px 8px;
            background: @okp_light_hairline;
        }

        .okp-quick-delay-row {
            min-height: 34px;
            margin-top: 2px;
            padding: 2px 4px 0 7px;
        }

        .okp-quick-delay-label {
            color: @okp_ink_mute;
            font-size: 13px;
        }

        .okp-quick-delay-value {
            color: @okp_navy;
            font-family: 'JetBrains Mono', 'Cascadia Mono', monospace;
            font-size: 12px;
            font-weight: 600;
            font-feature-settings: 'tnum';
        }

        button.okp-quick-delay-button {
            min-width: 26px;
            min-height: 26px;
            padding: 0;
            border-radius: 999px;
            border: 1px solid transparent;
            background: @okp_light_rail;
            box-shadow: none;
        }

        button.okp-quick-delay-button:hover {
            background: alpha(@okp_navy, 0.10);
        }

        button.okp-quick-delay-button:focus-visible {
            outline: none;
            box-shadow: 0 0 0 3px alpha(@okp_accent, 0.30);
        }

        .okp-quick-style-row {
            min-height: 34px;
            padding: 2px 4px 2px 7px;
        }

        .okp-subtitle-preset-status {
            margin: 2px 8px 0;
            color: @okp_ink_faint;
            font-size: 10.5px;
            line-height: 1.25;
        }

        button.okp-quick-style-button {
            min-height: 26px;
            padding: 2px 10px;
            border-radius: 999px;
            border: 1px solid transparent;
            background: @okp_light_rail;
            box-shadow: none;
            color: @okp_ink_dim;
            font-size: 11.5px;
            font-weight: 600;
        }

        button.okp-quick-style-button:hover {
            background: alpha(@okp_navy, 0.10);
        }

        button.okp-quick-style-button:disabled {
            background: @okp_light_rail;
            color: @okp_ink_faint;
        }

        .okp-quick-preference-footer {
            margin: 4px -8px -8px;
            padding: 8px 14px;
            border-top: 1px solid @okp_light_hairline;
            color: @okp_ink_faint;
            font-size: 10.5px;
        }
        /* Dark scheme for the track popover contents, generated from the light
           rules above the same way as the panel's. */
        .is-dark .okp-track-popover-scroll {
            background: @okp_dark_card;
        }

        .is-dark .okp-track-popover-title {
            color: @okp_snow;
        }

        .is-dark .okp-track-group-title {
            color: @okp_snow_faint;
        }

        .is-dark .okp-track-subgroup-title {
            color: @okp_snow_faint;
        }

        .is-dark button.okp-track-row {
            color: @okp_snow_dim;
        }

        .is-dark button.okp-track-row:hover {
            background: alpha(@okp_accent, 0.08);
            color: @okp_snow;
        }

        .is-dark button.okp-track-row:active {
            background: alpha(@okp_accent, 0.12);
        }

        .is-dark button.okp-track-row.is-selected {
            background: alpha(@okp_accent, 0.11);
            border-color: transparent;
            color: @okp_accent_bright;
        }

        .is-dark .okp-audio-track-detail {
            color: @okp_snow_faint;
        }

        .is-dark button.okp-track-row:disabled {
            background: transparent;
            border-color: transparent;
            color: alpha(@okp_snow, 0.38);
        }

        .is-dark button.okp-track-row:focus-visible {
            box-shadow: inset 0 0 0 3px alpha(@okp_accent, 0.30);
        }

        .is-dark button.okp-scribe-row {
            color: @okp_accent_bright;
        }

        .is-dark button.okp-scribe-row:disabled {
            color: alpha(@okp_accent_bright, 0.54);
        }

        .is-dark .okp-scribe-spinner {
            color: alpha(@okp_accent_bright, 0.72);
        }

        .is-dark .okp-subtitle-action-badge {
            background: @okp_dark_rail;
            color: @okp_snow_mute;
        }

        .is-dark button.okp-scribe-row .okp-subtitle-action-badge {
            background: alpha(@okp_accent_bright, 0.12);
            color: @okp_accent_bright;
        }

        .is-dark button.okp-online-subtitle-row:disabled {
            color: alpha(@okp_snow, 0.38);
        }

        .is-dark .okp-track-empty {
            color: @okp_snow_faint;
        }

        .is-dark .okp-track-divider {
            background: @okp_dark_hairline;
        }

        .is-dark .okp-quick-delay-label {
            color: @okp_snow_mute;
        }

        .is-dark .okp-quick-delay-value {
            color: @okp_accent_bright;
        }

        .is-dark button.okp-quick-delay-button {
            background: @okp_dark_rail;
        }

        .is-dark button.okp-quick-delay-button:hover {
            background: alpha(@okp_accent, 0.12);
        }

        .is-dark button.okp-quick-delay-button:focus-visible {
            box-shadow: 0 0 0 3px alpha(@okp_accent, 0.30);
        }

        .is-dark .okp-subtitle-preset-status {
            color: @okp_snow_faint;
        }

        .is-dark button.okp-quick-style-button {
            background: @okp_dark_rail;
            color: @okp_snow_dim;
        }

        .is-dark button.okp-quick-style-button:hover {
            background: alpha(@okp_accent, 0.12);
        }

        .is-dark button.okp-quick-style-button:disabled {
            background: @okp_dark_rail;
            color: @okp_snow_faint;
        }

        .is-dark .okp-quick-preference-footer {
            border-top: 1px solid @okp_dark_hairline;
            color: @okp_snow_faint;
        }


        .okp-settings-audio-delay-row {
            margin: 0;
            padding: 10px 0;
            border-top: 1px solid @okp_light_hairline;
        }

        .okp-settings-audio-delay-row .okp-sub-adjust-label {
            color: @okp_ink_mute;
            font-size: 13px;
        }

        .okp-settings-audio-delay-row entry.okp-sub-adjust-entry {
            min-width: 74px;
            min-height: 28px;
            padding: 4px 8px;
            border-radius: 8px;
            border: 1px solid @okp_light_hairline_hi;
            background: @okp_light_card;
            color: @okp_ink;
            font-family: 'JetBrains Mono', 'Cascadia Mono', monospace;
            font-feature-settings: 'tnum';
        }

        .okp-settings-audio-delay-row entry.okp-sub-adjust-entry:focus {
            border-color: @okp_accent;
            box-shadow: 0 0 0 3px alpha(@okp_accent, 0.30);
        }

        .okp-settings-audio-delay-row entry.okp-sub-adjust-entry.is-error {
            border-color: alpha(@okp_danger, 0.88);
            box-shadow: 0 0 0 2px alpha(@okp_danger, 0.14);
        }

        .okp-settings-audio-delay-row .okp-sub-adjust-unit {
            color: @okp_ink_mute;
            font-size: 12px;
        }

        .okp-settings-audio-delay-row .okp-sub-adjust-button {
            min-width: 44px;
            min-height: 28px;
            padding: 4px 8px;
            border-radius: 999px;
            border: 1px solid @okp_light_hairline_hi;
            background: @okp_light_card;
            color: @okp_ink;
        }

        .okp-settings-audio-delay-row .okp-sub-adjust-button:hover {
            background: @okp_light_rail;
        }

        window.okp-companion-window,
        window.okp-companion-window > contents {
            background: transparent;
            box-shadow: none;
            border: none;
        }

        .okp-companion-resize-zone {
            background: transparent;
        }

        .okp-media-info-card {
            background: @okp_settings_light;
            color: @okp_ink;
            border: 1px solid @okp_light_hairline;
            border-radius: 16px;
            box-shadow: 0 16px 50px rgba(12, 20, 36, 0.18);
        }

        .okp-media-info-header {
            padding: 17px 20px 15px;
            border-bottom: 1px solid @okp_light_hairline;
        }

        .okp-media-info-identity {
            min-width: 38px;
            min-height: 38px;
            border-radius: 10px;
            background: linear-gradient(135deg, @okp_navy, @okp_blue 55%, @okp_accent);
            box-shadow: 0 1px 2px alpha(@okp_navy, 0.30);
            color: #ffffff;
        }

        .okp-media-info-title {
            color: @okp_ink;
            font-family: 'Noto Sans', 'Inter', 'Cantarell', sans-serif;
            font-size: 17px;
            font-weight: 600;
            letter-spacing: -0.01em;
        }

        .okp-media-info-subtitle,
        .okp-media-info-path {
            color: @okp_ink_faint;
            font-family: 'JetBrains Mono', 'Cascadia Mono', monospace;
            font-size: 12px;
            font-weight: 400;
        }

        button.okp-media-info-close {
            min-width: 30px;
            min-height: 30px;
            padding: 0;
            border: none;
            border-radius: 999px;
            background: transparent;
            box-shadow: none;
            color: @okp_ink_mute;
        }

        button.okp-media-info-close:hover {
            background: alpha(@okp_navy, 0.06);
            color: @okp_ink;
        }

        .okp-media-info-tabs {
            padding: 13px 20px 0;
        }

        .okp-media-info-tab-strip {
            padding: 3px;
            border-radius: 999px;
            background: @okp_light_rail;
            border: 1px solid @okp_light_hairline;
        }

        button.okp-media-info-tab {
            min-width: 137px;
            min-height: 28px;
            padding: 0 10px;
            border: none;
            border-radius: 999px;
            background: transparent;
            box-shadow: none;
            color: @okp_ink_mute;
            font-family: 'Noto Sans', 'Inter', 'Cantarell', sans-serif;
            font-size: 12px;
            font-weight: 600;
        }

        button.okp-media-info-tab.is-active {
            background: @okp_light_card;
            box-shadow: 0 1px 0 rgba(12, 20, 36, 0.04), 0 8px 24px rgba(12, 20, 36, 0.05);
            color: @okp_ink;
        }

        button.okp-media-info-tab:hover:not(.is-active) {
            background: transparent;
            color: @okp_ink;
        }

        .okp-media-info-stack,
        .okp-media-info-scroller,
        .okp-media-info-scroller > viewport {
            background: @okp_settings_light;
        }

        .okp-media-info-content {
            padding: 16px 20px 20px;
        }

        .okp-media-info-card .okp-media-info-grid {
            background: transparent;
        }

        .okp-media-info-scroller scrollbar {
            background: transparent;
            border: none;
        }

        .okp-media-info-scroller scrollbar trough {
            background: transparent;
            border: none;
        }

        .okp-media-info-scroller scrollbar slider {
            min-width: 4px;
            border-radius: 999px;
            background: alpha(@okp_ink, 0.22);
        }

        .okp-media-info-card .okp-info-section {
            padding: 16px 20px;
            border-radius: 12px;
            background: @okp_light_card;
            border: 1px solid @okp_light_hairline;
            box-shadow: 0 1px 2px rgba(12, 20, 36, 0.04), 0 12px 32px rgba(12, 20, 36, 0.06);
        }

        .okp-media-info-card .okp-info-section-title {
            margin-bottom: 12px;
            color: @okp_ink_faint;
            font-size: 11px;
            font-weight: 600;
            letter-spacing: 0.05em;
            text-transform: uppercase;
        }

        .okp-media-info-card .okp-info-row {
            min-height: 18px;
        }

        .okp-media-info-card .okp-info-label {
            color: @okp_ink_mute;
            font-family: 'Noto Sans', 'Inter', 'Cantarell', sans-serif;
            font-size: 13px;
            font-weight: 400;
        }

        .okp-media-info-card .okp-info-value {
            color: @okp_ink;
            font-family: 'JetBrains Mono', 'Cascadia Mono', monospace;
            font-size: 12px;
            font-weight: 500;
            font-feature-settings: 'tnum';
        }

        .okp-media-info-card .okp-info-row.is-highlight .okp-info-value {
            color: @okp_navy;
            font-weight: 600;
        }

        .okp-media-info-card .okp-info-track-row {
            min-height: 42px;
            padding: 9px 11px;
            border-radius: 8px;
            background: transparent;
            border: 1px solid transparent;
        }

        .okp-media-info-card .okp-info-track-row.is-selected {
            background: alpha(@okp_navy, 0.09);
            border-color: transparent;
        }

        .okp-media-info-card .okp-info-track-kind {
            min-width: 34px;
            color: @okp_ink_faint;
            font-family: 'JetBrains Mono', 'Cascadia Mono', monospace;
            font-size: 10px;
            font-weight: 500;
        }

        .okp-media-info-card .okp-info-track-current {
            background: alpha(@okp_navy, 0.09);
            color: @okp_navy;
            font-size: 9px;
        }

        .okp-media-info-empty {
            color: @okp_ink_mute;
            font-family: 'Noto Sans', 'Inter', 'Cantarell', sans-serif;
            font-size: 13px;
        }

        .okp-media-info-footer {
            padding: 12px 20px;
            border-top: 1px solid @okp_light_hairline;
        }

        .okp-media-info-path {
            color: @okp_ink_faint;
            font-size: 11px;
        }

        button.okp-media-info-copy,
        button.okp-media-info-done {
            min-height: 34px;
            padding: 0 18px;
            border-radius: 999px;
            box-shadow: none;
            font-family: 'Noto Sans', 'Inter', 'Cantarell', sans-serif;
            font-size: 13px;
            font-weight: 600;
        }

        button.okp-media-info-copy {
            border: 1px solid @okp_light_hairline_hi;
            background: @okp_light_card;
            color: @okp_ink;
        }

        button.okp-media-info-copy:hover {
            background: @okp_light_rail;
        }

        button.okp-media-info-done {
            min-width: 70px;
            padding: 0 20px;
            border: none;
            background: linear-gradient(135deg, @okp_navy, @okp_blue 55%, @okp_accent);
            color: #ffffff;
            box-shadow: 0 1px 2px alpha(@okp_navy, 0.30);
        }

        button.okp-media-info-done:hover {
            background: linear-gradient(135deg, @okp_navy_hover, @okp_blue 55%, @okp_accent_bright);
        }

        button.okp-media-info-close:focus-visible,
        button.okp-media-info-tab:focus-visible,
        button.okp-media-info-copy:focus-visible,
        button.okp-media-info-done:focus-visible {
            outline: none;
            box-shadow: 0 0 0 3px alpha(@okp_accent, 0.30);
        }

        window.okp-media-info-window.is-dark .okp-media-info-card,
        window.okp-media-info-window.is-dark .okp-media-info-stack,
        window.okp-media-info-window.is-dark .okp-media-info-scroller,
        window.okp-media-info-window.is-dark .okp-media-info-scroller > viewport {
            background: @okp_dark_card;
            color: @okp_snow;
        }

        window.okp-media-info-window.is-dark .okp-media-info-card {
            border-color: @okp_dark_hairline_hi;
        }

        window.okp-media-info-window.is-dark .okp-media-info-header,
        window.okp-media-info-window.is-dark .okp-media-info-footer {
            border-color: @okp_dark_hairline;
        }

        window.okp-media-info-window.is-dark .okp-media-info-title,
        window.okp-media-info-window.is-dark .okp-info-value,
        window.okp-media-info-window.is-dark button.okp-media-info-copy {
            color: @okp_snow;
        }

        window.okp-media-info-window.is-dark .okp-media-info-subtitle,
        window.okp-media-info-window.is-dark .okp-media-info-path,
        window.okp-media-info-window.is-dark .okp-info-label,
        window.okp-media-info-window.is-dark .okp-info-track-kind,
        window.okp-media-info-window.is-dark .okp-media-info-empty {
            color: @okp_snow_mute;
        }

        window.okp-media-info-window.is-dark .okp-media-info-tab-strip,
        window.okp-media-info-window.is-dark button.okp-media-info-close,
        window.okp-media-info-window.is-dark button.okp-media-info-copy {
            background: @okp_dark_rail;
            border-color: @okp_dark_hairline;
        }

        window.okp-media-info-window.is-dark button.okp-media-info-tab {
            color: @okp_snow_mute;
        }

        window.okp-media-info-window.is-dark button.okp-media-info-tab.is-active,
        window.okp-media-info-window.is-dark .okp-info-section {
            background: @okp_dark_rail;
            border-color: @okp_dark_hairline;
            box-shadow: none;
            color: @okp_snow;
        }

        window.okp-media-info-window.is-dark button.okp-media-info-tab.is-active,
        window.okp-media-info-window.is-dark .okp-info-section-title,
        window.okp-media-info-window.is-dark .okp-info-row.is-highlight .okp-info-value,
        window.okp-media-info-window.is-dark .okp-info-track-current {
            color: @okp_accent;
        }

        window.okp-media-info-window.is-dark .okp-media-info-scroller scrollbar slider {
            background: alpha(@okp_snow, 0.24);
        }

        window.okp-media-info-window.is-high-contrast .okp-media-info-card,
        window.okp-media-info-window.is-high-contrast .okp-media-info-stack,
        window.okp-media-info-window.is-high-contrast .okp-media-info-scroller,
        window.okp-media-info-window.is-high-contrast .okp-media-info-scroller > viewport,
        window.okp-media-info-window.is-high-contrast .okp-info-section {
            background: #000000;
            border-color: #ffffff;
            color: #ffffff;
        }

        window.okp-media-info-window.is-high-contrast .okp-media-info-tab-strip {
            background: #000000;
            border-color: #ffffff;
        }

        window.okp-media-info-window.is-high-contrast button.okp-media-info-tab.is-active {
            background: #ffffff;
            box-shadow: none;
            color: #000000;
        }

        window.okp-media-info-window.is-high-contrast .okp-media-info-title,
        window.okp-media-info-window.is-high-contrast .okp-media-info-subtitle,
        window.okp-media-info-window.is-high-contrast .okp-media-info-path,
        window.okp-media-info-window.is-high-contrast .okp-info-label,
        window.okp-media-info-window.is-high-contrast .okp-info-value,
        window.okp-media-info-window.is-high-contrast .okp-info-section-title,
        window.okp-media-info-window.is-high-contrast .okp-info-track-kind,
        window.okp-media-info-window.is-high-contrast .okp-media-info-empty,
        window.okp-media-info-window.is-high-contrast button.okp-media-info-tab,
        window.okp-media-info-window.is-high-contrast button.okp-media-info-copy,
        window.okp-media-info-window.is-high-contrast button.okp-media-info-close {
            color: #ffffff;
        }

        window.okp-command-dialog {
            background: @okp_dark_card;
            color: @okp_snow;
            border-radius: 12px;
        }

        window.okp-command-dialog > contents {
            background: @okp_dark_card;
        }

        .okp-command-dialog-title {
            color: @okp_snow;
            font-size: 17px;
            font-weight: 600;
            letter-spacing: -0.01em;
        }

        window.okp-command-dialog entry {
            min-height: 34px;
            border-radius: 8px;
            border: 1px solid @okp_dark_hairline_hi;
            background: @okp_settings_dark;
            color: @okp_snow;
            box-shadow: none;
        }

        window.okp-command-dialog button {
            min-width: 72px;
            min-height: 34px;
            padding: 0 16px;
            border-radius: 999px;
            border: 1px solid @okp_dark_hairline_hi;
            background: @okp_dark_rail;
            color: @okp_snow;
            box-shadow: none;
        }

        window.okp-command-dialog button:hover {
            background: @okp_dark_inset;
            color: @okp_snow;
        }

        window.okp-command-dialog button:active {
            background: alpha(@okp_accent, 0.22);
            border-color: alpha(@okp_accent, 0.45);
        }

        window.okp-command-dialog .okp-info-label {
            color: @okp_snow_mute;
            font-family: 'Noto Sans', 'Inter', 'Cantarell', sans-serif;
            font-size: 12px;
            font-weight: 500;
        }

        /* Missing-tooling hint (e.g. the YouTube resolver is not installed): a warm,
           attention-drawing tint that stays calm — the state is informational, not an error. */
        window.okp-command-dialog .okp-info-label.okp-info-label-muted {
            color: @okp_warning_dark;
        }

        .okp-settings-window {
            background: transparent;
        }

        window.okp-settings-window > contents {
            background: transparent;
            box-shadow: none;
            border: none;
        }

        window.okp-settings-window headerbar,
        window.okp-settings-window decoration {
            min-height: 0;
            margin: 0;
            padding: 0;
            border: none;
            background: transparent;
            box-shadow: none;
        }

        .okp-settings-root {
            background: @okp_settings_light;
            color: @okp_ink;
            border: none;
            border-radius: 0;
        }

        window.okp-settings-window {
            background: @okp_settings_light;
            font-family: 'Noto Sans', 'Inter', 'Cantarell', sans-serif;
        }

        .okp-settings-titlebar {
            min-height: 41px;
            padding: 0 144px 0 16px;
            background: @okp_settings_light;
            border-bottom: 1px solid @okp_light_hairline;
        }

        .okp-settings-titlebar-label {
            color: @okp_ink_mute;
            font-family: 'Noto Sans', 'Inter', 'Cantarell', sans-serif;
            font-size: 12.5px;
            font-weight: 600;
        }

        .okp-settings-body {
            background: @okp_settings_light;
        }

        .okp-settings-rail-frame {
            background: @okp_light_rail;
        }

        .okp-settings-rail {
            padding: 12px 10px 0;
            background: @okp_light_rail;
            border-right: 1px solid @okp_light_hairline;
        }

        entry.okp-settings-search {
            min-height: 16px;
            margin-bottom: 6px;
            padding: 7px 10px;
            border-radius: 8px;
            background: @okp_light_card;
            border: 1px solid @okp_light_hairline_hi;
            box-shadow: none;
            color: @okp_ink_faint;
            font-family: 'Noto Sans', 'Inter', 'Cantarell', sans-serif;
            font-size: 12px;
            font-weight: 400;
        }

        entry.okp-settings-search:focus {
            border-color: @okp_accent;
            box-shadow: 0 0 0 3px alpha(@okp_accent, 0.30);
        }

        /* The Windows rail states the distance from the magnifier to the query outright:
           SettingsWindow.xaml lays the icon and the text out in a horizontal StackPanel
           whose spacing is 8. GTK leaves 6, which reads as no gap at all beside a 16px
           glyph, so the remaining 2 is added here.

           It is added to the text rather than to the icon because GtkImage keeps its
           request square: a horizontal margin on the icon is charged to the entry's height
           as well, which measures 32px tall untouched, 34px with 2px on the icon and 40px
           with 8px. GtkText has no such coupling, so this moves the gap and nothing else. */
        entry.okp-settings-search > text {
            padding-left: 2px;
        }

        button.okp-settings-search-result {
            min-height: 40px;
            margin: -2px 0 5px;
            padding: 6px 10px;
            border: none;
            border-radius: 8px;
            background: alpha(@okp_navy, 0.09);
            box-shadow: none;
            color: @okp_ink;
        }

        button.okp-settings-search-result:hover {
            background: alpha(@okp_navy, 0.13);
        }

        .okp-settings-search-result-label {
            color: inherit;
            font-family: 'Noto Sans', 'Inter', 'Cantarell', sans-serif;
            font-size: 11.5px;
            font-weight: 600;
        }

        .okp-settings-search-result-page {
            color: @okp_ink_faint;
            font-family: 'Noto Sans', 'Inter', 'Cantarell', sans-serif;
            font-size: 10.5px;
            font-weight: 400;
        }

        entry.okp-shortcuts-search {
            min-height: 30px;
            margin-bottom: 2px;
            padding: 6px 10px;
            border-radius: 8px;
            background: @okp_light_card;
            border: 1px solid @okp_light_hairline_hi;
            box-shadow: none;
            color: @okp_ink;
            font-family: 'Noto Sans', 'Inter', 'Cantarell', sans-serif;
            font-size: 12px;
        }

        entry.okp-shortcuts-search:focus {
            border-color: @okp_accent;
            box-shadow: 0 0 0 3px alpha(@okp_accent, 0.30);
        }

        .okp-settings-nav-row {
            min-height: 18px;
            padding: 8px 10px;
            border: none;
            border-radius: 8px;
            background: transparent;
            box-shadow: none;
            color: @okp_ink_mute;
            font-family: 'Noto Sans', 'Inter', 'Cantarell', sans-serif;
            font-size: 13px;
            font-weight: 500;
        }

        .okp-settings-nav-row:hover {
            background: alpha(@okp_navy, 0.06);
            color: @okp_ink;
        }

        .okp-settings-nav-row.is-selected {
            background: @okp_light_card;
            box-shadow: 0 1px 0 rgba(12, 20, 36, 0.04), 0 8px 24px rgba(12, 20, 36, 0.05);
            color: @okp_ink;
            font-weight: 600;
        }

        .okp-settings-nav-icon {
            min-width: 16px;
            min-height: 16px;
            color: inherit;
        }

        .okp-settings-footer-rule {
            background: @okp_light_hairline;
        }

        .okp-captionless-window-drag-layer {
            min-height: 42px;
            background: transparent;
        }

        .okp-settings-window-controls {
            min-height: 42px;
        }

        .okp-settings-window-control {
            min-width: 48px;
            min-height: 42px;
            padding: 0;
            border: none;
            border-radius: 0;
            background: transparent;
            box-shadow: none;
            color: @okp_ink_mute;
        }

        .okp-settings-window-control:hover {
            background: alpha(@okp_navy, 0.06);
        }

        .okp-settings-window-control-glyph {
            min-width: 10px;
            min-height: 10px;
            color: @okp_ink_mute;
        }

        button.okp-settings-window-control:hover .okp-settings-window-control-glyph {
            color: @okp_ink;
        }

        button.okp-settings-window-close:hover {
            background: @okp_danger;
        }

        button.okp-settings-window-close:hover .okp-settings-window-control-glyph {
            color: #ffffff;
        }

        .okp-settings-stack {
            background: @okp_settings_light;
        }

        .okp-settings-scroller {
            background: @okp_settings_light;
        }

        .okp-settings-page {
            padding: 28px 44px 28px 28px;
        }

        .okp-settings-content {
            padding-right: 4px;
        }

        .okp-about-pane {
            padding: 28px 44px 0 28px;
            background: @okp_settings_light;
        }

        .okp-about-identity {
            min-height: 94px;
            padding: 24px;
            border-radius: 16px;
            background-color: @okp_light_card;
            background-image: linear-gradient(135deg, alpha(@okp_navy, 0.07), alpha(@okp_blue, 0.05) 55%, alpha(@okp_accent, 0.06));
            border: 1px solid @okp_light_hairline;
            box-shadow: 0 1px 2px rgba(12, 20, 36, 0.04), 0 12px 32px rgba(12, 20, 36, 0.06);
        }

        .okp-about-illustration {
            min-width: 118px;
            min-height: 94px;
        }

        .okp-about-illustration-art {
            color: @okp_navy;
        }

        .okp-about-wordmark {
            color: @okp_ink;
            font-family: 'Noto Sans', 'Inter', 'Cantarell', sans-serif;
            font-size: 28px;
            letter-spacing: -0.02em;
        }

        .okp-about-wordmark-ok {
            font-weight: 700;
        }

        .okp-about-wordmark-player {
            font-weight: 400;
        }

        .okp-about-chip-row {
            margin-top: 10px;
        }

        .okp-about-version-chip {
            padding: 3px 10px;
            border-radius: 999px;
            background: @okp_light_rail;
            border: 1px solid @okp_light_hairline;
            color: @okp_ink_mute;
            font-family: 'JetBrains Mono', 'Cascadia Mono', monospace;
            font-size: 11.5px;
            font-weight: 600;
            font-feature-settings: 'tnum';
        }

        .okp-about-channel-chip {
            padding: 3px 10px;
            border-radius: 999px;
            background: alpha(@okp_accent, 0.14);
            border: 1px solid alpha(@okp_link, 0.42);
            color: @okp_link;
            font-family: 'Noto Sans', 'Inter', 'Cantarell', sans-serif;
            font-size: 11px;
            font-weight: 600;
            letter-spacing: 0;
        }

        .okp-about-tagline {
            margin-top: 11px;
            color: @okp_ink_dim;
            font-family: 'Noto Sans', 'Inter', 'Cantarell', sans-serif;
            font-size: 13px;
            font-weight: 400;
        }

        .okp-about-byline {
            margin-top: 3px;
            color: @okp_ink_faint;
            font-family: 'Noto Sans', 'Inter', 'Cantarell', sans-serif;
            font-size: 12px;
            font-weight: 400;
        }

        .okp-about-identity-divider {
            margin: 20px 0;
            background: transparent;
        }

        .okp-about-card {
            padding: 20px;
            border-radius: 12px;
            background: @okp_light_card;
            border: 1px solid @okp_light_hairline;
            box-shadow: 0 1px 2px rgba(12, 20, 36, 0.04), 0 12px 32px rgba(12, 20, 36, 0.06);
        }

        .okp-about-card-title {
            margin-bottom: 12px;
            color: @okp_ink_faint;
            font-family: 'Noto Sans', 'Inter', 'Cantarell', sans-serif;
            font-size: 11px;
            font-weight: 600;
            letter-spacing: 0.05em;
            text-transform: uppercase;
        }

        .okp-about-row {
            min-height: 14px;
        }

        .okp-about-row-label {
            color: @okp_ink_mute;
            font-family: 'Noto Sans', 'Inter', 'Cantarell', sans-serif;
            font-size: 13px;
            font-weight: 400;
        }

        .okp-about-row-detail {
            color: @okp_ink_faint;
            font-family: 'Noto Sans', 'Inter', 'Cantarell', sans-serif;
            font-size: 12px;
            font-weight: 400;
        }

        .okp-about-row-value,
        .okp-about-row-value-mono {
            color: @okp_ink;
            font-size: 12.5px;
            font-weight: 500;
        }

        .okp-about-row-value {
            font-family: 'Noto Sans', 'Inter', 'Cantarell', sans-serif;
        }

        .okp-about-row-value-mono {
            font-family: 'JetBrains Mono', 'Cascadia Mono', monospace;
            font-size: 12px;
            font-feature-settings: 'tnum';
        }

        .okp-about-host-grid {
            min-width: 0;
        }

        .okp-about-tag {
            padding: 2px 7px;
            border-radius: 999px;
            background: @okp_light_rail;
            border: 1px solid @okp_light_hairline;
            color: @okp_ink_faint;
            font-family: 'Noto Sans', 'Inter', 'Cantarell', sans-serif;
            font-size: 10px;
            font-weight: 600;
            letter-spacing: 0;
        }

        .okp-about-tag.is-ok {
            background: alpha(@okp_ok, 0.10);
            border-color: alpha(@okp_ok, 0.42);
            color: @okp_ok;
        }

        .okp-about-copy-button {
            min-height: 34px;
            padding: 0 18px;
            border-radius: 999px;
            background: @okp_light_card;
            border: 1px solid @okp_light_hairline_hi;
            box-shadow: none;
            color: @okp_ink;
            font-family: 'Noto Sans', 'Inter', 'Cantarell', sans-serif;
            font-size: 13px;
            font-weight: 600;
        }

        .okp-about-copy-button:hover {
            background: @okp_light_rail;
        }

        .okp-about-check-button {
            min-width: 132px;
            min-height: 34px;
            padding: 0 18px;
            border-radius: 999px;
            background: @okp_light_card;
            border: 1px solid @okp_light_hairline_hi;
            box-shadow: none;
            color: @okp_ink;
            font-family: 'Noto Sans', 'Inter', 'Cantarell', sans-serif;
            font-size: 13px;
            font-weight: 600;
        }

        .okp-about-check-button:hover {
            background: @okp_light_rail;
        }

        button.okp-settings-switch {
            min-width: 39px;
            min-height: 22px;
            padding: 3px;
            border: none;
            border-radius: 999px;
            background: @okp_light_inset;
            box-shadow: none;
        }

        button.okp-settings-switch.is-active {
            background: linear-gradient(to right, @okp_navy, @okp_blue 55%, @okp_accent);
        }

        button.okp-settings-switch:focus-visible {
            outline: none;
            box-shadow: 0 0 0 3px alpha(@okp_accent, 0.30);
        }

        .okp-settings-switch-knob {
            min-width: 16px;
            min-height: 16px;
            border-radius: 999px;
            background: #ffffff;
            box-shadow: 0 1px 3px rgba(12, 20, 36, 0.25);
        }

        .okp-about-link-button {
            min-height: 24px;
            padding: 0;
            border: none;
            background: transparent;
            box-shadow: none;
            color: @okp_link;
            font-family: 'Noto Sans', 'Inter', 'Cantarell', sans-serif;
            font-size: 12px;
            font-weight: 600;
        }

        .okp-about-link-arrow {
            color: @okp_link;
            font-family: 'Noto Sans', 'Inter', 'Cantarell', sans-serif;
            font-size: 12px;
            font-weight: 600;
        }

        .okp-about-link-dot {
            min-width: 3px;
            min-height: 24px;
            color: @okp_ink_faint;
            font-family: 'Noto Sans', 'Inter', 'Cantarell', sans-serif;
            font-size: 8px;
            font-weight: 600;
        }

        .okp-update-status {
            color: @okp_ink_mute;
            font-family: 'Noto Sans', 'Inter', 'Cantarell', sans-serif;
            font-size: 12px;
        }

        .okp-mpv-conf-scroller {
            min-height: 132px;
            border-radius: 8px;
            background: @okp_light_card;
            border: 1px solid @okp_light_hairline_hi;
        }

        textview.okp-mpv-conf-editor,
        textview.okp-mpv-conf-editor text {
            padding: 10px;
            background: @okp_light_card;
            color: @okp_ink;
            font-family: 'JetBrains Mono', 'Cascadia Mono', monospace;
            font-size: 12px;
            font-weight: 500;
            caret-color: @okp_navy;
        }

        textview.okp-mpv-conf-editor selection,
        textview.okp-mpv-conf-editor text selection {
            background: alpha(@okp_navy, 0.18);
            color: @okp_ink;
        }

        /* Switch rows sit inside an .okp-info-section card, so they use the same
           recessed inset as the track rows instead of a second white card with a
           matching border. That keeps grouped settings from reading as
           card-inside-card clutter. */
        .okp-settings-switch-row {
            min-height: 42px;
            padding: 10px 12px;
            border-radius: 8px;
            background: @okp_light_rail;
            border: 1px solid transparent;
        }

        .okp-settings-state-pill {
            min-width: 34px;
            padding: 3px 10px;
            border-radius: 999px;
            background: alpha(@okp_navy, 0.09);
            border: 1px solid alpha(@okp_navy, 0.20);
            color: @okp_navy;
            font-family: 'Noto Sans', 'Inter', 'Cantarell', sans-serif;
            font-size: 11px;
            font-weight: 600;
        }

        .okp-integration-state-pill {
            min-width: 74px;
            padding: 3px 10px;
            border-radius: 999px;
            border: 1px solid transparent;
            font-family: 'Noto Sans', 'Inter', 'Cantarell', sans-serif;
            font-size: 11px;
            font-weight: 600;
        }

        .okp-integration-state-pill.is-good {
            background: alpha(@okp_ok, 0.10);
            border-color: alpha(@okp_ok, 0.42);
            color: @okp_ok;
        }

        .okp-integration-state-pill.is-warning {
            background: alpha(@okp_warning, 0.10);
            border-color: alpha(@okp_warning, 0.42);
            color: @okp_warning;
        }

        .okp-integration-state-pill.is-bad {
            background: alpha(@okp_danger, 0.08);
            border-color: alpha(@okp_danger, 0.42);
            color: @okp_danger;
        }

        .okp-info-section {
            padding: 16px 20px;
            border-radius: 12px;
            background: @okp_light_card;
            border: 1px solid @okp_light_hairline;
            box-shadow: 0 1px 2px rgba(12, 20, 36, 0.04), 0 12px 32px rgba(12, 20, 36, 0.06);
        }

        .okp-info-section-title {
            margin-bottom: 12px;
            color: @okp_ink_faint;
            font-family: 'Noto Sans', 'Inter', 'Cantarell', sans-serif;
            font-size: 11px;
            font-weight: 600;
            letter-spacing: 0.05em;
            text-transform: uppercase;
        }

        .okp-info-row {
            min-height: 22px;
        }

        .okp-info-label {
            color: @okp_ink_mute;
            font-family: 'Noto Sans', 'Inter', 'Cantarell', sans-serif;
            font-size: 13px;
            font-weight: 400;
        }

        .okp-info-value {
            color: @okp_ink;
            font-family: 'JetBrains Mono', 'Cascadia Mono', monospace;
            font-size: 12px;
            font-weight: 500;
            font-feature-settings: 'tnum';
        }

        .okp-info-row.is-highlight .okp-info-value {
            color: @okp_navy;
            font-weight: 600;
        }

        .okp-settings-row {
            min-height: 34px;
        }

        .okp-settings-action-row {
            margin-top: 8px;
        }

        .okp-settings-hint {
            margin-top: 6px;
            color: @okp_ink_faint;
            font-family: 'Noto Sans', 'Inter', 'Cantarell', sans-serif;
            font-size: 12px;
            font-weight: 400;
        }

        .okp-shortcuts-list {
            margin-top: 4px;
        }

        .okp-shortcut-row {
            min-height: 44px;
            padding: 7px 0;
            border-bottom: 1px solid @okp_light_hairline;
        }

        .okp-shortcut-row:last-child {
            border-bottom: none;
        }

        .okp-shortcut-row.is-conflict {
            color: @okp_danger;
        }

        .okp-shortcut-action-title {
            color: @okp_ink;
            font-family: 'Noto Sans', 'Inter', 'Cantarell', sans-serif;
            font-size: 13px;
            font-weight: 500;
        }

        .okp-shortcut-action-id {
            color: @okp_ink_faint;
            font-family: 'JetBrains Mono', 'Cascadia Mono', monospace;
            font-size: 11px;
            font-feature-settings: 'tnum';
        }

        .okp-shortcut-badge {
            padding: 2px 7px;
            border-radius: 999px;
            background: alpha(@okp_navy, 0.09);
            color: @okp_navy;
            font-family: 'Noto Sans', 'Inter', 'Cantarell', sans-serif;
            font-size: 10px;
            font-weight: 600;
        }

        button.okp-shortcut-chip {
            min-width: 82px;
            min-height: 30px;
            padding: 0 12px;
            border-radius: 999px;
            background: @okp_light_card;
            border: 1px solid @okp_light_hairline_hi;
            box-shadow: none;
            color: @okp_ink;
        }

        button.okp-shortcut-chip:hover {
            background: @okp_light_rail;
        }

        button.okp-shortcut-chip.is-secondary {
            min-width: 66px;
        }

        button.okp-shortcut-chip.is-empty {
            background: transparent;
            border-color: alpha(@okp_navy, 0.30);
            color: @okp_navy;
        }

        button.okp-shortcut-chip.is-empty:hover {
            background: alpha(@okp_navy, 0.06);
        }

        button.okp-shortcut-chip.is-capturing {
            background: alpha(@okp_accent, 0.14);
            border-color: @okp_accent;
        }

        button.okp-shortcut-chip.is-conflict {
            background: alpha(@okp_danger, 0.08);
            border-color: alpha(@okp_danger, 0.42);
        }

        .okp-shortcut-chip-label {
            font-family: 'JetBrains Mono', 'Cascadia Mono', monospace;
            font-size: 11.5px;
            font-weight: 600;
            font-feature-settings: 'tnum';
        }

        button.okp-shortcut-reset {
            min-width: 52px;
            min-height: 30px;
            padding: 0 12px;
            border-radius: 999px;
            background: transparent;
            border: 1px solid transparent;
            box-shadow: none;
            color: @okp_link;
            font-family: 'Noto Sans', 'Inter', 'Cantarell', sans-serif;
            font-size: 12px;
            font-weight: 600;
        }

        button.okp-shortcut-reset:hover {
            background: alpha(@okp_navy, 0.06);
        }

        button.okp-shortcut-reset:disabled {
            color: alpha(@okp_ink, 0.24);
        }

        .okp-settings-scale trough {
            min-height: 6px;
            border-radius: 999px;
            background: @okp_light_inset;
        }

        .okp-settings-scale highlight {
            min-height: 6px;
            border-radius: 999px;
            background: linear-gradient(to right, @okp_navy, @okp_blue 55%, @okp_accent);
        }

        .okp-settings-scale slider {
            min-width: 18px;
            min-height: 18px;
            border-radius: 999px;
            background: #ffffff;
            border: 1px solid @okp_light_hairline_hi;
            box-shadow: 0 1px 3px rgba(12, 20, 36, 0.18);
        }

        .okp-settings-button {
            min-width: 82px;
            min-height: 32px;
            padding: 0 16px;
            border-radius: 999px;
            background: @okp_light_card;
            border: 1px solid @okp_light_hairline_hi;
            box-shadow: none;
            color: @okp_ink;
            font-family: 'Noto Sans', 'Inter', 'Cantarell', sans-serif;
            font-size: 12.5px;
            font-weight: 600;
        }

        .okp-settings-button:hover {
            background: @okp_light_rail;
        }

        dropdown.okp-history-retention button {
            min-width: 132px;
            min-height: 32px;
            padding: 0 14px;
            border-radius: 999px;
            background: @okp_light_card;
            border: 1px solid @okp_light_hairline_hi;
            box-shadow: none;
            color: @okp_ink;
            font-family: 'Noto Sans', 'Inter', 'Cantarell', sans-serif;
            font-size: 12.5px;
            font-weight: 500;
        }

        dropdown.okp-history-retention button:hover {
            background: @okp_light_rail;
        }

        button.okp-screenshot-format-button {
            min-width: 62px;
        }

        button.okp-screenshot-format-button.is-selected {
            background: alpha(@okp_navy, 0.09);
            border-color: alpha(@okp_navy, 0.30);
            color: @okp_navy;
            font-weight: 600;
        }

        .okp-settings-button:disabled,
        .okp-settings-stepper-button:disabled {
            background: @okp_light_rail;
            border-color: @okp_light_hairline;
            color: alpha(@okp_ink, 0.32);
        }

        button.okp-settings-stepper-button {
            min-width: 58px;
            padding: 0 10px;
            font-feature-settings: 'tnum';
        }

        .okp-empty-state {
            min-height: 40px;
            margin: 2px 0;
            padding: 14px;
            border-radius: 12px;
            background: @okp_light_rail;
            border: 1px dashed @okp_light_hairline_hi;
        }

        .okp-empty-state-icon {
            color: @okp_ink_faint;
            -gtk-icon-size: 14px;
        }

        .okp-empty-state-text {
            color: @okp_ink_mute;
            font-family: 'Noto Sans', 'Inter', 'Cantarell', sans-serif;
            font-size: 13px;
            font-weight: 400;
        }

        button.okp-settings-track-row {
            min-height: 34px;
            padding: 7px 12px;
            border-radius: 8px;
            background: @okp_light_rail;
            border: 1px solid transparent;
            box-shadow: none;
            color: @okp_ink;
            font-family: 'Noto Sans', 'Inter', 'Cantarell', sans-serif;
            font-size: 13px;
            font-weight: 400;
        }

        button.okp-settings-track-row:hover {
            background: alpha(@okp_navy, 0.06);
        }

        button.okp-settings-track-row.is-selected {
            background: alpha(@okp_navy, 0.09);
            border-color: transparent;
            color: @okp_navy;
            font-weight: 600;
        }

        .okp-info-track-row {
            min-height: 44px;
            padding: 8px 10px;
            border-radius: 8px;
            background: @okp_light_rail;
            border: 1px solid transparent;
        }

        .okp-info-track-row.is-selected {
            background: alpha(@okp_navy, 0.09);
            border-color: transparent;
        }

        .okp-info-track-kind {
            color: @okp_navy;
            font-family: 'Noto Sans', 'Inter', 'Cantarell', sans-serif;
            font-size: 11px;
            font-weight: 600;
            letter-spacing: 0;
        }

        .okp-info-track-title {
            color: @okp_ink;
            font-family: 'Noto Sans', 'Inter', 'Cantarell', sans-serif;
            font-size: 12.5px;
            font-weight: 600;
        }

        .okp-info-track-current {
            padding: 2px 7px;
            border-radius: 999px;
            background: alpha(@okp_navy, 0.09);
            color: @okp_navy;
            font-family: 'Noto Sans', 'Inter', 'Cantarell', sans-serif;
            font-size: 9px;
            font-weight: 600;
            letter-spacing: 0;
        }

        .okp-info-track-detail {
            color: @okp_ink_mute;
            font-family: 'Noto Sans', 'Inter', 'Cantarell', sans-serif;
            font-size: 12px;
        }

        .okp-info-footer-button {
            min-width: 82px;
            min-height: 34px;
            padding: 0 16px;
            border-radius: 999px;
            background: @okp_light_card;
            border: 1px solid @okp_light_hairline_hi;
            box-shadow: none;
            color: @okp_ink;
            font-family: 'Noto Sans', 'Inter', 'Cantarell', sans-serif;
            font-size: 12.5px;
            font-weight: 600;
        }

        .okp-info-footer-button:hover {
            background: @okp_light_rail;
        }

        .okp-settings-segmented {
            padding: 3px;
            border-radius: 999px;
            background: @okp_light_rail;
            border: 1px solid @okp_light_hairline;
        }

        button.okp-settings-segment-button {
            min-width: 72px;
            min-height: 28px;
            padding: 0 14px;
            border: none;
            border-radius: 999px;
            background: transparent;
            box-shadow: none;
            color: @okp_ink_mute;
            font-family: 'Noto Sans', 'Inter', 'Cantarell', sans-serif;
            font-size: 12px;
            font-weight: 600;
        }

        button.okp-settings-segment-button:hover {
            background: transparent;
            color: @okp_ink;
        }

        button.okp-settings-segment-button.is-selected {
            background: @okp_light_card;
            color: @okp_ink;
            box-shadow: 0 1px 0 rgba(12, 20, 36, 0.04), 0 8px 24px rgba(12, 20, 36, 0.05);
        }

        button.okp-settings-segment-button.okp-subtitle-style-choice {
            min-width: 52px;
            padding: 0 10px;
            font-size: 11.5px;
        }

        window.okp-settings-window.is-dark,
        window.okp-settings-window.is-dark .okp-settings-root,
        window.okp-settings-window.is-dark .okp-settings-titlebar,
        window.okp-settings-window.is-dark .okp-settings-body,
        window.okp-settings-window.is-dark .okp-settings-stack,
        window.okp-settings-window.is-dark .okp-settings-scroller,
        window.okp-settings-window.is-dark .okp-settings-page,
        window.okp-settings-window.is-dark .okp-about-pane {
            background: @okp_settings_dark;
            color: @okp_snow;
        }

        window.okp-settings-window.is-dark .okp-settings-titlebar {
            border-bottom-color: @okp_dark_hairline;
        }

        window.okp-settings-window.is-dark .okp-settings-titlebar-label {
            color: @okp_snow_mute;
        }

        window.okp-settings-window.is-dark .okp-settings-rail-frame,
        window.okp-settings-window.is-dark .okp-settings-rail {
            background: @okp_dark_rail;
        }

        window.okp-settings-window.is-dark .okp-settings-rail {
            border-right-color: @okp_dark_hairline;
        }

        window.okp-settings-window.is-dark entry.okp-settings-search,
        window.okp-settings-window.is-dark entry.okp-shortcuts-search {
            background: @okp_dark_card;
            border-color: @okp_dark_hairline_hi;
            color: @okp_snow;
        }

        window.okp-settings-window.is-dark button.okp-settings-search-result {
            background: alpha(@okp_accent, 0.16);
            box-shadow: none;
            color: @okp_snow;
        }

        window.okp-settings-window.is-dark button.okp-settings-search-result:hover {
            background: alpha(@okp_accent, 0.22);
        }

        window.okp-settings-window.is-dark .okp-settings-search-result-page {
            color: @okp_snow_faint;
        }

        window.okp-settings-window.is-dark .okp-settings-nav-row {
            color: @okp_snow_mute;
        }

        window.okp-settings-window.is-dark .okp-settings-nav-row:hover {
            background: alpha(@okp_accent, 0.08);
            color: @okp_snow;
        }

        window.okp-settings-window.is-dark .okp-settings-nav-row.is-selected {
            background: @okp_dark_card;
            box-shadow: 0 0 0 1px @okp_dark_hairline;
            color: @okp_snow;
        }

        window.okp-settings-window.is-dark .okp-settings-footer-rule {
            background: @okp_dark_hairline;
        }

        window.okp-settings-window.is-dark .okp-settings-window-control,
        window.okp-settings-window.is-dark .okp-settings-window-control-glyph {
            color: @okp_snow_mute;
        }

        window.okp-settings-window.is-dark .okp-settings-window-control:hover {
            background: alpha(@okp_accent, 0.08);
        }

        window.okp-settings-window.is-dark button.okp-settings-window-close:hover {
            background: @okp_danger_dark;
        }

        window.okp-settings-window.is-dark button.okp-settings-window-control:hover .okp-settings-window-control-glyph {
            color: @okp_snow;
        }

        window.okp-settings-window.is-dark .okp-about-illustration-art {
            color: @okp_accent;
        }

        window.okp-settings-window.is-dark .okp-about-wordmark,
        window.okp-settings-window.is-dark .okp-about-row-value,
        window.okp-settings-window.is-dark .okp-about-row-value-mono,
        window.okp-settings-window.is-dark .okp-info-value,
        window.okp-settings-window.is-dark .okp-update-action-title,
        window.okp-settings-window.is-dark .okp-shortcut-action-title {
            color: @okp_snow;
        }

        window.okp-settings-window.is-dark .okp-about-tagline,
        window.okp-settings-window.is-dark .okp-about-row-label,
        window.okp-settings-window.is-dark .okp-info-label,
        window.okp-settings-window.is-dark .okp-update-status,
        window.okp-settings-window.is-dark .okp-update-setup-commands,
        window.okp-settings-window.is-dark .okp-update-action-detail,
        window.okp-settings-window.is-dark .okp-info-track-detail {
            color: @okp_snow_mute;
        }

        window.okp-settings-window.is-dark .okp-settings-hint {
            color: @okp_snow_faint;
        }

        window.okp-settings-window.is-dark .okp-about-byline,
        window.okp-settings-window.is-dark .okp-about-card-title,
        window.okp-settings-window.is-dark .okp-about-row-detail,
        window.okp-settings-window.is-dark .okp-shortcut-action-id,
        window.okp-settings-window.is-dark .okp-info-section-title {
            color: @okp_snow_faint;
        }

        window.okp-settings-window.is-dark .okp-about-identity-divider {
            background: transparent;
        }

        window.okp-settings-window.is-dark .okp-about-identity {
            background-color: @okp_dark_card;
            background-image: linear-gradient(135deg, alpha(@okp_blue, 0.10), alpha(@okp_accent, 0.06));
            border-color: @okp_dark_hairline;
            box-shadow: none;
        }

        window.okp-settings-window.is-dark .okp-update-setup-commands {
            background: @okp_settings_dark;
            border-color: @okp_dark_hairline;
        }

        window.okp-settings-window.is-dark .okp-shortcut-row {
            border-color: @okp_dark_hairline;
        }

        window.okp-settings-window.is-dark .okp-about-card,
        window.okp-settings-window.is-dark .okp-info-section,
        window.okp-settings-window.is-dark .okp-update-action-surface {
            background: @okp_dark_card;
            border-color: @okp_dark_hairline;
            box-shadow: none;
        }

        window.okp-settings-window.is-dark .okp-about-version-chip,
        window.okp-settings-window.is-dark .okp-about-tag,
        window.okp-settings-window.is-dark .okp-about-copy-button {
            background: @okp_dark_rail;
            border-color: @okp_dark_hairline;
            color: @okp_snow;
        }

        window.okp-settings-window.is-dark .okp-about-channel-chip,
                window.okp-settings-window.is-dark .okp-settings-state-pill,
        window.okp-settings-window.is-dark .okp-shortcut-badge,
        window.okp-settings-window.is-dark .okp-info-track-current {
            background: alpha(@okp_accent, 0.16);
            border-color: alpha(@okp_accent, 0.35);
            color: @okp_accent;
        }

        window.okp-settings-window.is-dark .okp-about-link-button,
        window.okp-settings-window.is-dark .okp-about-link-arrow,
        window.okp-settings-window.is-dark button.okp-shortcut-reset,
        window.okp-settings-window.is-dark .okp-info-row.is-highlight .okp-info-value {
            color: @okp_accent_bright;
        }

        window.okp-settings-window.is-dark .okp-about-link-dot {
            color: @okp_snow_faint;
        }

        window.okp-settings-window.is-dark .okp-settings-switch-row,
        window.okp-settings-window.is-dark button.okp-settings-track-row,
        window.okp-settings-window.is-dark .okp-info-track-row,
        window.okp-settings-window.is-dark button.okp-shortcut-chip,
        window.okp-settings-window.is-dark .okp-empty-state,
        window.okp-settings-window.is-dark .okp-mpv-conf-scroller,
        window.okp-settings-window.is-dark textview.okp-mpv-conf-editor,
        window.okp-settings-window.is-dark textview.okp-mpv-conf-editor text,
        window.okp-settings-window.is-dark .okp-settings-button,
        window.okp-settings-window.is-dark dropdown.okp-history-retention button {
            background: @okp_dark_rail;
            border-color: @okp_dark_hairline;
            color: @okp_snow;
        }

        window.okp-settings-window.is-dark .okp-settings-button:hover,
        window.okp-settings-window.is-dark dropdown.okp-history-retention button:hover,
        window.okp-settings-window.is-dark button.okp-settings-track-row:hover,
        window.okp-settings-window.is-dark button.okp-shortcut-chip:hover,
        window.okp-settings-window.is-dark .okp-about-copy-button:hover {
            background: @okp_dark_inset;
        }

        window.okp-settings-window.is-dark scale.okp-settings-scale highlight,
        window.okp-settings-window.is-dark button.okp-settings-switch.is-active {
            background: linear-gradient(to right, @okp_blue, @okp_accent 60%, @okp_accent_bright);
        }

        window.okp-settings-window.is-dark button.okp-settings-switch {
            background: @okp_dark_inset;
        }

        window.okp-settings-window.is-dark .okp-settings-scale trough {
            background: @okp_dark_inset;
        }

        window.okp-settings-window.is-dark textview.okp-mpv-conf-editor {
            caret-color: @okp_accent;
        }

        window.okp-settings-window.is-dark textview.okp-mpv-conf-editor selection,
        window.okp-settings-window.is-dark textview.okp-mpv-conf-editor text selection {
            background: alpha(@okp_accent, 0.28);
            color: @okp_snow;
        }

        window.okp-settings-window.is-dark button.okp-update-primary-button {
            background: linear-gradient(135deg, @okp_blue, @okp_accent 60%, @okp_accent_bright);
            color: @okp_accent_ink;
            box-shadow: none;
        }

        window.okp-settings-window.is-dark button.okp-update-primary-button:hover {
            background: linear-gradient(135deg, @okp_blue, @okp_accent_bright 60%, @okp_accent_bright);
        }

        /* The grouped dark rule above outranks the light state rules at
           (0,2,1), so every selected / conflict / semantic state the dark
           window shows needs its own dark rule. */
        window.okp-settings-window.is-dark button.okp-settings-track-row.is-selected,
        window.okp-settings-window.is-dark button.okp-screenshot-format-button.is-selected {
            background: alpha(@okp_accent, 0.16);
            border-color: alpha(@okp_accent, 0.40);
            color: @okp_accent_bright;
        }

        window.okp-settings-window.is-dark .okp-integration-state-pill.is-good,
        window.okp-settings-window.is-dark .okp-about-tag.is-ok {
            background: alpha(@okp_ok_dark, 0.12);
            border-color: alpha(@okp_ok_dark, 0.42);
            color: @okp_ok_dark;
        }

        window.okp-settings-window.is-dark .okp-integration-state-pill.is-warning {
            background: alpha(@okp_warning_dark, 0.12);
            border-color: alpha(@okp_warning_dark, 0.42);
            color: @okp_warning_dark;
        }

        window.okp-settings-window.is-dark .okp-integration-state-pill.is-bad,
        window.okp-settings-window.is-dark button.okp-shortcut-chip.is-conflict {
            background: alpha(@okp_danger_bright, 0.10);
            border-color: alpha(@okp_danger_bright, 0.42);
            color: @okp_danger_bright;
        }

        window.okp-settings-window.is-dark .okp-shortcut-row.is-conflict {
            color: @okp_danger_bright;
        }

        window.okp-settings-window.is-dark .okp-settings-audio-delay-row entry.okp-sub-adjust-entry.is-error {
            border-color: alpha(@okp_danger_bright, 0.88);
            box-shadow: 0 0 0 2px alpha(@okp_danger_bright, 0.18);
        }

        window.okp-settings-window.is-dark button.okp-settings-switch:focus-visible {
            box-shadow: 0 0 0 3px alpha(@okp_accent, 0.30);
        }

        window.okp-settings-window.is-dark .okp-settings-segmented {
            background: @okp_dark_rail;
            border-color: @okp_dark_hairline;
        }

        window.okp-settings-window.is-dark button.okp-settings-segment-button {
            color: @okp_snow_mute;
        }

        window.okp-settings-window.is-dark button.okp-settings-segment-button:hover {
            background: transparent;
            color: @okp_snow;
        }

        window.okp-settings-window.is-dark button.okp-settings-segment-button.is-selected {
            background: @okp_dark_card;
            color: @okp_snow;
            box-shadow: 0 0 0 1px @okp_dark_hairline;
        }

        window.okp-settings-window.is-dark scrollbar,
        window.okp-settings-window.is-dark scrollbar trough {
            background: transparent;
            border: none;
        }

        window.okp-settings-window.is-dark scrollbar slider {
            background: alpha(@okp_snow, 0.24);
        }

        window.okp-settings-window.is-high-contrast,
        window.okp-settings-window.is-high-contrast .okp-settings-root,
        window.okp-settings-window.is-high-contrast .okp-settings-titlebar,
        window.okp-settings-window.is-high-contrast .okp-settings-body,
        window.okp-settings-window.is-high-contrast .okp-settings-rail,
        window.okp-settings-window.is-high-contrast .okp-settings-stack,
        window.okp-settings-window.is-high-contrast .okp-settings-scroller,
        window.okp-settings-window.is-high-contrast .okp-settings-page,
        window.okp-settings-window.is-high-contrast .okp-about-pane {
            background: #000000;
            color: #ffffff;
        }

        window.okp-settings-window.is-high-contrast .okp-about-card,
        window.okp-settings-window.is-high-contrast .okp-info-section,
        window.okp-settings-window.is-high-contrast .okp-settings-switch-row,
        window.okp-settings-window.is-high-contrast .okp-update-action-surface,
        window.okp-settings-window.is-high-contrast .okp-settings-button,
        window.okp-settings-window.is-high-contrast dropdown.okp-history-retention button {
            background: #000000;
            border-color: #ffffff;
            color: #ffffff;
        }

        /* High contrast keeps the light base beneath it, so the hero card and
           the command block - white surfaces in the light scheme - have to be
           reset or their white text vanishes. */
        window.okp-settings-window.is-high-contrast .okp-about-identity {
            background-color: #000000;
            background-image: none;
            border-color: #ffffff;
            box-shadow: none;
        }

        window.okp-settings-window.is-high-contrast .okp-update-setup-commands {
            background: #000000;
            border-color: #ffffff;
            color: #ffffff;
        }

        /* The remaining light-scheme surfaces of the Settings window: insets,
           chips, secondary buttons and the segmented track. The selected inset
           row and segment invert, as the rail's selected row already does. */
        window.okp-settings-window.is-high-contrast .okp-settings-rail-frame,
        window.okp-settings-window.is-high-contrast .okp-settings-segmented,
        window.okp-settings-window.is-high-contrast .okp-about-version-chip,
        window.okp-settings-window.is-high-contrast .okp-about-copy-button,
        window.okp-settings-window.is-high-contrast .okp-about-check-button,
        window.okp-settings-window.is-high-contrast .okp-info-footer-button,
        window.okp-settings-window.is-high-contrast .okp-mpv-conf-scroller,
        window.okp-settings-window.is-high-contrast .okp-empty-state,
        window.okp-settings-window.is-high-contrast button.okp-settings-track-row,
        window.okp-settings-window.is-high-contrast .okp-info-track-row,
        window.okp-settings-window.is-high-contrast .okp-settings-button:disabled,
        window.okp-settings-window.is-high-contrast .okp-settings-stepper-button:disabled {
            background: #000000;
            border-color: #ffffff;
            box-shadow: none;
            color: #ffffff;
        }

        window.okp-settings-window.is-high-contrast .okp-info-track-row label,
        window.okp-settings-window.is-high-contrast .okp-empty-state label,
        window.okp-settings-window.is-high-contrast .okp-empty-state-icon,
        window.okp-settings-window.is-high-contrast button.okp-settings-segment-button {
            color: #ffffff;
        }

        window.okp-settings-window.is-high-contrast button.okp-settings-track-row.is-selected,
        window.okp-settings-window.is-high-contrast .okp-info-track-row.is-selected,
        window.okp-settings-window.is-high-contrast .okp-info-track-row.is-selected label {
            background: #ffffff;
            color: #000000;
        }

        window.okp-settings-window.is-high-contrast .okp-settings-titlebar-label,
        window.okp-settings-window.is-high-contrast .okp-settings-nav-row,
        window.okp-settings-window.is-high-contrast .okp-settings-window-control,
        window.okp-settings-window.is-high-contrast .okp-settings-window-control-glyph,
        window.okp-settings-window.is-high-contrast .okp-about-wordmark,
        window.okp-settings-window.is-high-contrast .okp-about-tagline,
        window.okp-settings-window.is-high-contrast .okp-about-byline,
        window.okp-settings-window.is-high-contrast .okp-about-card-title,
        window.okp-settings-window.is-high-contrast .okp-about-row-label,
        window.okp-settings-window.is-high-contrast .okp-about-row-detail,
        window.okp-settings-window.is-high-contrast .okp-about-row-value,
        window.okp-settings-window.is-high-contrast .okp-about-row-value-mono,
        window.okp-settings-window.is-high-contrast .okp-about-link-button,
        window.okp-settings-window.is-high-contrast .okp-about-link-arrow,
        window.okp-settings-window.is-high-contrast .okp-info-label,
        window.okp-settings-window.is-high-contrast .okp-info-value,
        window.okp-settings-window.is-high-contrast .okp-update-status,
        window.okp-settings-window.is-high-contrast .okp-update-action-title,
        window.okp-settings-window.is-high-contrast .okp-update-setup-commands,
        window.okp-settings-window.is-high-contrast .okp-update-action-detail,
        window.okp-settings-window.is-high-contrast .okp-shortcut-action-title,
        window.okp-settings-window.is-high-contrast .okp-shortcut-action-id {
            color: #ffffff;
        }

        window.okp-settings-window.is-high-contrast button.okp-update-primary-button {
            background: #ffffff;
            color: #000000;
            border: 1px solid #ffffff;
        }

        window.okp-settings-window.is-high-contrast .okp-settings-hint {
            color: #ffffff;
        }

        window.okp-settings-window.is-high-contrast entry.okp-settings-search,
        window.okp-settings-window.is-high-contrast entry.okp-shortcuts-search,
        window.okp-settings-window.is-high-contrast button.okp-shortcut-chip,
        window.okp-settings-window.is-high-contrast textview.okp-mpv-conf-editor,
        window.okp-settings-window.is-high-contrast textview.okp-mpv-conf-editor text {
            background: #000000;
            border-color: #ffffff;
            color: #ffffff;
        }

        window.okp-settings-window.is-high-contrast button.okp-settings-search-result {
            background: #000000;
            border: 1px solid #ffffff;
            box-shadow: none;
            color: #ffffff;
        }

        window.okp-settings-window.is-high-contrast .okp-settings-search-result-page {
            color: #ffffff;
        }

        window.okp-settings-window.is-high-contrast .okp-settings-nav-row.is-selected,
        window.okp-settings-window.is-high-contrast button.okp-settings-segment-button.is-selected {
            background: #ffffff;
            color: #000000;
            box-shadow: none;
        }

        /* #731. A widget node that takes its icon from CSS is the one kind of
           chrome icon neither a call site nor a resource path can reach: GTK's
           own stylesheet names the standard icon, and an application provider
           outranks the theme, so naming the shipped icon here is what makes it
           ours. These are exactly the two selectors GTK's own stylesheet points
           at the icon theme for the widgets this shell uses - `dropdown arrow`
           and `spinner`. Arrow nodes GTK draws nothing for are left alone: an
           icon-source there would add an arrow rather than replace one. */
        dropdown arrow,
        combobox arrow {
            -gtk-icon-source: -gtk-icontheme('okp-pan-down-symbolic');
        }

        spinner {
            -gtk-icon-source: -gtk-icontheme('okp-process-working-symbolic');
        }
        ";

pub(crate) fn install_css() {
    let Some(display) = gdk::Display::default() else {
        return;
    };

    let provider = gtk::CssProvider::new();
    provider.load_from_data(OKP_STYLESHEET);
    gtk::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

#[cfg(test)]
mod tests {
    use super::{OKP_STYLESHEET, SHELL_CORNER_RADIUS_PX};
    use std::collections::HashSet;

    /// The mask the native video plane cuts is driven by
    /// `SHELL_CORNER_RADIUS_PX`, while the stylesheet is a static string that
    /// has to state the same number. Pin them together: a radius that
    /// disagrees with the chrome is the very defect #778 reported.
    #[test]
    fn shell_radius_matches_the_stylesheet() {
        let radius = format!("border-radius: {SHELL_CORNER_RADIUS_PX}px;");
        for selector in [
            "window.okp-player-window:not(.maximized):not(.fullscreen),",
            "window.okp-player-window.is-compact-mode,",
        ] {
            let rule_start = OKP_STYLESHEET
                .find(selector)
                .unwrap_or_else(|| panic!("rounded shell selector missing: {selector}"));
            let rule = &OKP_STYLESHEET[rule_start..];
            let rule_end = rule.find('}').expect("rounded shell rule terminator");
            assert!(
                rule[..rule_end].contains(&radius),
                "{selector} must round the shell at {SHELL_CORNER_RADIUS_PX}px"
            );
        }
    }

    #[test]
    fn native_video_compact_mode_keeps_the_parent_surface_transparent() {
        let rule_start = OKP_STYLESHEET
            .find("window.okp-player-window.okp-native-video.is-compact-mode,")
            .expect("native Mini-player transparency selector");
        let rule = &OKP_STYLESHEET[rule_start..];
        let rule_end = rule.find('}').expect("native Mini-player rule terminator");
        let rule = &rule[..rule_end];
        assert!(rule.contains(
            "window.okp-player-window.okp-native-video.is-compact-mode .okp-root.okp-native-video"
        ));
        assert!(rule.contains("background: transparent;"));
    }

    /// Token names declared via `@define-color okp_<name> <value>;`.
    fn defined_tokens() -> HashSet<String> {
        OKP_STYLESHEET
            .lines()
            .filter_map(|line| line.trim().strip_prefix("@define-color "))
            .filter_map(|rest| rest.split_whitespace().next())
            .map(str::to_owned)
            .collect()
    }

    #[test]
    fn every_token_reference_resolves_to_a_definition() {
        let defined = defined_tokens();
        assert!(!defined.is_empty(), "stylesheet declares no @okp tokens");

        // A stray `@okp_...` typo makes GTK drop that declaration silently, so a
        // whole surface would lose its colour with no build error. Guard it.
        let mut idx = 0;
        while let Some(pos) = OKP_STYLESHEET[idx..].find("@okp_") {
            let start = idx + pos + 1; // skip the leading '@'
            let name: String = OKP_STYLESHEET[start..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            assert!(
                defined.contains(&name),
                "stylesheet references undefined token @{name}"
            );
            idx = start + name.len();
        }
    }

    #[test]
    fn palette_is_unified_on_the_brand_accent() {
        // Neither the generic adwaita blue nor the retired teal may reappear on
        // any surface: light chrome leads with the BeFeast navy, everything over
        // the video and the dark scheme with the brand cyan.
        for stray in [
            "#0067c0",
            "0, 103, 192",
            "#28b3aa",
            "#37cfc5",
            "#229a92",
            "#10938a",
            "#0a655f",
            "#15a89d",
        ] {
            assert!(
                !OKP_STYLESHEET.contains(stray),
                "stray literal `{stray}` found; use @okp_navy or @okp_accent"
            );
        }

        // Brand colours live only in their `@define-color`; every other use goes
        // through a token so the palette retints from one edit.
        for once in [
            "#39bef9", "#163065", "#7dd8ff", "#1aa9ec", "#2563eb", "#0284c7",
        ] {
            let count = OKP_STYLESHEET.matches(once).count();
            assert_eq!(
                count, 1,
                "brand colour `{once}` should only appear in its @define-color, found {count}"
            );
        }
        for gone in [
            "40, 179, 170",
            "16, 147, 138",
            "57, 190, 249",
            "22, 48, 101",
        ] {
            assert!(
                !OKP_STYLESHEET.contains(gone),
                "raw accent literal `{gone}` should be replaced by a token"
            );
        }
    }

    #[test]
    fn shell_surface_roles_remain_distinct() {
        let value = |name: &str| {
            OKP_STYLESHEET
                .lines()
                .map(str::trim)
                .find_map(|line| line.strip_prefix(&format!("@define-color {name} ")))
                .and_then(|value| value.strip_suffix(';'))
                .expect("surface token should be defined")
        };
        let dark = value("okp_bg");
        let light = value("okp_light_bg");
        let rail = value("okp_light_rail");
        assert_ne!(dark, light);
        assert_ne!(dark, rail);
        assert!(dark.starts_with('#') && light.starts_with('#') && rail.starts_with('#'));
    }

    #[test]
    fn canonical_idle_and_history_geometry_stays_pinned() {
        for geometry in [
            "min-height: 34px;",
            "min-width: 280px;",
            "min-width: 194px;",
            "min-width: 36px;",
            "min-height: 82px;",
            "min-height: 84px;",
            "font-size: 22px;",
            "font-size: 13px;",
            "min-width: 64px;",
            "min-height: 36px;",
            "min-height: 4px;",
            "border-radius: 8px;",
        ] {
            assert!(
                OKP_STYLESHEET.contains(geometry),
                "canonical idle/history geometry missing `{geometry}`"
            );
        }
        assert!(OKP_STYLESHEET.contains(".okp-idle-canvas.is-light"));
        assert!(OKP_STYLESHEET.contains(".okp-idle-canvas.is-dark"));
        assert!(OKP_STYLESHEET.contains(".okp-idle-canvas.is-preview-substrate"));
        assert!(OKP_STYLESHEET.contains(".okp-idle-footer"));
        assert!(OKP_STYLESHEET.contains(".okp-history-bucket"));
        assert!(OKP_STYLESHEET.contains("padding: 34px 32px 0;"));
        assert!(OKP_STYLESHEET.contains("margin-top: 20px;"));
        assert!(OKP_STYLESHEET.contains("margin-top: 24px;"));
        assert!(OKP_STYLESHEET.contains("button.okp-recents-history-button"));
        assert!(OKP_STYLESHEET.contains("border-radius: 18px;"));
        assert!(OKP_STYLESHEET.contains(".okp-welcome-action-column"));
        assert!(OKP_STYLESHEET.contains("min-width: 132px;"));
        assert!(OKP_STYLESHEET.contains("padding: 0 16px;"));
        // BeFeast shapes, pinned on the rules that carry them: 12px cards, pill
        // controls, and the History bucket card in every theme.
        assert!(rule_body("button.okp-recent-card").contains("border-radius: 12px;"));
        assert!(
            rule_body("button.okp-idle-primary-button,\n        button.okp-idle-secondary-button")
                .contains("border-radius: 999px;")
        );
        assert!(
            rule_body(".is-light .okp-history-bucket-card")
                .contains("background: @okp_light_card;")
        );
        assert!(
            rule_body(".is-dark .okp-history-bucket-card").contains("background: @okp_dark_card;")
        );
        // The side panel and the track popovers follow the scheme too.
        assert!(rule_body(".okp-up-next-panel.is-dark").contains("background: @okp_dark_card;"));
        assert!(
            rule_body(
                "popover.okp-track-popover.is-dark > contents,\n        popover.okp-track-popover.is-dark contents"
            )
            .contains("background: @okp_dark_card;")
        );
        assert!(rule_body(".is-dark button.okp-track-row.is-selected").contains("@okp_accent"));
        assert!(
            rule_body(".is-high-contrast .okp-history-bucket-card")
                .contains("border: 1px solid #ffffff;")
        );
        // The light base stays beneath high contrast on both the idle canvas and
        // the Settings window, so every surface this restyle turned white must
        // be reset there too.
        for selector in [
            ".is-high-contrast .okp-history-state-card,\n        .is-high-contrast .okp-history-state-icon-wrap",
            ".is-high-contrast button.okp-recent-card,\n        .is-high-contrast button.okp-recents-history-button,\n        .is-high-contrast button.okp-idle-secondary-button,\n        .is-high-contrast button.okp-first-run-drop-target,\n        .is-high-contrast button.okp-welcome-drop-target,\n        .is-high-contrast .okp-history-private-banner",
            "window.okp-settings-window.is-high-contrast .okp-about-identity",
            "window.okp-settings-window.is-high-contrast .okp-update-setup-commands",
            "window.okp-media-info-window.is-high-contrast .okp-media-info-tab-strip",
        ] {
            let body = rule_body(selector);
            assert!(
                body.contains("#000000") && body.contains("#ffffff"),
                "{selector} must paint black on a white border, got {body}"
            );
        }
        // The idle root keeps .is-light / .is-dark beside .is-high-contrast,
        // and the themed state-card rules have the same specificity as the
        // reset, so the reset only wins by coming later in the sheet.
        let reset = OKP_STYLESHEET
            .find(".is-high-contrast .okp-history-state-card,")
            .expect("state-card high-contrast reset");
        for themed in [
            ".is-light .okp-history-state-card {",
            ".is-dark .okp-history-state-card {",
            ".is-light .okp-history-state-body {",
            ".is-dark button.okp-history-state-button {",
        ] {
            let at = OKP_STYLESHEET.find(themed).expect(themed);
            assert!(
                at < reset,
                "{themed} comes after the high-contrast reset and would win"
            );
        }
    }

    /// The declarations of the first rule that opens with exactly `selector`.
    fn rule_body(selector: &str) -> &'static str {
        let key = format!("\n        {selector} {{\n");
        let start = OKP_STYLESHEET
            .find(&key)
            .unwrap_or_else(|| panic!("rule `{selector}` missing from the stylesheet"))
            + key.len();
        let end = OKP_STYLESHEET[start..].find('}').expect("rule terminator") + start;
        &OKP_STYLESHEET[start..end]
    }

    #[test]
    fn stylesheet_braces_are_balanced() {
        assert_eq!(
            OKP_STYLESHEET.matches('{').count(),
            OKP_STYLESHEET.matches('}').count(),
            "unbalanced CSS braces in the stylesheet"
        );
    }

    #[test]
    fn playback_chrome_keeps_the_canonical_redlines() {
        for required in [
            "min-height: 42px;",
            "min-width: 46px;",
            "border-radius: 14px;",
            "background: rgba(12, 20, 36, 0.56);",
            "background: rgba(12, 20, 36, 0.62);",
            "transform: translate(0, -10px);",
            "transform: translate(0, 16px);",
            "transition: opacity 180ms ease, transform 200ms ease;",
            "min-width: 54px;",
            "min-width: 62px;",
            "min-height: 20px;",
            ".okp-timeline-rail",
            ".okp-paused-cue",
            ".okp-error-card",
        ] {
            assert!(
                OKP_STYLESHEET.contains(required),
                "missing canonical playback chrome redline `{required}`"
            );
        }
        assert!(
            !OKP_STYLESHEET.contains("okp-control-separator"),
            "the unified OSC must not regress to separated toolbar islands"
        );
        // The 7px/14px OSC pill inset moved out of CSS and into the adaptive
        // OscBar's own allocation (issue #328); the canonical redline lives
        // there now so the geometry is unchanged.
        let osc_bar = include_str!("osc_bar.rs");
        assert!(osc_bar.contains("pub(crate) const PAD_HORIZONTAL: i32 = 14;"));
        assert!(osc_bar.contains("pub(crate) const PAD_VERTICAL: i32 = 7;"));
    }
}
