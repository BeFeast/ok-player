use super::*;

const OKP_STYLESHEET: &str = "
        /* Design tokens: one coherent OK Player palette. Every accent and state
           colour below derives from these bases via alpha()/mix(), so the whole
           shell retints from a single edit. Dark chrome and the light settings
           surface share one teal brand accent; there is no stray adwaita blue. */
        @define-color okp_bg #050507;
        @define-color okp_accent #28b3aa;
        @define-color okp_accent_bright #37cfc5;
        @define-color okp_accent_deep #229a92;
        @define-color okp_light_bg #eef4f9;
        @define-color okp_light_rail #eaf0f5;
        @define-color okp_settings_light #f7f7f5;
        @define-color okp_settings_dark #1f1f1f;
        @define-color okp_ink #161616;
        @define-color okp_teal #10938a;
        @define-color okp_teal_deep #0a655f;
        @define-color okp_danger #c42b1c;
        @define-color okp_danger_deep #9a1f15;
        @define-color okp_danger_dark #db3b3b;
        @define-color okp_danger_bright #ff6868;
        @define-color okp_warning #b07600;
        @define-color okp_warning_deep #6f4b00;

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
        }

        window.okp-player-window.is-compact-mode,
        window.okp-player-window.is-compact-mode .okp-root {
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
            background: linear-gradient(to bottom, rgba(0, 0, 0, 0.50), rgba(0, 0, 0, 0));
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
            background: alpha(@okp_accent, 0.24);
            color: @okp_accent_bright;
        }

        .okp-player-window-controls button:focus-visible,
        button.okp-player-window-control:focus-visible {
            outline: none;
            box-shadow: inset 0 0 0 1px alpha(@okp_accent, 0.65);
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
            background: #050507;
        }

        .okp-empty-surface.is-preview-substrate.is-preview-bright {
            background: #f4f7fa;
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
            padding: 7px 12px;
            border-radius: 8px;
            background: rgba(22, 22, 25, 0.50);
            border: 1px solid rgba(255, 255, 255, 0.10);
            color: rgba(255, 255, 255, 0.72);
            font-size: 10.5px;
            font-weight: 700;
        }

        .okp-loading-state {
            color: rgba(255, 255, 255, 0.88);
        }

        .okp-loading-ring {
            min-width: 34px;
            min-height: 34px;
            color: @okp_accent_bright;
        }

        .okp-loading-label {
            color: rgba(255, 255, 255, 0.72);
            font-size: 12px;
            font-weight: 600;
        }

        .okp-error-card {
            min-width: 340px;
            padding: 22px 24px;
            border-radius: 12px;
            background: rgba(22, 22, 25, 0.88);
            border: 1px solid rgba(255, 255, 255, 0.12);
            box-shadow: 0 18px 46px rgba(0, 0, 0, 0.42);
        }

        .okp-error-icon {
            color: @okp_danger_bright;
            -gtk-icon-size: 24px;
        }

        .okp-error-title {
            color: rgba(255, 255, 255, 0.96);
            font-size: 16px;
            font-weight: 700;
        }

        .okp-error-body {
            color: rgba(255, 255, 255, 0.66);
            font-size: 12.5px;
        }

        button.okp-error-primary,
        button.okp-error-secondary {
            min-height: 32px;
            padding: 6px 12px;
            border-radius: 7px;
            box-shadow: none;
            font-size: 12px;
            font-weight: 650;
        }

        button.okp-error-primary {
            background: @okp_accent;
            border: 1px solid @okp_accent;
            color: #041110;
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
            background: rgba(13, 14, 18, 0.72);
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
            padding: 8px 18px;
            border-radius: 8px;
            border: 1px solid transparent;
            box-shadow: none;
            font-size: 13.5px;
            font-weight: 650;
        }

        .okp-empty-primary-button {
            background: @okp_accent_bright;
            color: #041110;
        }

        .okp-empty-primary-button:hover {
            background: @okp_accent_bright;
        }

        .okp-empty-primary-button:active {
            background: @okp_accent_deep;
        }

        .okp-empty-primary-button:focus-visible,
        .okp-empty-secondary-button:focus-visible {
            outline: none;
            box-shadow: 0 0 0 2px alpha(@okp_accent, 0.55);
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

        .okp-welcome-recents {
            min-width: 300px;
        }

        .okp-welcome-recents-heading {
            min-height: 48px;
        }

        .okp-recents-mark {
            margin-top: 2px;
        }

        .okp-welcome-recents-title {
            color: rgba(255, 255, 255, 0.97);
            font-size: 30px;
            font-weight: 760;
        }

        .okp-welcome-recents-subtitle {
            margin-top: 6px;
            color: rgba(255, 255, 255, 0.70);
            font-size: 13.5px;
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
            border-radius: 8px;
            background: transparent;
            box-shadow: none;
            color: rgba(255, 255, 255, 0.94);
        }

        button.okp-recent-card:hover {
            background: rgba(255, 255, 255, 0.055);
        }

        button.okp-recent-card:focus-visible {
            outline: none;
            box-shadow: 0 0 0 2px alpha(@okp_accent, 0.58);
        }

        .okp-history-thumbnail {
            border-radius: 8px;
            background: rgba(255, 255, 255, 0.05);
        }

        .okp-history-thumbnail-placeholder {
            border-radius: 8px;
            background: linear-gradient(135deg, rgba(51, 57, 65, 0.96), rgba(25, 29, 35, 0.98));
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
            border-radius: 0 0 0 8px;
            background: @okp_accent_bright;
        }

        .okp-recent-time-left {
            padding: 3px 7px;
            border-radius: 5px;
            background: rgba(0, 0, 0, 0.72);
            color: #ffffff;
            font-size: 10px;
            font-weight: 700;
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
            padding: 4px 7px;
            border: none;
            border-radius: 7px;
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
            color: alpha(@okp_accent_bright, 0.88);
        }

        .okp-welcome-private-title {
            color: rgba(255, 255, 255, 0.96);
            font-size: 22px;
            font-weight: 740;
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
            font-size: 30px;
            font-weight: 760;
        }

        entry.okp-history-search {
            min-width: 220px;
            min-height: 38px;
            border-radius: 8px;
            border: 1px solid rgba(255, 255, 255, 0.10);
            background: rgba(255, 255, 255, 0.06);
            color: rgba(255, 255, 255, 0.92);
            box-shadow: none;
        }

        entry.okp-history-search:focus-within {
            border-color: alpha(@okp_accent, 0.75);
            box-shadow: 0 0 0 1px alpha(@okp_accent, 0.32);
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
            border-radius: 7px;
            background: transparent;
            box-shadow: none;
            color: rgba(255, 255, 255, 0.92);
        }

        button.okp-history-row:hover {
            background: rgba(255, 255, 255, 0.06);
        }

        button.okp-history-row:focus-visible {
            outline: none;
            box-shadow: inset 0 0 0 1px alpha(@okp_accent, 0.62);
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
            color: alpha(@okp_accent_bright, 0.86);
            font-weight: 650;
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
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
        }

        .okp-idle-canvas.is-light {
            background: linear-gradient(135deg, #f7f9fa, #edf3f6);
            color: #161616;
        }

        .okp-idle-canvas.is-dark {
            background: linear-gradient(135deg, #232629, #1b1e21);
            color: rgba(255, 255, 255, 0.94);
        }

        .okp-idle-canvas.is-preview-substrate {
            background: #050507;
        }

        .okp-idle-canvas.is-preview-substrate.is-preview-bright {
            background: #f4f7fa;
        }

        .okp-idle-titlebar {
            min-height: 34px;
            padding-left: 15px;
        }

        .is-light .okp-idle-titlebar-mark { color: @okp_teal; }
        .is-dark .okp-idle-titlebar-mark { color: @okp_accent; }

        .okp-idle-titlebar-text {
            font-size: 12.5px;
            font-weight: 600;
        }

        .is-light .okp-idle-titlebar-text { color: rgba(0, 0, 0, 0.72); }
        .is-dark .okp-idle-titlebar-text { color: rgba(255, 255, 255, 0.76); }

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
            box-shadow: 0 8px 22px alpha(@okp_teal_deep, 0.24);
        }

        .okp-welcome-first-run {
            padding: 34px 24px;
        }

        .okp-welcome-brand-tile { margin-bottom: 13px; }

        .okp-first-run-title {
            font-family: 'Segoe UI Variable Display', 'Segoe UI', sans-serif;
            font-size: 17px;
            font-weight: 600;
        }

        .is-light .okp-first-run-title { color: #161616; }
        .is-dark .okp-first-run-title { color: rgba(255, 255, 255, 0.94); }

        .okp-first-run-copy {
            margin-top: 5px;
            font-size: 12.5px;
        }

        .is-light .okp-first-run-copy { color: rgba(0, 0, 0, 0.52); }
        .is-dark .okp-first-run-copy { color: rgba(255, 255, 255, 0.58); }

        button.okp-first-run-drop-target,
        button.okp-welcome-drop-target {
            border: 1.5px dashed rgba(128, 128, 128, 0.48);
            box-shadow: none;
        }

        button.okp-first-run-drop-target {
            min-width: 280px;
            min-height: 70px;
            margin-top: 13px;
            padding: 12px 16px;
            border-radius: 10px;
        }

        button.okp-welcome-drop-target {
            min-width: 280px;
            min-height: 82px;
            padding: 0 16px;
            border-radius: 9px;
        }

        .is-light button.okp-first-run-drop-target,
        .is-light button.okp-welcome-drop-target {
            background: rgba(255, 255, 255, 0.52);
            color: rgba(0, 0, 0, 0.56);
        }

        .is-dark button.okp-first-run-drop-target,
        .is-dark button.okp-welcome-drop-target {
            background: rgba(255, 255, 255, 0.035);
            color: rgba(255, 255, 255, 0.58);
        }

        .okp-empty-surface.is-drop-target button.okp-first-run-drop-target,
        .okp-empty-surface.is-drop-target button.okp-welcome-drop-target {
            border-color: @okp_accent;
            background: alpha(@okp_accent, 0.12);
        }

        .okp-drop-primary {
            font-size: 12.5px;
            font-weight: 600;
        }

        .is-light .okp-first-run-drop-target .okp-drop-primary { color: @okp_teal_deep; }
        .is-dark .okp-first-run-drop-target .okp-drop-primary { color: @okp_accent_bright; }

        .okp-drop-secondary {
            font-size: 11px;
        }

        .okp-welcome-recents {
            padding: 34px 32px 0;
        }

        .okp-welcome-recents-title {
            font-family: 'Segoe UI Variable Display', 'Segoe UI', sans-serif;
            font-size: 30px;
            font-weight: 600;
            letter-spacing: 0;
        }

        .is-light .okp-welcome-recents-title { color: #161616; }
        .is-dark .okp-welcome-recents-title { color: rgba(255, 255, 255, 0.94); }

        .okp-welcome-recents-subtitle {
            margin-top: 6px;
            font-size: 13.5px;
        }

        .is-light .okp-welcome-recents-subtitle { color: rgba(0, 0, 0, 0.52); }
        .is-dark .okp-welcome-recents-subtitle { color: rgba(255, 255, 255, 0.58); }

        .okp-recents-shelf {
            margin-top: 20px;
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
            border-radius: 8px;
            background: transparent;
            box-shadow: none;
        }

        button.okp-recents-history-button {
            min-width: 36px;
            min-height: 36px;
            padding: 0;
            border: none;
            border-radius: 18px;
            box-shadow: none;
        }

        .is-light button.okp-recents-history-button {
            background: rgba(0, 0, 0, 0.045);
            color: rgba(0, 0, 0, 0.52);
        }

        .is-dark button.okp-recents-history-button {
            background: rgba(255, 255, 255, 0.055);
            color: rgba(255, 255, 255, 0.58);
        }

        .is-light button.okp-recents-history-button:hover { background: rgba(0, 0, 0, 0.08); }
        .is-dark button.okp-recents-history-button:hover { background: rgba(255, 255, 255, 0.10); }

        .is-light button.okp-recent-card { color: #161616; }
        .is-dark button.okp-recent-card { color: rgba(255, 255, 255, 0.94); }

        .is-light button.okp-recent-card:hover { background: rgba(0, 0, 0, 0.045); }
        .is-dark button.okp-recent-card:hover { background: rgba(255, 255, 255, 0.055); }

        .okp-history-thumbnail { border-radius: 8px; }

        .is-light .okp-history-thumbnail-placeholder {
            background: linear-gradient(135deg, #d9e3e8, #bdcbd2);
            border: 1px solid rgba(0, 0, 0, 0.06);
        }

        .is-dark .okp-history-thumbnail-placeholder {
            background: linear-gradient(135deg, #343a40, #202429);
            border: 1px solid rgba(255, 255, 255, 0.07);
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
            background: rgba(255, 255, 255, 0.30);
        }

        progressbar.okp-recent-progress progress,
        progressbar.okp-history-thumb-progress progress {
            min-width: 0;
            min-height: 4px;
            border: none;
            background: @okp_accent;
        }

        .okp-recent-time-left {
            padding: 2px 6px;
            border-radius: 5px;
            background: rgba(0, 0, 0, 0.58);
            color: #ffffff;
            font-size: 10px;
            font-weight: 600;
        }

        .okp-recent-title {
            margin-top: 9px;
            font-size: 13px;
            font-weight: 500;
        }

        .is-light .okp-recent-title { color: #202020; }
        .is-dark .okp-recent-title { color: rgba(255, 255, 255, 0.92); }

        .okp-recent-location {
            margin-top: 2px;
            font-size: 11.5px;
        }

        .is-light .okp-recent-location { color: rgba(0, 0, 0, 0.46); }
        .is-dark .okp-recent-location { color: rgba(255, 255, 255, 0.48); }

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
            padding: 0 12px;
            border-radius: 7px;
            box-shadow: none;
            font-size: 12.5px;
            font-weight: 600;
        }

        button.okp-idle-primary-button {
            background: @okp_teal;
            border: 1px solid @okp_teal;
            color: #ffffff;
        }

        button.okp-idle-primary-button:hover { background: @okp_teal_deep; }

        .is-light button.okp-idle-secondary-button {
            background: rgba(0, 0, 0, 0.045);
            border: 1px solid rgba(0, 0, 0, 0.08);
            color: #202020;
        }

        .is-dark button.okp-idle-secondary-button {
            background: rgba(255, 255, 255, 0.055);
            border: 1px solid rgba(255, 255, 255, 0.09);
            color: rgba(255, 255, 255, 0.90);
        }

        .okp-welcome-private {
            padding: 36px 24px;
        }

        .okp-private-hero-icon { color: @okp_accent; }

        .okp-private-hero-title {
            margin-top: 12px;
            font-size: 17px;
            font-weight: 600;
        }

        .okp-private-hero-copy {
            margin-top: 7px;
            font-size: 12.5px;
            line-height: 1.45;
        }

        .is-light .okp-private-hero-copy { color: rgba(0, 0, 0, 0.52); }
        .is-dark .okp-private-hero-copy { color: rgba(255, 255, 255, 0.56); }

        .okp-private-actions { margin-top: 20px; }

        .okp-idle-footer {
            min-height: 42px;
            padding: 0 24px;
            border-top: 1px solid rgba(128, 128, 128, 0.18);
        }

        button.okp-idle-footer-button {
            min-height: 30px;
            padding: 4px 7px;
            border: none;
            border-radius: 7px;
            background: transparent;
            box-shadow: none;
        }

        .is-light button.okp-idle-footer-button { color: rgba(0, 0, 0, 0.52); }
        .is-dark button.okp-idle-footer-button { color: rgba(255, 255, 255, 0.56); }
        .is-light button.okp-idle-footer-button:hover { background: rgba(0, 0, 0, 0.05); }
        .is-dark button.okp-idle-footer-button:hover { background: rgba(255, 255, 255, 0.06); }

        .okp-idle-footer-status {
            font-size: 11px;
        }

        .is-light .okp-idle-footer-status { color: rgba(0, 0, 0, 0.40); }
        .is-dark .okp-idle-footer-status { color: rgba(255, 255, 255, 0.42); }
        .okp-idle-footer-status:not(.is-private) { color: @okp_teal; }

        .okp-history-page {
            padding: 30px 26px 40px;
        }

        .okp-history-header { min-height: 44px; }

        button.okp-history-back-button {
            min-width: 32px;
            min-height: 32px;
            padding: 0;
            border: none;
            border-radius: 8px;
            box-shadow: none;
        }

        .is-light button.okp-history-back-button { background: rgba(0, 0, 0, 0.045); color: #202020; }
        .is-dark button.okp-history-back-button { background: rgba(255, 255, 255, 0.055); color: #ffffff; }

        .okp-history-title {
            font-family: 'Segoe UI Variable Display', 'Segoe UI', sans-serif;
            font-size: 30px;
            font-weight: 600;
            letter-spacing: 0;
        }

        .is-light .okp-history-title { color: #161616; }
        .is-dark .okp-history-title { color: rgba(255, 255, 255, 0.94); }

        entry.okp-history-search {
            min-width: 220px;
            min-height: 36px;
            border-radius: 8px;
            box-shadow: none;
        }

        .is-light entry.okp-history-search {
            background: rgba(0, 0, 0, 0.035);
            border: 1px solid rgba(0, 0, 0, 0.08);
            color: #161616;
        }

        .is-dark entry.okp-history-search {
            background: rgba(255, 255, 255, 0.05);
            border: 1px solid rgba(255, 255, 255, 0.09);
            color: rgba(255, 255, 255, 0.92);
        }

        .okp-history-subtitle {
            margin-top: 8px;
            font-size: 13.5px;
        }

        .is-light .okp-history-subtitle { color: rgba(0, 0, 0, 0.50); }
        .is-dark .okp-history-subtitle { color: rgba(255, 255, 255, 0.54); }

        .okp-history-divider { margin-top: 16px; }

        .okp-history-private-banner {
            margin-top: 16px;
            padding: 9px 13px;
            border-radius: 8px;
            font-size: 12px;
            font-weight: 500;
        }

        .is-light .okp-history-private-banner {
            background: rgba(0, 0, 0, 0.035);
            border: 1px solid rgba(0, 0, 0, 0.07);
            color: rgba(0, 0, 0, 0.52);
        }

        .is-dark .okp-history-private-banner {
            background: rgba(255, 255, 255, 0.045);
            border: 1px solid rgba(255, 255, 255, 0.08);
            color: rgba(255, 255, 255, 0.56);
        }

        .okp-history-result-caption,
        .okp-history-bucket {
            margin-top: 20px;
            padding: 0 12px 8px;
            font-size: 11px;
            font-weight: 600;
        }

        .okp-history-bucket { letter-spacing: 1px; }

        .is-light .okp-history-result-caption,
        .is-light .okp-history-bucket { color: rgba(0, 0, 0, 0.48); }
        .is-dark .okp-history-result-caption,
        .is-dark .okp-history-bucket { color: rgba(255, 255, 255, 0.48); }

        button.okp-history-row {
            margin-bottom: 4px;
            padding: 12px;
            border: none;
            border-radius: 7px;
            background: transparent;
            box-shadow: none;
        }

        .is-light button.okp-history-row { color: #161616; }
        .is-dark button.okp-history-row { color: rgba(255, 255, 255, 0.92); }
        .is-light button.okp-history-row:hover { background: rgba(0, 0, 0, 0.04); }
        .is-dark button.okp-history-row:hover { background: rgba(255, 255, 255, 0.05); }

        .okp-history-row-title { font-size: 13px; font-weight: 500; }
        .is-light .okp-history-row-title { color: #202020; }
        .is-dark .okp-history-row-title { color: rgba(255, 255, 255, 0.92); }
        .okp-history-row-location,
        .okp-history-row-when,
        .okp-history-progress-label,
        .okp-history-barely-label { font-size: 11.5px; }

        .okp-history-row-when,
        .okp-history-progress-label,
        .okp-history-barely-label {
            font-feature-settings: 'tnum';
        }

        .is-light .okp-history-row-location,
        .is-light .okp-history-row-when,
        .is-light .okp-history-barely-label { color: rgba(0, 0, 0, 0.42); }
        .is-dark .okp-history-row-location,
        .is-dark .okp-history-row-when,
        .is-dark .okp-history-barely-label { color: rgba(255, 255, 255, 0.42); }

        .okp-history-progress-label { color: @okp_teal; font-weight: 600; }

        .okp-history-finished-chip {
            padding: 3px 7px;
            border-radius: 6px;
            font-size: 10.5px;
            font-weight: 600;
        }

        .is-light .okp-history-finished-chip { background: rgba(0, 0, 0, 0.045); color: rgba(0, 0, 0, 0.52); }
        .is-dark .okp-history-finished-chip { background: rgba(255, 255, 255, 0.055); color: rgba(255, 255, 255, 0.56); }

        .okp-history-end-cap {
            margin-top: 26px;
            padding-top: 18px;
            border-top: 1px solid rgba(128, 128, 128, 0.18);
            font-size: 11px;
        }

        .is-light .okp-history-end-cap { color: rgba(0, 0, 0, 0.36); }
        .is-dark .okp-history-end-cap { color: rgba(255, 255, 255, 0.36); }

        .okp-idle-canvas.is-high-contrast {
            background: #000000;
            color: #ffffff;
        }

        .is-high-contrast .okp-idle-titlebar-text,
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

        .okp-history-state-card {
            min-width: 360px;
            margin-top: 30px;
            padding: 34px 28px;
            border-radius: 12px;
        }

        .is-light .okp-history-state-card { background: rgba(255, 255, 255, 0.52); border: 1px solid rgba(0, 0, 0, 0.07); }
        .is-dark .okp-history-state-card { background: rgba(255, 255, 255, 0.035); border: 1px solid rgba(255, 255, 255, 0.08); }

        .okp-history-state-icon-wrap {
            min-width: 54px;
            min-height: 54px;
            border-radius: 14px;
            background: alpha(@okp_accent, 0.12);
            color: @okp_teal;
        }

        .okp-history-state-title { font-size: 16px; font-weight: 600; }

        .okp-history-state-body { font-size: 12.5px; line-height: 1.55; }
        .is-light .okp-history-state-body { color: rgba(0, 0, 0, 0.50); }
        .is-dark .okp-history-state-body { color: rgba(255, 255, 255, 0.54); }

        button.okp-history-state-button {
            min-height: 34px;
            padding: 7px 18px;
            border-radius: 7px;
            background: @okp_teal;
            border: none;
            color: #ffffff;
            font-size: 12px;
            font-weight: 600;
        }

        .okp-history-loading { margin-top: 18px; }
        .okp-history-skeleton-row { padding: 9px 12px; }
        .okp-history-skeleton-caption { min-width: 64px; min-height: 10px; margin: 0 12px 12px; border-radius: 4px; }
        .okp-history-skeleton-thumb { min-width: 64px; min-height: 36px; border-radius: 6px; }
        .okp-history-skeleton-line-1 { min-width: 280px; min-height: 11px; border-radius: 4px; }
        .okp-history-skeleton-line-2 { min-width: 170px; min-height: 9px; border-radius: 4px; }
        .is-light .okp-history-skeleton-caption,
        .is-light .okp-history-skeleton-thumb,
        .is-light .okp-history-skeleton-line-1,
        .is-light .okp-history-skeleton-line-2 { background: rgba(0, 0, 0, 0.08); }
        .is-dark .okp-history-skeleton-caption,
        .is-dark .okp-history-skeleton-thumb,
        .is-dark .okp-history-skeleton-line-1,
        .is-dark .okp-history-skeleton-line-2 { background: rgba(255, 255, 255, 0.08); }

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
            color: alpha(@okp_accent_bright, 0.90);
            font-size: 11px;
            font-weight: 760;
            letter-spacing: 2px;
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
            border-radius: 10px;
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
            font-weight: 780;
        }

        button.okp-lyrics-line.is-active:hover {
            background: rgba(255, 255, 255, 0.06);
        }

        button.okp-lyrics-line:focus-visible {
            outline: none;
            box-shadow: 0 0 0 2px alpha(@okp_accent, 0.55);
        }

        .okp-lyrics-line-plain {
            padding: 7px 14px;
            color: rgba(255, 255, 255, 0.82);
            font-size: 16px;
            font-weight: 500;
        }

        .okp-lyrics-empty-icon {
            color: alpha(@okp_accent_bright, 0.80);
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
            border-radius: 14px;
            background: rgba(22, 22, 25, 0.56);
            border: 1px solid rgba(255, 255, 255, 0.14);
            box-shadow: 0 14px 40px rgba(0, 0, 0, 0.32);
        }

        .okp-compact-top-bar {
            background-image: radial-gradient(ellipse at top, rgba(0, 0, 0, 0.50), rgba(22, 22, 25, 0.56) 72%);
        }

        .okp-compact-bottom-bar {
            background-image: radial-gradient(ellipse at bottom, rgba(0, 0, 0, 0.50), rgba(22, 22, 25, 0.56) 72%);
        }

        button.okp-compact-button {
            min-width: 28px;
            min-height: 28px;
            padding: 0;
            border: 1px solid transparent;
            border-radius: 8px;
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
            box-shadow: 0 0 0 2px alpha(@okp_accent, 0.60);
        }

        button.okp-compact-close:hover {
            background: alpha(@okp_danger_dark, 0.86);
        }

        button.okp-compact-play {
            min-width: 42px;
            min-height: 42px;
            border-radius: 21px;
            background: rgba(22, 22, 25, 0.56);
            border: 1px solid rgba(255, 255, 255, 0.14);
            box-shadow: 0 6px 20px rgba(0, 0, 0, 0.40);
            -gtk-icon-size: 21px;
        }

        .okp-compact-title {
            color: rgba(255, 255, 255, 0.95);
            font-size: 11px;
            font-weight: 600;
        }

        .okp-compact-time {
            color: rgba(255, 255, 255, 0.82);
            font-size: 10px;
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
            border-radius: 2px;
            background: rgba(255, 255, 255, 0.30);
            box-shadow: none;
        }

        scale.okp-compact-seek highlight {
            min-width: 2px;
            min-height: 3px;
            border: none;
            border-radius: 2px;
            background: @okp_accent;
        }

        scale.okp-compact-seek slider {
            min-width: 10px;
            min-height: 10px;
            margin: 0;
            border: none;
            border-radius: 6px;
            background: #ffffff;
            box-shadow: 0 1px 4px rgba(0, 0, 0, 0.45);
        }

        window.okp-player-window.is-high-contrast .okp-compact-top-bar,
        window.okp-player-window.is-high-contrast .okp-compact-bottom-bar,
        window.okp-player-window.is-reduced-transparency .okp-compact-top-bar,
        window.okp-player-window.is-reduced-transparency .okp-compact-bottom-bar {
            background: #161619;
            background-image: none;
            border-color: #ffffff;
            box-shadow: none;
        }

        .okp-root.is-reduced-motion .okp-compact-motion {
            transition: none;
        }

        .okp-bottom-scrim {
            background: linear-gradient(to top, rgba(0, 0, 0, 0.48), rgba(0, 0, 0, 0));
        }

        .okp-controls {
            min-height: 32px;
            /* The adaptive OscBar owns the 7px/14px content inset in its own
             * allocation (issue #328), so the CSS padding is zeroed to avoid
             * double-insetting the controls. */
            padding: 0;
            border-radius: 14px;
            /* GTK 4 has no portable backdrop-filter. This 50% tint plus the
             * localized scrim is the deterministic fallback for the locked
             * 24-26px blur/saturation material used on platforms that support it. */
            background: rgba(22, 22, 25, 0.50);
            border: 1px solid rgba(255, 255, 255, 0.12);
            box-shadow: 0 12px 34px rgba(0, 0, 0, 0.34);
        }

        button.okp-control-button,
        menubutton.okp-control-button > button {
            min-width: 32px;
            min-height: 32px;
            padding: 0;
            border-radius: 8px;
            border: 1px solid transparent;
            background: transparent;
            box-shadow: none;
            color: rgba(255, 255, 255, 0.86);
            font-size: 12.5px;
            font-weight: 600;
        }

        button.okp-control-button:hover,
        menubutton.okp-control-button > button:hover {
            background: rgba(255, 255, 255, 0.11);
            color: rgba(255, 255, 255, 0.96);
        }

        button.okp-control-button:active,
        menubutton.okp-control-button > button:active,
        button.okp-control-button:checked,
        menubutton.okp-control-button > button:checked {
            background: alpha(@okp_accent, 0.24);
            border-color: alpha(@okp_accent, 0.42);
            color: rgba(255, 255, 255, 0.98);
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
            box-shadow: 0 0 0 2px alpha(@okp_accent, 0.55);
        }

        button.okp-play-button {
            min-width: 32px;
            border-radius: 8px;
            background: transparent;
            color: #ffffff;
            -gtk-icon-size: 22px;
        }

        button.okp-play-button:hover {
            background: rgba(255, 255, 255, 0.12);
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
            padding: 0 8px;
            background: rgba(255, 255, 255, 0.14);
            color: rgba(255, 255, 255, 0.92);
            font-feature-settings: 'tnum';
        }

        .okp-control-button.is-selected {
            background: alpha(@okp_accent, 0.22);
        }

        .okp-time-label {
            color: rgba(255, 255, 255, 0.85);
            font-size: 12.5px;
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
            border-radius: 10px;
            background: rgba(22, 22, 25, 0.60);
            border: 1px solid rgba(255, 255, 255, 0.10);
            box-shadow: 0 14px 34px rgba(0, 0, 0, 0.42);
            color: rgba(255, 255, 255, 0.92);
            font-size: 13px;
            font-weight: 600;
        }

        .okp-status-toast-thumbnail {
            min-width: 64px;
            min-height: 36px;
            border-radius: 5px;
            background: #050608;
        }

        .okp-update-action-surface {
            padding: 12px 14px;
            border-radius: 8px;
            background: #f8fafb;
            border: 1px solid rgba(0, 0, 0, 0.06);
        }

        .okp-update-action-title {
            color: @okp_ink;
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 13px;
            font-weight: 600;
        }

        .okp-update-action-detail {
            color: rgba(0, 0, 0, 0.50);
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 12px;
        }

        button.okp-update-primary-button {
            min-height: 30px;
            padding: 5px 14px;
            border-radius: 7px;
            border: none;
            background: @okp_teal;
            color: #ffffff;
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 12px;
            font-weight: 600;
            box-shadow: none;
        }

        button.okp-update-primary-button:hover {
            background: @okp_teal_deep;
        }

        button.okp-update-primary-button:focus-visible {
            outline: none;
            box-shadow: 0 0 0 2px alpha(@okp_teal, 0.35);
        }

        button.okp-update-primary-button:disabled {
            opacity: 0.64;
        }

        .okp-persistent-update {
            padding: 12px 14px;
            border-radius: 10px;
            background: rgba(22, 22, 25, 0.88);
            border: 1px solid rgba(255, 255, 255, 0.12);
            box-shadow: 0 14px 34px rgba(0, 0, 0, 0.42);
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

        .okp-timeline,
        .okp-seek {
            min-width: 120px;
            min-height: 20px;
        }

        .okp-timeline-rail {
            min-width: 120px;
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
            border-radius: 9px;
            background: rgba(14, 15, 18, 0.94);
            border: 1px solid rgba(255, 255, 255, 0.10);
            box-shadow: 0 12px 30px rgba(0, 0, 0, 0.40);
        }

        .okp-seek-preview-thumb {
            margin-bottom: 6px;
            border-radius: 5px;
            background: rgba(255, 255, 255, 0.08);
        }

        .okp-seek-preview-time {
            color: rgba(255, 255, 255, 0.92);
            font-size: 12px;
            font-weight: 700;
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
            border-radius: 6px;
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
            color: #F0B840;
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
            border-radius: 13px;
            background: rgba(28, 28, 32, 0.72);
            border: 1px solid rgba(255, 255, 255, 0.13);
            box-shadow: 0 14px 34px rgba(0, 0, 0, 0.42);
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
            border-radius: 7px;
            background: rgba(255, 255, 255, 0.98);
            box-shadow: 0 2px 8px rgba(0, 0, 0, 0.46);
        }

        .okp-volume-capsule.is-boosted scale.okp-volume-slider slider {
            background: #F0B840;
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
            padding: 0 2px;
            border: none;
            background: transparent;
            box-shadow: none;
            color: rgba(255, 255, 255, 0.94);
            font-size: 12px;
            font-weight: 700;
            font-feature-settings: 'tnum';
        }

        button.okp-volume-readout:hover,
        button.okp-volume-readout:focus-visible {
            background: rgba(255, 255, 255, 0.09);
        }

        .okp-volume-capsule.is-boosted button.okp-volume-readout,
        .okp-volume-capsule.is-muted button.okp-volume-readout {
            color: #F0B840;
        }

        entry.okp-volume-readout-input {
            padding: 2px 5px;
            border-radius: 5px;
            background: rgba(255, 255, 255, 0.10);
            border: 1px solid @okp_accent;
            color: rgba(255, 255, 255, 0.96);
            font-size: 12px;
            font-feature-settings: 'tnum';
        }

        .okp-up-next-panel {
            border-radius: 12px 0 0 12px;
            background: rgba(244, 244, 244, 0.96);
            border-style: solid;
            border-color: rgba(0, 0, 0, 0.12);
            border-width: 1px 0 1px 1px;
            box-shadow: -18px 18px 48px rgba(0, 0, 0, 0.30);
        }

        .okp-side-panel-header {
            padding: 14px 8px 6px 14px;
        }

        .okp-side-panel-tabs {
            padding: 3px;
            border-radius: 8px;
            background: rgba(0, 0, 0, 0.08);
        }

        button.okp-side-panel-tab {
            min-height: 28px;
            padding: 0 16px;
            border-radius: 6px;
            border: none;
            background: transparent;
            box-shadow: none;
            color: rgba(0, 0, 0, 0.50);
            font-size: 12.5px;
            font-weight: 650;
        }

        button.okp-side-panel-tab:hover {
            background: rgba(255, 255, 255, 0.55);
            color: rgba(0, 0, 0, 0.74);
        }

        button.okp-side-panel-tab.is-selected {
            background: rgba(255, 255, 255, 0.96);
            color: @okp_teal_deep;
            box-shadow: 0 1px 4px rgba(0, 0, 0, 0.16);
        }

        button.okp-side-panel-tab:focus-visible {
            outline: none;
            box-shadow: inset 0 0 0 1px alpha(@okp_accent, 0.6);
        }

        button.okp-side-panel-close {
            min-width: 30px;
            min-height: 30px;
            padding: 0;
            border: none;
            border-radius: 6px;
            background: transparent;
            color: rgba(0, 0, 0, 0.46);
            box-shadow: none;
        }

        button.okp-side-panel-close:hover {
            background: rgba(0, 0, 0, 0.07);
            color: rgba(0, 0, 0, 0.74);
        }

        button.okp-side-panel-close:focus-visible {
            outline: none;
            box-shadow: inset 0 0 0 1px alpha(@okp_accent, 0.6);
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
            color: rgba(0, 0, 0, 0.42);
            font-size: 11px;
            font-weight: 720;
        }

        .okp-up-next-list row.okp-panel-empty-row {
            min-height: 88px;
            margin: 4px 6px 8px 6px;
            padding: 20px 18px;
            border-radius: 8px;
            background: rgba(255, 255, 255, 0.44);
            border: 1px dashed rgba(0, 0, 0, 0.18);
        }

        .okp-panel-empty {
            color: rgba(0, 0, 0, 0.58);
            font-size: 12.5px;
        }

        .okp-panel-caption-row {
            padding: 0 12px 6px 12px;
        }

        .okp-panel-caption {
            color: rgba(0, 0, 0, 0.48);
            font-size: 11.5px;
        }

        .okp-up-next-row {
            min-height: 40px;
            margin: 1px 0;
            padding: 7px 8px;
            border-radius: 6px;
            border: 1px solid transparent;
            background: transparent;
            color: rgba(0, 0, 0, 0.78);
        }

        .okp-chapter-row {
            min-height: 42px;
            padding: 5px 0;
        }

        .okp-chapter-current-rail {
            min-width: 3px;
            min-height: 34px;
            border-radius: 2px;
            background: @okp_teal;
            opacity: 0;
        }

        .okp-chapter-current-rail.is-current {
            opacity: 1;
        }

        .okp-chapter-thumb {
            min-width: 56px;
            min-height: 32px;
            border-radius: 4px;
            background: rgba(0, 0, 0, 0.08);
            border: 1px solid rgba(0, 0, 0, 0.06);
        }

        .okp-chapter-thumb.is-pending {
            background: rgba(0, 0, 0, 0.045);
            border-style: dashed;
            border-color: rgba(0, 0, 0, 0.12);
        }

        .okp-chapter-thumb-placeholder {
            color: rgba(0, 0, 0, 0.30);
        }

        .okp-bookmark-row .okp-bookmark-icon {
            color: alpha(@okp_accent, 0.92);
        }

        .okp-up-next-list row.okp-interval-row {
            background: alpha(@okp_teal, 0.045);
            border-style: dashed;
            border-color: rgba(0, 0, 0, 0.13);
        }

        .okp-interval-row .okp-interval-icon {
            color: rgba(0, 0, 0, 0.42);
        }

        .okp-interval-row .okp-up-next-marker {
            color: rgba(0, 0, 0, 0.52);
        }

        .okp-up-next-list row.okp-detect-row {
            min-height: 52px;
            margin: 2px 6px 8px 6px;
            background: alpha(@okp_teal, 0.11);
            border-color: alpha(@okp_teal, 0.40);
            color: @okp_teal_deep;
        }

        .okp-up-next-list row.okp-detect-row:hover {
            background: alpha(@okp_teal, 0.17);
            border-color: alpha(@okp_teal, 0.62);
            color: @okp_teal_deep;
        }

        .okp-detect-row .okp-detect-icon {
            color: @okp_teal;
        }

        .okp-detect-subtitle {
            color: rgba(0, 0, 0, 0.52);
            font-size: 11px;
        }

        .okp-detect-status-row {
            margin: 2px 6px 8px 6px;
            padding: 9px 10px;
            border-radius: 6px;
            background: rgba(255, 255, 255, 0.44);
            border: 1px dashed rgba(0, 0, 0, 0.16);
        }

        .okp-detect-status-icon {
            color: rgba(0, 0, 0, 0.40);
        }

        .okp-detect-status {
            color: rgba(0, 0, 0, 0.58);
            font-size: 12px;
        }

        .okp-up-next-list row.okp-interval-row:focus-visible,
        .okp-up-next-list row.okp-detect-row:focus-visible {
            outline: none;
            box-shadow: inset 0 0 0 2px alpha(@okp_teal, 0.58);
        }

        .okp-up-next-list row.okp-add-bookmark-row {
            margin-top: 6px;
            background: @okp_teal;
            border-color: @okp_teal;
            color: #ffffff;
            font-weight: 650;
        }

        .okp-up-next-list row.okp-add-bookmark-row:hover {
            background: @okp_teal_deep;
            border-color: @okp_teal_deep;
            color: #ffffff;
        }

        .okp-add-bookmark-row .okp-add-bookmark-icon {
            color: #ffffff;
        }

        .okp-up-next-list row.okp-add-files-row {
            min-height: 70px;
            margin: 6px;
            background: rgba(255, 255, 255, 0.44);
            border-style: dashed;
            border-color: alpha(@okp_teal, 0.42);
            color: @okp_teal_deep;
            font-weight: 650;
        }

        .okp-up-next-list row.okp-add-files-row:hover {
            background: alpha(@okp_teal, 0.12);
            border-color: alpha(@okp_teal, 0.64);
            color: @okp_teal_deep;
        }

        .okp-add-files-row .okp-add-files-icon {
            color: alpha(@okp_accent, 0.92);
        }

        /* The lone now-playing card at the top of a short queue has no reorder /
           remove controls, so give it a touch more breathing room than a regular
           queue row so it reads as a pinned card rather than a stripped row. */
        .okp-up-next-list row.okp-now-playing-pinned-row {
            min-height: 50px;
            margin: 6px 6px 10px 6px;
            padding: 8px;
            border-radius: 8px;
            background: alpha(@okp_teal, 0.12);
            color: @okp_teal_deep;
        }

        .okp-now-playing-thumb {
            min-width: 54px;
            min-height: 34px;
            border-radius: 5px;
            background: linear-gradient(135deg, #16352b, @okp_teal);
            color: rgba(255, 255, 255, 0.56);
        }

        .okp-now-playing-title {
            color: @okp_teal_deep;
            font-size: 12.5px;
            font-weight: 650;
        }

        .okp-now-playing-state {
            color: @okp_teal;
            font-size: 10.5px;
            font-weight: 650;
        }

        .okp-up-next-row.is-current .okp-chapter-thumb {
            border-color: alpha(@okp_accent, 0.55);
        }

        .okp-up-next-row:hover {
            background: rgba(0, 0, 0, 0.055);
        }

        .okp-up-next-row.is-current {
            background: alpha(@okp_accent, 0.10);
            border-color: alpha(@okp_teal, 0.18);
            color: @okp_teal_deep;
        }

        .okp-up-next-row.is-behind {
            background: transparent;
            color: rgba(0, 0, 0, 0.42);
        }

        .okp-up-next-row.is-behind:hover {
            background: rgba(0, 0, 0, 0.04);
            color: rgba(0, 0, 0, 0.62);
        }

        .okp-up-next-row.is-behind .okp-up-next-source-icon {
            opacity: 0.55;
        }

        .okp-up-next-row.is-drop-target {
            background: alpha(@okp_accent, 0.22);
            border-color: alpha(@okp_accent, 0.62);
        }

        .okp-up-next-drag-handle {
            min-width: 18px;
            color: rgba(0, 0, 0, 0.24);
        }

        .okp-up-next-drag-handle-icon {
            -gtk-icon-size: 16px;
        }

        .okp-up-next-row:hover .okp-up-next-drag-handle,
        .okp-up-next-row.is-drop-target .okp-up-next-drag-handle {
            color: rgba(0, 0, 0, 0.64);
        }

        .okp-up-next-lane {
            min-width: 42px;
        }

        .okp-up-next-index {
            min-width: 22px;
            color: rgba(0, 0, 0, 0.40);
            font-size: 11px;
            font-weight: 620;
            font-feature-settings: 'tnum';
        }

        .okp-up-next-watched-icon {
            color: @okp_teal;
        }

        .okp-up-next-source-icon {
            color: rgba(0, 0, 0, 0.46);
        }

        .okp-up-next-row.is-current .okp-up-next-source-icon {
            color: @okp_teal_deep;
        }

        .okp-now-badge {
            padding: 1px 7px;
            border-radius: 999px;
            background: @okp_accent;
            color: #041110;
            font-size: 9.5px;
            font-weight: 800;
            letter-spacing: 0;
        }

        .okp-next-badge {
            padding: 1px 7px;
            border-radius: 999px;
            background: alpha(@okp_accent, 0.18);
            color: alpha(@okp_accent, 0.98);
            font-size: 9.5px;
            font-weight: 760;
            letter-spacing: 0;
        }

        .okp-up-next-marker {
            color: rgba(0, 0, 0, 0.45);
            font-size: 11px;
            font-weight: 560;
            font-feature-settings: 'tnum';
        }

        .okp-up-next-row.is-current .okp-up-next-marker {
            color: @okp_teal_deep;
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
            border-radius: 5px;
            background: transparent;
            color: rgba(0, 0, 0, 0.42);
            box-shadow: none;
        }

        menubutton.okp-up-next-actions-menu > button:hover {
            background: rgba(0, 0, 0, 0.07);
            color: rgba(0, 0, 0, 0.72);
        }

        .okp-up-next-actions-popover {
            min-width: 142px;
            padding: 6px;
            background: #f4f4f4;
        }

        button.okp-up-next-menu-action {
            min-height: 30px;
            padding: 5px 8px;
            border: none;
            border-radius: 5px;
            background: transparent;
            color: rgba(0, 0, 0, 0.78);
            box-shadow: none;
        }

        button.okp-up-next-menu-action:hover {
            background: rgba(0, 0, 0, 0.07);
        }

        button.okp-up-next-action-button {
            min-width: 24px;
            min-height: 24px;
            padding: 0;
            border: none;
            border-radius: 5px;
            background: transparent;
            box-shadow: none;
            color: rgba(0, 0, 0, 0.46);
        }

        button.okp-up-next-action-button:hover {
            background: rgba(0, 0, 0, 0.07);
            color: rgba(0, 0, 0, 0.78);
        }

        button.okp-up-next-action-button:disabled {
            color: rgba(0, 0, 0, 0.18);
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
            background: rgba(0, 0, 0, 0.22);
        }

        .okp-track-popover-content {
            padding: 8px;
            background: #f7f7f5;
            color: #17191c;
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
            border-radius: 8px;
            background: #f7f7f5;
            border: 1px solid rgba(0, 0, 0, 0.16);
            box-shadow: 0 12px 34px rgba(0, 0, 0, 0.26);
        }

        popover.okp-track-popover arrow {
            min-width: 0;
            min-height: 0;
            background: transparent;
            border: none;
            box-shadow: none;
        }

        .okp-command-surface {
            padding: 6px;
            background: #f7f7f5;
        }

        entry.okp-command-search {
            min-height: 24px;
            margin: 0 0 6px 0;
            padding: 4px 10px 4px 0;
            border-radius: 6px;
            border: 1px solid rgba(0, 0, 0, 0.10);
            background: rgba(0, 0, 0, 0.035);
            color: rgba(23, 25, 28, 0.92);
            box-shadow: none;
            font-size: 12px;
        }

        entry.okp-command-search > image {
            margin: 0 6px 0 8px;
            color: rgba(23, 25, 28, 0.48);
        }

        entry.okp-command-search > text > placeholder {
            color: rgba(23, 25, 28, 0.48);
        }

        entry.okp-command-search:hover {
            background: rgba(0, 0, 0, 0.055);
            border-color: rgba(0, 0, 0, 0.14);
        }

        entry.okp-command-search:focus-within {
            border-color: alpha(@okp_teal, 0.62);
            box-shadow: inset 0 0 0 1px alpha(@okp_teal, 0.20);
            background: rgba(0, 0, 0, 0.045);
        }

        entry.okp-command-search:disabled {
            background: rgba(0, 0, 0, 0.02);
            border-color: rgba(0, 0, 0, 0.06);
            color: rgba(23, 25, 28, 0.42);
        }

        entry.okp-command-search:disabled > image {
            color: rgba(23, 25, 28, 0.30);
        }

        .okp-command-scroll,
        .okp-command-results {
            background: transparent;
        }

        .okp-command-group-title {
            margin: 10px 8px 4px 8px;
            color: rgba(23, 25, 28, 0.50);
            font-size: 10px;
            font-weight: 720;
            letter-spacing: 0.04em;
        }

        separator.okp-command-separator {
            min-height: 1px;
            margin: 4px 8px;
            background: rgba(23, 25, 28, 0.12);
        }

        button.okp-command-row {
            min-height: 26px;
            padding: 3px 8px;
            border-radius: 6px;
            border: 1px solid transparent;
            background: transparent;
            box-shadow: none;
            color: rgba(23, 25, 28, 0.88);
            font-size: 12px;
        }

        button.okp-command-row:hover {
            background: rgba(0, 0, 0, 0.055);
            color: #111316;
        }

        button.okp-command-row:active {
            background: rgba(0, 0, 0, 0.09);
        }

        button.okp-command-row.is-selected {
            background: alpha(@okp_teal, 0.11);
            border-color: alpha(@okp_teal, 0.20);
            color: #0a5f59;
        }

        button.okp-command-row:disabled {
            background: transparent;
            border-color: transparent;
            color: rgba(23, 25, 28, 0.34);
        }

        button.okp-command-row:focus-visible {
            outline: none;
            box-shadow: inset 0 0 0 1px alpha(@okp_teal, 0.72);
        }

        button.okp-command-back-row {
            margin-bottom: 4px;
            font-weight: 700;
        }

        .okp-command-submenu-arrow {
            color: rgba(23, 25, 28, 0.46);
        }

        .okp-command-row-label {
            color: inherit;
        }

        .okp-command-shortcut {
            color: rgba(23, 25, 28, 0.46);
            font-size: 10.5px;
            font-feature-settings: 'tnum';
        }

        .okp-command-no-results {
            min-height: 116px;
            padding: 28px 18px;
        }

        .okp-command-no-results-title {
            color: rgba(23, 25, 28, 0.84);
            font-size: 13px;
            font-weight: 650;
        }

        .okp-command-no-results-hint {
            color: rgba(23, 25, 28, 0.48);
            font-size: 11.5px;
        }

        popover.okp-command-popover.is-dark > contents,
        popover.okp-command-popover.is-dark contents,
        popover.okp-command-popover.is-dark .okp-command-surface {
            background: #252528;
            border-color: rgba(255, 255, 255, 0.11);
        }

        popover.okp-command-popover.is-dark entry.okp-command-search {
            background: rgba(255, 255, 255, 0.055);
            border-color: rgba(255, 255, 255, 0.12);
            color: rgba(255, 255, 255, 0.92);
        }

        popover.okp-command-popover.is-dark entry.okp-command-search > image {
            color: rgba(255, 255, 255, 0.48);
        }

        popover.okp-command-popover.is-dark entry.okp-command-search > text > placeholder {
            color: rgba(255, 255, 255, 0.48);
        }

        popover.okp-command-popover.is-dark entry.okp-command-search:hover {
            background: rgba(255, 255, 255, 0.075);
            border-color: rgba(255, 255, 255, 0.16);
        }

        popover.okp-command-popover.is-dark entry.okp-command-search:focus-within {
            border-color: alpha(@okp_accent, 0.62);
            box-shadow: inset 0 0 0 1px alpha(@okp_accent, 0.22);
            background: rgba(255, 255, 255, 0.065);
        }

        popover.okp-command-popover.is-dark entry.okp-command-search:disabled {
            background: rgba(255, 255, 255, 0.03);
            border-color: rgba(255, 255, 255, 0.08);
            color: rgba(255, 255, 255, 0.42);
        }

        popover.okp-command-popover.is-dark entry.okp-command-search:disabled > image {
            color: rgba(255, 255, 255, 0.30);
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
            background: #1a1a1a;
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
            color: rgba(255, 255, 255, 0.48);
        }

        popover.okp-command-popover.is-dark button.okp-command-row {
            color: rgba(255, 255, 255, 0.88);
        }

        popover.okp-command-popover.is-dark button.okp-command-row:hover {
            background: rgba(255, 255, 255, 0.075);
            color: #ffffff;
        }

        popover.okp-command-popover.is-dark button.okp-command-row:active {
            background: rgba(255, 255, 255, 0.12);
        }

        popover.okp-command-popover.is-dark button.okp-command-row.is-selected {
            background: alpha(@okp_accent, 0.16);
            border-color: alpha(@okp_accent, 0.25);
            color: @okp_accent_bright;
        }

        popover.okp-command-popover.is-dark button.okp-command-row:disabled {
            color: rgba(255, 255, 255, 0.30);
        }

        popover.okp-command-popover.is-dark separator.okp-command-separator {
            background: rgba(255, 255, 255, 0.12);
        }

        popover.okp-command-popover.is-dark .okp-command-submenu-arrow {
            color: rgba(255, 255, 255, 0.48);
        }

        popover.okp-command-popover.is-dark .okp-command-no-results-title {
            color: rgba(255, 255, 255, 0.86);
        }

        .okp-track-popover-scroll {
            background: #f7f7f5;
        }

        .okp-track-popover-title {
            min-height: 24px;
            margin: 0 6px 4px 6px;
            color: #17191c;
            font-size: 12.5px;
            font-weight: 700;
        }

        .okp-track-group-title {
            margin: 10px 4px 4px 4px;
            color: rgba(23, 25, 28, 0.58);
            font-size: 10.5px;
            font-weight: 720;
        }

        .okp-track-subgroup-title {
            margin: 6px 4px 2px 4px;
            color: rgba(23, 25, 28, 0.52);
            font-size: 10px;
            font-weight: 640;
        }

        button.okp-track-row {
            min-height: 32px;
            padding: 5px 7px;
            border-radius: 4px;
            background: transparent;
            border: 1px solid transparent;
            box-shadow: none;
            color: rgba(23, 25, 28, 0.88);
            font-size: 12px;
        }

        button.okp-track-row:hover {
            background: rgba(0, 0, 0, 0.055);
            color: #111316;
        }

        button.okp-track-row:active {
            background: rgba(0, 0, 0, 0.09);
        }

        button.okp-track-row.is-selected {
            background: alpha(@okp_teal, 0.12);
            border-color: alpha(@okp_teal, 0.22);
            color: #0a5f59;
        }

        button.okp-track-row.is-selected .okp-track-row-label {
            font-weight: 640;
        }

        button.okp-audio-track-row {
            min-height: 42px;
        }

        .okp-audio-track-name {
            color: inherit;
            font-size: 12px;
        }

        .okp-audio-track-detail {
            color: rgba(23, 25, 28, 0.54);
            font-size: 10.5px;
        }

        button.okp-audio-track-row.is-selected .okp-audio-track-name {
            font-weight: 640;
        }

        button.okp-track-row:disabled {
            background: transparent;
            border-color: transparent;
            color: rgba(23, 25, 28, 0.38);
        }

        button.okp-track-row:focus-visible {
            outline: none;
            box-shadow: inset 0 0 0 1px alpha(@okp_teal, 0.72);
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
            color: #0a655f;
        }

        button.okp-scribe-row:disabled {
            color: rgba(10, 101, 95, 0.54);
        }

        .okp-scribe-spinner {
            min-width: 16px;
            min-height: 16px;
            color: rgba(10, 101, 95, 0.72);
        }

        .okp-subtitle-action-icon {
            min-width: 16px;
            min-height: 16px;
        }

        .okp-subtitle-action-badge {
            min-height: 16px;
            padding: 0 5px;
            border-radius: 4px;
            background: rgba(0, 0, 0, 0.065);
            color: rgba(23, 25, 28, 0.52);
            font-size: 8.5px;
            font-weight: 720;
        }

        button.okp-scribe-row .okp-subtitle-action-badge {
            background: alpha(@okp_teal, 0.12);
            color: rgba(10, 101, 95, 0.64);
        }

        button.okp-online-subtitle-row:disabled {
            color: rgba(23, 25, 28, 0.38);
        }

        .okp-track-empty {
            min-height: 32px;
            margin: 0;
            padding: 5px 8px 5px 29px;
            border-radius: 4px;
            border: 1px solid transparent;
            background: transparent;
            color: rgba(23, 25, 28, 0.48);
            font-size: 12px;
        }

        .okp-track-divider {
            margin: 5px 4px;
            background: rgba(0, 0, 0, 0.11);
        }

        .okp-quick-delay-row {
            min-height: 34px;
            margin-top: 2px;
            padding: 2px 4px 0 7px;
        }

        .okp-quick-delay-label {
            color: rgba(23, 25, 28, 0.72);
            font-size: 12px;
        }

        .okp-quick-delay-value {
            color: #0a655f;
            font-size: 11.5px;
            font-weight: 650;
            font-feature-settings: 'tnum';
        }

        button.okp-quick-delay-button {
            min-width: 26px;
            min-height: 26px;
            padding: 0;
            border-radius: 4px;
            border: 1px solid transparent;
            background: transparent;
            box-shadow: none;
        }

        button.okp-quick-delay-button:hover {
            background: rgba(0, 0, 0, 0.06);
        }

        button.okp-quick-delay-button:focus-visible {
            outline: none;
            box-shadow: inset 0 0 0 1px alpha(@okp_teal, 0.72);
        }

        .okp-quick-style-row {
            min-height: 34px;
            padding: 2px 4px 2px 7px;
        }

        .okp-subtitle-preset-status {
            margin: 2px 7px 0;
            color: rgba(23, 25, 28, 0.56);
            font-size: 10.5px;
            line-height: 1.25;
        }

        button.okp-quick-style-button {
            min-height: 26px;
            padding: 2px 8px;
            border-radius: 5px;
            border: 1px solid transparent;
            background: rgba(0, 0, 0, 0.055);
            box-shadow: none;
            color: rgba(23, 25, 28, 0.86);
            font-size: 11.5px;
            font-weight: 600;
        }

        button.okp-quick-style-button:hover {
            background: rgba(0, 0, 0, 0.09);
        }

        button.okp-quick-style-button:disabled {
            background: rgba(0, 0, 0, 0.035);
            color: rgba(23, 25, 28, 0.48);
        }

        .okp-quick-preference-footer {
            margin: 4px -8px -8px;
            padding: 8px 14px;
            border-top: 1px solid rgba(0, 0, 0, 0.08);
            color: rgba(23, 25, 28, 0.48);
            font-size: 10.5px;
        }

        .okp-settings-audio-delay-row {
            margin: 0;
            padding: 10px 0;
            border-top: 1px solid rgba(0, 0, 0, 0.08);
        }

        .okp-settings-audio-delay-row .okp-sub-adjust-label {
            color: rgba(23, 25, 28, 0.72);
            font-size: 12px;
        }

        .okp-settings-audio-delay-row entry.okp-sub-adjust-entry {
            min-width: 74px;
            min-height: 28px;
            padding: 4px 7px;
            border-radius: 6px;
            border: 1px solid rgba(0, 0, 0, 0.16);
            background: #ffffff;
            color: #17191c;
            font-feature-settings: 'tnum';
        }

        .okp-settings-audio-delay-row entry.okp-sub-adjust-entry:focus {
            border-color: alpha(@okp_teal, 0.72);
            box-shadow: 0 0 0 2px alpha(@okp_teal, 0.14);
        }

        .okp-settings-audio-delay-row entry.okp-sub-adjust-entry.is-error {
            border-color: rgba(196, 43, 28, 0.88);
            box-shadow: 0 0 0 2px rgba(196, 43, 28, 0.14);
        }

        .okp-settings-audio-delay-row .okp-sub-adjust-unit {
            color: rgba(23, 25, 28, 0.58);
            font-size: 12px;
        }

        .okp-settings-audio-delay-row .okp-sub-adjust-button {
            min-width: 44px;
            min-height: 28px;
            padding: 4px 7px;
            border-radius: 6px;
            border: 1px solid rgba(0, 0, 0, 0.10);
            background: rgba(0, 0, 0, 0.035);
            color: rgba(23, 25, 28, 0.86);
        }

        .okp-settings-audio-delay-row .okp-sub-adjust-button:hover {
            background: rgba(0, 0, 0, 0.07);
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
            background: #f7f7f5;
            color: @okp_ink;
            border: 1px solid rgba(0, 0, 0, 0.08);
            border-radius: 11px;
            box-shadow: 0 12px 34px rgba(0, 0, 0, 0.28);
        }

        .okp-media-info-header {
            padding: 17px 20px 15px;
            border-bottom: 1px solid rgba(0, 0, 0, 0.07);
        }

        .okp-media-info-identity {
            min-width: 38px;
            min-height: 38px;
            border-radius: 9px;
            background: linear-gradient(150deg, @okp_teal, @okp_teal_deep);
            box-shadow: 0 4px 12px rgba(14, 133, 132, 0.30);
            color: #ffffff;
        }

        .okp-media-info-title {
            color: #161616;
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 17px;
            font-weight: 600;
        }

        .okp-media-info-subtitle,
        .okp-media-info-path {
            color: rgba(0, 0, 0, 0.50);
            font-family: 'Cascadia Code', 'Cascadia Mono', monospace;
            font-size: 12px;
            font-weight: 400;
        }

        button.okp-media-info-close {
            min-width: 30px;
            min-height: 30px;
            padding: 0;
            border: none;
            border-radius: 7px;
            background: rgba(0, 0, 0, 0.04);
            box-shadow: none;
            color: rgba(0, 0, 0, 0.55);
        }

        button.okp-media-info-close:hover {
            background: rgba(0, 0, 0, 0.08);
            color: rgba(0, 0, 0, 0.75);
        }

        .okp-media-info-tabs {
            padding: 13px 20px 0;
        }

        .okp-media-info-tab-strip {
            padding: 3px;
            border-radius: 8px;
            background: rgba(0, 0, 0, 0.05);
        }

        button.okp-media-info-tab {
            min-width: 137px;
            min-height: 30px;
            padding: 0 8px;
            border: none;
            border-radius: 6px;
            background: transparent;
            box-shadow: none;
            color: rgba(0, 0, 0, 0.50);
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 12.5px;
            font-weight: 500;
        }

        button.okp-media-info-tab.is-active {
            background: #ffffff;
            box-shadow: 0 1px 3px rgba(0, 0, 0, 0.10);
            color: @okp_teal_deep;
            font-weight: 600;
        }

        button.okp-media-info-tab:hover:not(.is-active) {
            background: rgba(255, 255, 255, 0.45);
            color: rgba(0, 0, 0, 0.68);
        }

        .okp-media-info-stack,
        .okp-media-info-scroller,
        .okp-media-info-scroller > viewport {
            background: #f7f7f5;
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
            background: rgba(0, 0, 0, 0.22);
        }

        .okp-media-info-card .okp-info-section {
            padding: 13px 16px;
            border-radius: 8px;
            background: #ffffff;
            border: 1px solid rgba(0, 0, 0, 0.06);
        }

        .okp-media-info-card .okp-info-section-title {
            margin-bottom: 11px;
            color: #0c7c75;
            font-size: 11px;
            font-weight: 600;
        }

        .okp-media-info-card .okp-info-row {
            min-height: 18px;
        }

        .okp-media-info-card .okp-info-label {
            color: rgba(0, 0, 0, 0.50);
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 12.5px;
            font-weight: 400;
        }

        .okp-media-info-card .okp-info-value {
            color: #1a1a1a;
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 12.5px;
            font-weight: 500;
            font-feature-settings: 'tnum';
        }

        .okp-media-info-card .okp-info-row.is-highlight .okp-info-value {
            color: @okp_teal_deep;
            font-weight: 600;
        }

        .okp-media-info-card .okp-info-track-row {
            min-height: 42px;
            padding: 9px 11px;
            border-radius: 7px;
            background: transparent;
            border: 1px solid transparent;
        }

        .okp-media-info-card .okp-info-track-row.is-selected {
            background: alpha(@okp_teal, 0.07);
            border-color: alpha(@okp_teal, 0.16);
        }

        .okp-media-info-card .okp-info-track-kind {
            min-width: 34px;
            color: rgba(0, 0, 0, 0.40);
            font-family: 'Cascadia Code', 'Cascadia Mono', monospace;
            font-size: 10px;
            font-weight: 500;
        }

        .okp-media-info-card .okp-info-track-current {
            background: alpha(@okp_teal, 0.14);
            color: #0c7c75;
            font-size: 9px;
        }

        .okp-media-info-empty {
            color: rgba(0, 0, 0, 0.50);
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 12.5px;
        }

        .okp-media-info-footer {
            padding: 12px 20px;
            border-top: 1px solid rgba(0, 0, 0, 0.07);
        }

        .okp-media-info-path {
            color: rgba(0, 0, 0, 0.40);
            font-size: 11px;
        }

        button.okp-media-info-copy,
        button.okp-media-info-done {
            min-height: 32px;
            padding: 0 14px;
            border-radius: 7px;
            box-shadow: none;
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 12px;
            font-weight: 600;
        }

        button.okp-media-info-copy {
            border: 1px solid rgba(0, 0, 0, 0.08);
            background: rgba(0, 0, 0, 0.05);
            color: #1a1a1a;
        }

        button.okp-media-info-copy:hover {
            background: rgba(0, 0, 0, 0.08);
        }

        button.okp-media-info-done {
            min-width: 70px;
            padding: 0 18px;
            border: none;
            background: @okp_teal;
            color: #ffffff;
            box-shadow: 0 3px 10px alpha(@okp_teal, 0.28);
        }

        button.okp-media-info-done:hover {
            background: #0c7c75;
        }

        button.okp-media-info-close:focus-visible,
        button.okp-media-info-tab:focus-visible,
        button.okp-media-info-copy:focus-visible,
        button.okp-media-info-done:focus-visible {
            outline: none;
            box-shadow: 0 0 0 2px alpha(@okp_teal, 0.42);
        }

        window.okp-media-info-window.is-dark .okp-media-info-card,
        window.okp-media-info-window.is-dark .okp-media-info-stack,
        window.okp-media-info-window.is-dark .okp-media-info-scroller,
        window.okp-media-info-window.is-dark .okp-media-info-scroller > viewport {
            background: #17191d;
            color: rgba(255, 255, 255, 0.94);
        }

        window.okp-media-info-window.is-dark .okp-media-info-card {
            border-color: rgba(255, 255, 255, 0.10);
        }

        window.okp-media-info-window.is-dark .okp-media-info-header,
        window.okp-media-info-window.is-dark .okp-media-info-footer {
            border-color: rgba(255, 255, 255, 0.08);
        }

        window.okp-media-info-window.is-dark .okp-media-info-title,
        window.okp-media-info-window.is-dark .okp-info-value,
        window.okp-media-info-window.is-dark button.okp-media-info-copy {
            color: rgba(255, 255, 255, 0.94);
        }

        window.okp-media-info-window.is-dark .okp-media-info-subtitle,
        window.okp-media-info-window.is-dark .okp-media-info-path,
        window.okp-media-info-window.is-dark .okp-info-label,
        window.okp-media-info-window.is-dark .okp-info-track-kind,
        window.okp-media-info-window.is-dark .okp-media-info-empty {
            color: rgba(255, 255, 255, 0.56);
        }

        window.okp-media-info-window.is-dark .okp-media-info-tab-strip,
        window.okp-media-info-window.is-dark button.okp-media-info-close,
        window.okp-media-info-window.is-dark button.okp-media-info-copy {
            background: rgba(255, 255, 255, 0.07);
            border-color: rgba(255, 255, 255, 0.08);
        }

        window.okp-media-info-window.is-dark button.okp-media-info-tab {
            color: rgba(255, 255, 255, 0.58);
        }

        window.okp-media-info-window.is-dark button.okp-media-info-tab.is-active,
        window.okp-media-info-window.is-dark .okp-info-section {
            background: rgba(255, 255, 255, 0.07);
            border-color: rgba(255, 255, 255, 0.08);
            color: rgba(255, 255, 255, 0.94);
        }

        window.okp-media-info-window.is-dark button.okp-media-info-tab.is-active,
        window.okp-media-info-window.is-dark .okp-info-section-title,
        window.okp-media-info-window.is-dark .okp-info-row.is-highlight .okp-info-value,
        window.okp-media-info-window.is-dark .okp-info-track-current {
            color: @okp_accent;
        }

        window.okp-media-info-window.is-dark .okp-media-info-scroller scrollbar slider {
            background: rgba(255, 255, 255, 0.24);
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
            background: #101115;
            color: rgba(255, 255, 255, 0.9);
            border-radius: 8px;
        }

        window.okp-command-dialog > contents {
            background: #101115;
        }

        .okp-command-dialog-title {
            color: rgba(255, 255, 255, 0.96);
            font-size: 16px;
            font-weight: 700;
        }

        window.okp-command-dialog entry {
            min-height: 34px;
            border-radius: 7px;
            border: 1px solid alpha(@okp_accent, 0.42);
            background: rgba(255, 255, 255, 0.055);
            color: rgba(255, 255, 255, 0.92);
            box-shadow: none;
        }

        window.okp-command-dialog button {
            min-width: 72px;
            min-height: 34px;
            padding: 0 14px;
            border-radius: 7px;
            border: 1px solid rgba(255, 255, 255, 0.12);
            background: rgba(255, 255, 255, 0.08);
            color: rgba(255, 255, 255, 0.9);
            box-shadow: none;
        }

        window.okp-command-dialog button:hover {
            background: rgba(255, 255, 255, 0.13);
            color: rgba(255, 255, 255, 0.98);
        }

        window.okp-command-dialog button:active {
            background: alpha(@okp_accent, 0.28);
            border-color: alpha(@okp_accent, 0.48);
        }

        window.okp-command-dialog .okp-info-label {
            color: rgba(255, 255, 255, 0.62);
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 12px;
            font-weight: 500;
        }

        /* Missing-tooling hint (e.g. the YouTube resolver is not installed): a warm,
           attention-drawing tint that stays calm — the state is informational, not an error. */
        window.okp-command-dialog .okp-info-label.okp-info-label-muted {
            color: @okp_warning;
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
        }

        .okp-settings-titlebar {
            min-height: 41px;
            padding: 0 144px 0 16px;
            background: @okp_settings_light;
            border-bottom: 1px solid rgba(0, 0, 0, 0.05);
        }

        .okp-settings-titlebar-label {
            color: rgba(0, 0, 0, 0.70);
            font-family: 'Segoe UI Variable Text', 'Noto Sans', sans-serif;
            font-size: 12.5px;
            font-weight: 600;
        }

        .okp-settings-body {
            background: @okp_settings_light;
        }

        .okp-settings-rail-frame {
            background: rgba(0, 0, 0, 0.015);
        }

        .okp-settings-rail {
            padding: 12px 10px;
            background: rgba(0, 0, 0, 0.015);
            border-right: 1px solid rgba(0, 0, 0, 0.05);
        }

        entry.okp-settings-search {
            min-height: 16px;
            margin-bottom: 6px;
            padding: 7px 10px;
            border-radius: 7px;
            background: #ffffff;
            border: 1px solid rgba(0, 0, 0, 0.09);
            box-shadow: none;
            color: rgba(0, 0, 0, 0.40);
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 12px;
            font-weight: 400;
        }

        entry.okp-settings-search:focus {
            border-color: alpha(@okp_teal, 0.68);
            box-shadow: 0 0 0 1px alpha(@okp_teal, 0.18);
        }

        button.okp-settings-search-result {
            min-height: 40px;
            margin: -2px 0 5px;
            padding: 6px 10px;
            border: none;
            border-radius: 7px;
            background: alpha(@okp_teal, 0.08);
            box-shadow: inset 2px 0 0 alpha(@okp_teal, 0.65);
            color: @okp_ink;
        }

        button.okp-settings-search-result:hover {
            background: alpha(@okp_teal, 0.13);
        }

        .okp-settings-search-result-label {
            color: inherit;
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 11.5px;
            font-weight: 600;
        }

        .okp-settings-search-result-page {
            color: rgba(0, 0, 0, 0.45);
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 10.5px;
            font-weight: 400;
        }

        entry.okp-shortcuts-search {
            min-height: 30px;
            margin-bottom: 2px;
            padding: 6px 10px;
            border-radius: 7px;
            background: #f9fbfc;
            border: 1px solid #d5dce2;
            box-shadow: none;
            color: @okp_ink;
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 12px;
        }

        entry.okp-shortcuts-search:focus {
            border-color: alpha(@okp_teal, 0.68);
            box-shadow: 0 0 0 1px alpha(@okp_teal, 0.18);
        }

        .okp-settings-nav-row {
            min-height: 18px;
            padding: 8px 10px;
            border: none;
            border-radius: 7px;
            background: transparent;
            box-shadow: none;
            color: rgba(0, 0, 0, 0.72);
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 12.5px;
            font-weight: 400;
        }

        .okp-settings-nav-row:hover {
            background: rgba(0, 0, 0, 0.035);
        }

        .okp-settings-nav-row.is-selected {
            background: alpha(@okp_teal, 0.12);
            box-shadow: inset 3px 0 0 @okp_teal;
            color: @okp_teal_deep;
            font-weight: 600;
        }

        .okp-settings-nav-icon {
            min-width: 16px;
            min-height: 16px;
            color: inherit;
        }

        .okp-settings-rail-divider {
            margin: 6px 9px 8px;
            background: rgba(0, 0, 0, 0.05);
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
            color: @okp_ink;
        }

        .okp-settings-window-control:hover {
            background: rgba(0, 0, 0, 0.06);
        }

        .okp-settings-window-control-glyph {
            min-width: 10px;
            min-height: 10px;
            color: @okp_ink;
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
            padding: 28px 44px 28px 24px;
        }

        .okp-settings-content {
            padding-right: 4px;
        }

        .okp-about-pane {
            padding: 28px 44px 28px 24px;
            background: @okp_settings_light;
        }

        .okp-about-identity {
            min-height: 94px;
        }

        .okp-about-illustration {
            min-width: 118px;
            min-height: 94px;
        }

        .okp-about-illustration-art {
            color: @okp_teal;
        }

        .okp-about-wordmark {
            color: @okp_ink;
            font-family: 'Segoe UI Variable Display', 'Segoe UI', sans-serif;
            font-size: 30px;
            letter-spacing: 0;
        }

        .okp-about-wordmark-ok {
            font-weight: 700;
        }

        .okp-about-wordmark-player {
            font-weight: 300;
        }

        .okp-about-chip-row {
            margin-top: 10px;
        }

        .okp-about-version-chip {
            padding: 3px 9px;
            border-radius: 6px;
            background: #e2e8ec;
            color: @okp_ink;
            font-family: 'Cascadia Code', 'Cascadia Mono', monospace;
            font-size: 11.5px;
            font-weight: 600;
            font-feature-settings: 'tnum';
        }

        .okp-about-channel-chip {
            padding: 4px 9px;
            border-radius: 6px;
            background: alpha(@okp_teal, 0.12);
            color: @okp_teal_deep;
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 10px;
            font-weight: 600;
            letter-spacing: 0;
            text-transform: uppercase;
        }

        .okp-about-tagline {
            margin-top: 11px;
            color: rgba(0, 0, 0, 0.50);
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 13px;
            font-weight: 400;
        }

        .okp-about-byline {
            margin-top: 3px;
            color: rgba(0, 0, 0, 0.40);
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 11.5px;
            font-weight: 400;
        }

        .okp-about-identity-divider {
            margin: 22px 0;
            background: rgba(0, 0, 0, 0.07);
        }

        .okp-about-card {
            padding: 14px 16px;
            border-radius: 8px;
            background: #ffffff;
            border: 1px solid rgba(0, 0, 0, 0.06);
        }

        .okp-about-card-title {
            margin-bottom: 13px;
            color: rgba(0, 0, 0, 0.40);
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 11px;
            font-weight: 600;
            letter-spacing: 0;
        }

        .okp-about-row {
            min-height: 14px;
        }

        .okp-about-row-label {
            color: rgba(0, 0, 0, 0.50);
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 12.5px;
            font-weight: 400;
        }

        .okp-about-row-detail {
            color: rgba(0, 0, 0, 0.40);
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 11.5px;
            font-weight: 400;
        }

        .okp-about-row-value,
        .okp-about-row-value-mono {
            color: @okp_ink;
            font-size: 12.5px;
            font-weight: 500;
        }

        .okp-about-row-value {
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
        }

        .okp-about-row-value-mono {
            font-family: 'Cascadia Code', 'Cascadia Mono', monospace;
            font-size: 12px;
            font-feature-settings: 'tnum';
        }

        .okp-about-host-grid {
            min-width: 0;
        }

        .okp-about-tag {
            padding: 2px 6px;
            border-radius: 5px;
            background: rgba(0, 0, 0, 0.05);
            color: rgba(0, 0, 0, 0.40);
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 8.5px;
            font-weight: 600;
            letter-spacing: 0;
        }

        .okp-about-tag.is-accent {
            background: alpha(@okp_teal, 0.12);
            color: @okp_teal_deep;
        }

        .okp-about-footer {
            margin-top: 8px;
            padding-top: 17px;
            border-top: 1px solid rgba(0, 0, 0, 0.07);
        }

        .okp-about-copy-button {
            min-height: 34px;
            padding: 0 14px;
            border-radius: 7px;
            background: #e2e8ec;
            border: 1px solid rgba(0, 0, 0, 0.06);
            box-shadow: none;
            color: @okp_ink;
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 12px;
            font-weight: 600;
        }

        .okp-about-copy-button:hover {
            background: #d9e1e7;
        }

        .okp-about-check-button {
            min-width: 132px;
            min-height: 34px;
            padding: 0 14px;
            border-radius: 7px;
            background: #ffffff;
            border: 1px solid rgba(0, 0, 0, 0.06);
            box-shadow: none;
            color: @okp_ink;
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 12px;
            font-weight: 400;
        }

        .okp-about-check-button:hover {
            background: #f8fafb;
        }

        button.okp-settings-switch {
            min-width: 39px;
            min-height: 22px;
            padding: 3px;
            border: none;
            border-radius: 999px;
            background: #ccd5dc;
            box-shadow: none;
        }

        button.okp-settings-switch.is-active {
            background: @okp_teal;
        }

        button.okp-settings-switch:focus-visible {
            outline: none;
            box-shadow: 0 0 0 2px alpha(@okp_teal, 0.35);
        }

        .okp-settings-switch-knob {
            min-width: 16px;
            min-height: 16px;
            border-radius: 999px;
            background: #ffffff;
        }

        .okp-about-link-button {
            min-height: 24px;
            padding: 0;
            border: none;
            background: transparent;
            box-shadow: none;
            color: @okp_teal_deep;
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 12px;
            font-weight: 600;
        }

        .okp-about-link-arrow {
            color: @okp_teal_deep;
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 12px;
            font-weight: 600;
        }

        .okp-about-link-dot {
            min-width: 3px;
            min-height: 24px;
            color: rgba(0, 0, 0, 0.40);
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 8px;
            font-weight: 600;
        }

        .okp-update-status {
            color: rgba(0, 0, 0, 0.50);
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 12px;
        }

        .okp-mpv-conf-scroller {
            min-height: 132px;
            border-radius: 8px;
            background: #ffffff;
            border: 1px solid rgba(0, 0, 0, 0.06);
        }

        textview.okp-mpv-conf-editor,
        textview.okp-mpv-conf-editor text {
            padding: 10px;
            background: #ffffff;
            color: @okp_ink;
            font-family: 'Cascadia Code', 'Cascadia Mono', monospace;
            font-size: 12px;
            font-weight: 500;
            caret-color: @okp_teal;
        }

        textview.okp-mpv-conf-editor selection,
        textview.okp-mpv-conf-editor text selection {
            background: alpha(@okp_teal, 0.24);
            color: @okp_ink;
        }

        /* Switch rows sit inside an .okp-info-section card, so they use the same
           recessed inset as the track rows instead of a second white card with a
           matching border. That keeps grouped settings from reading as
           card-inside-card clutter. */
        .okp-settings-switch-row {
            min-height: 42px;
            padding: 10px;
            border-radius: 8px;
            background: #f8fafb;
            border: 1px solid rgba(0, 0, 0, 0.05);
        }

        .okp-settings-state-pill {
            min-width: 34px;
            padding: 3px 8px;
            border-radius: 999px;
            background: alpha(@okp_teal, 0.12);
            color: @okp_teal_deep;
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 11px;
            font-weight: 600;
        }

        .okp-integration-state-pill {
            min-width: 74px;
            padding: 4px 8px;
            border-radius: 999px;
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 11px;
            font-weight: 600;
        }

        .okp-integration-state-pill.is-good {
            background: alpha(@okp_teal, 0.12);
            color: @okp_teal_deep;
        }

        .okp-integration-state-pill.is-warning {
            background: alpha(@okp_warning, 0.14);
            color: @okp_warning_deep;
        }

        .okp-integration-state-pill.is-bad {
            background: alpha(@okp_danger, 0.12);
            color: @okp_danger_deep;
        }

        .okp-info-section {
            padding: 14px 16px;
            border-radius: 8px;
            background: #ffffff;
            border: 1px solid rgba(0, 0, 0, 0.06);
        }

        .okp-info-section-title {
            margin-bottom: 10px;
            color: rgba(0, 0, 0, 0.40);
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 11px;
            font-weight: 600;
            letter-spacing: 0;
        }

        .okp-info-row {
            min-height: 22px;
        }

        .okp-info-label {
            color: rgba(0, 0, 0, 0.50);
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 12.5px;
            font-weight: 400;
        }

        .okp-info-value {
            color: @okp_ink;
            font-family: 'Cascadia Code', 'Cascadia Mono', monospace;
            font-size: 12px;
            font-weight: 500;
            font-feature-settings: 'tnum';
        }

        .okp-info-row.is-highlight .okp-info-value {
            color: @okp_teal_deep;
            font-weight: 700;
        }

        .okp-settings-row {
            min-height: 34px;
        }

        .okp-settings-action-row {
            margin-top: 8px;
        }

        .okp-settings-hint {
            margin-top: 6px;
            color: rgba(0, 0, 0, 0.46);
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 11.5px;
            font-weight: 400;
        }

        .okp-shortcuts-list {
            margin-top: 4px;
        }

        .okp-shortcut-row {
            min-height: 44px;
            padding: 7px 0;
            border-bottom: 1px solid rgba(0, 0, 0, 0.06);
        }

        .okp-shortcut-row:last-child {
            border-bottom: none;
        }

        .okp-shortcut-row.is-conflict {
            color: @okp_danger_deep;
        }

        .okp-shortcut-action-title {
            color: @okp_ink;
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 12.5px;
            font-weight: 500;
        }

        .okp-shortcut-action-id {
            color: rgba(0, 0, 0, 0.40);
            font-family: 'Cascadia Code', 'Cascadia Mono', monospace;
            font-size: 10.5px;
            font-feature-settings: 'tnum';
        }

        .okp-shortcut-badge {
            padding: 2px 6px;
            border-radius: 5px;
            background: alpha(@okp_teal, 0.12);
            color: @okp_teal_deep;
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 8.5px;
            font-weight: 600;
        }

        button.okp-shortcut-chip {
            min-width: 82px;
            min-height: 30px;
            padding: 0 10px;
            border-radius: 7px;
            background: #f8fafb;
            border: 1px solid rgba(0, 0, 0, 0.07);
            box-shadow: none;
            color: @okp_ink;
        }

        button.okp-shortcut-chip:hover {
            background: #f1f5f8;
        }

        button.okp-shortcut-chip.is-secondary {
            min-width: 66px;
        }

        button.okp-shortcut-chip.is-empty {
            background: transparent;
            border-color: alpha(@okp_teal, 0.18);
            color: @okp_teal_deep;
        }

        button.okp-shortcut-chip.is-empty:hover {
            background: alpha(@okp_teal, 0.08);
        }

        button.okp-shortcut-chip.is-capturing {
            background: alpha(@okp_teal, 0.12);
            border-color: alpha(@okp_teal, 0.52);
        }

        button.okp-shortcut-chip.is-conflict {
            background: alpha(@okp_danger, 0.10);
            border-color: alpha(@okp_danger, 0.42);
        }

        .okp-shortcut-chip-label {
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 11.5px;
            font-weight: 600;
            font-feature-settings: 'tnum';
        }

        button.okp-shortcut-reset {
            min-width: 52px;
            min-height: 30px;
            padding: 0 10px;
            border-radius: 7px;
            background: transparent;
            border: 1px solid transparent;
            box-shadow: none;
            color: @okp_teal_deep;
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 12px;
            font-weight: 600;
        }

        button.okp-shortcut-reset:hover {
            background: alpha(@okp_teal, 0.08);
        }

        button.okp-shortcut-reset:disabled {
            color: rgba(0, 0, 0, 0.24);
        }

        .okp-settings-scale trough {
            min-height: 6px;
            border-radius: 999px;
            background: rgba(0, 0, 0, 0.13);
        }

        .okp-settings-scale highlight {
            min-height: 6px;
            border-radius: 999px;
            background: @okp_teal;
        }

        .okp-settings-scale slider {
            min-width: 18px;
            min-height: 18px;
            border-radius: 999px;
            background: #ffffff;
            border: 1px solid rgba(0, 0, 0, 0.13);
        }

        .okp-settings-button {
            min-width: 82px;
            min-height: 32px;
            border-radius: 7px;
            background: #ffffff;
            border: 1px solid rgba(0, 0, 0, 0.06);
            box-shadow: none;
            color: @okp_ink;
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 12px;
        }

        .okp-settings-button:hover {
            background: #f8fafb;
        }

        dropdown.okp-history-retention button {
            min-width: 132px;
            min-height: 32px;
            padding: 0 10px;
            border-radius: 7px;
            background: #ffffff;
            border: 1px solid rgba(0, 0, 0, 0.06);
            box-shadow: none;
            color: @okp_ink;
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 12px;
        }

        dropdown.okp-history-retention button:hover {
            background: #f8fafb;
        }

        button.okp-screenshot-format-button {
            min-width: 62px;
        }

        button.okp-screenshot-format-button.is-selected {
            background: alpha(@okp_teal, 0.12);
            border-color: alpha(@okp_teal, 0.38);
            color: @okp_teal_deep;
            font-weight: 650;
        }

        .okp-settings-button:disabled,
        .okp-settings-stepper-button:disabled {
            background: #f2f5f8;
            border-color: rgba(0, 0, 0, 0.04);
            color: rgba(0, 0, 0, 0.32);
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
            border-radius: 8px;
            background: #f4f8fb;
            border: 1px dashed rgba(0, 0, 0, 0.13);
        }

        .okp-empty-state-icon {
            color: rgba(0, 0, 0, 0.34);
            -gtk-icon-size: 14px;
        }

        .okp-empty-state-text {
            color: rgba(0, 0, 0, 0.46);
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 12.5px;
            font-weight: 400;
        }

        button.okp-settings-track-row {
            min-height: 34px;
            padding: 7px 10px;
            border-radius: 7px;
            background: #f8fafb;
            border: 1px solid rgba(0, 0, 0, 0.04);
            box-shadow: none;
            color: @okp_ink;
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 12px;
            font-weight: 400;
        }

        button.okp-settings-track-row:hover {
            background: #f1f5f8;
        }

        button.okp-settings-track-row.is-selected {
            background: alpha(@okp_teal, 0.12);
            border-color: alpha(@okp_teal, 0.24);
            color: @okp_teal_deep;
            font-weight: 600;
        }

        .okp-info-track-row {
            min-height: 44px;
            padding: 8px 9px;
            border-radius: 7px;
            background: #f8fafb;
            border: 1px solid rgba(0, 0, 0, 0.04);
        }

        .okp-info-track-row.is-selected {
            background: alpha(@okp_teal, 0.10);
            border-color: alpha(@okp_teal, 0.18);
        }

        .okp-info-track-kind {
            color: @okp_teal_deep;
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 11px;
            font-weight: 600;
            letter-spacing: 0;
        }

        .okp-info-track-title {
            color: @okp_ink;
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 12.5px;
            font-weight: 600;
        }

        .okp-info-track-current {
            padding: 2px 6px;
            border-radius: 5px;
            background: alpha(@okp_teal, 0.12);
            color: @okp_teal_deep;
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 8.5px;
            font-weight: 600;
            letter-spacing: 0;
        }

        .okp-info-track-detail {
            color: rgba(0, 0, 0, 0.48);
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 11.5px;
        }

        .okp-info-footer-button {
            min-width: 82px;
            min-height: 34px;
            padding: 0 14px;
            border-radius: 7px;
            background: #e2e8ec;
            border: 1px solid rgba(0, 0, 0, 0.06);
            box-shadow: none;
            color: @okp_ink;
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 12px;
            font-weight: 600;
        }

        .okp-info-footer-button:hover {
            background: #d9e1e7;
        }

        .okp-settings-segmented {
            padding: 3px;
            border-radius: 8px;
            background: rgba(0, 0, 0, 0.055);
        }

        button.okp-settings-segment-button {
            min-width: 72px;
            min-height: 30px;
            padding: 0 10px;
            border: none;
            border-radius: 6px;
            background: transparent;
            box-shadow: none;
            color: rgba(0, 0, 0, 0.54);
            font-family: 'Segoe UI Variable Text', 'Noto Sans', sans-serif;
            font-size: 12px;
            font-weight: 600;
        }

        button.okp-settings-segment-button:hover {
            background: rgba(255, 255, 255, 0.52);
        }

        button.okp-settings-segment-button.is-selected {
            background: #ffffff;
            color: @okp_ink;
            box-shadow: 0 1px 3px rgba(0, 0, 0, 0.12);
        }

        button.okp-settings-segment-button.okp-subtitle-style-choice {
            min-width: 52px;
            padding: 0 6px;
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
            color: rgba(255, 255, 255, 0.94);
        }

        window.okp-settings-window.is-dark .okp-settings-titlebar {
            border-bottom-color: rgba(255, 255, 255, 0.06);
        }

        window.okp-settings-window.is-dark .okp-settings-titlebar-label {
            color: rgba(255, 255, 255, 0.74);
        }

        window.okp-settings-window.is-dark .okp-settings-rail-frame,
        window.okp-settings-window.is-dark .okp-settings-rail {
            background: rgba(255, 255, 255, 0.02);
        }

        window.okp-settings-window.is-dark .okp-settings-rail {
            border-right-color: rgba(255, 255, 255, 0.06);
        }

        window.okp-settings-window.is-dark entry.okp-settings-search,
        window.okp-settings-window.is-dark entry.okp-shortcuts-search {
            background: rgba(255, 255, 255, 0.05);
            border-color: rgba(255, 255, 255, 0.09);
            color: rgba(255, 255, 255, 0.74);
        }

        window.okp-settings-window.is-dark button.okp-settings-search-result {
            background: alpha(@okp_accent, 0.10);
            box-shadow: inset 2px 0 0 alpha(@okp_accent, 0.70);
            color: rgba(255, 255, 255, 0.90);
        }

        window.okp-settings-window.is-dark button.okp-settings-search-result:hover {
            background: alpha(@okp_accent, 0.16);
        }

        window.okp-settings-window.is-dark .okp-settings-search-result-page {
            color: rgba(255, 255, 255, 0.48);
        }

        window.okp-settings-window.is-dark .okp-settings-nav-row {
            color: rgba(255, 255, 255, 0.74);
        }

        window.okp-settings-window.is-dark .okp-settings-nav-row:hover {
            background: rgba(255, 255, 255, 0.05);
        }

        window.okp-settings-window.is-dark .okp-settings-nav-row.is-selected {
            background: alpha(@okp_accent, 0.16);
            box-shadow: inset 3px 0 0 @okp_accent;
            color: @okp_accent;
        }

        window.okp-settings-window.is-dark .okp-settings-rail-divider {
            background: rgba(255, 255, 255, 0.06);
        }

        window.okp-settings-window.is-dark .okp-settings-window-control,
        window.okp-settings-window.is-dark .okp-settings-window-control-glyph {
            color: rgba(255, 255, 255, 0.60);
        }

        window.okp-settings-window.is-dark .okp-settings-window-control:hover {
            background: rgba(255, 255, 255, 0.08);
        }

        window.okp-settings-window.is-dark button.okp-settings-window-close:hover {
            background: @okp_danger_dark;
        }

        window.okp-settings-window.is-dark button.okp-settings-window-control:hover .okp-settings-window-control-glyph {
            color: rgba(255, 255, 255, 0.96);
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
            color: rgba(255, 255, 255, 0.94);
        }

        window.okp-settings-window.is-dark .okp-about-tagline,
        window.okp-settings-window.is-dark .okp-about-row-label,
        window.okp-settings-window.is-dark .okp-info-label,
        window.okp-settings-window.is-dark .okp-update-status,
        window.okp-settings-window.is-dark .okp-update-action-detail,
        window.okp-settings-window.is-dark .okp-info-track-detail {
            color: rgba(255, 255, 255, 0.56);
        }

        window.okp-settings-window.is-dark .okp-settings-hint {
            color: rgba(255, 255, 255, 0.50);
        }

        window.okp-settings-window.is-dark .okp-about-byline,
        window.okp-settings-window.is-dark .okp-about-card-title,
        window.okp-settings-window.is-dark .okp-about-row-detail,
        window.okp-settings-window.is-dark .okp-shortcut-action-id,
        window.okp-settings-window.is-dark .okp-info-section-title {
            color: rgba(255, 255, 255, 0.42);
        }

        window.okp-settings-window.is-dark .okp-about-identity-divider {
            background: rgba(255, 255, 255, 0.08);
        }

        window.okp-settings-window.is-dark .okp-about-footer,
        window.okp-settings-window.is-dark .okp-shortcut-row {
            border-color: rgba(255, 255, 255, 0.08);
        }

        window.okp-settings-window.is-dark .okp-about-card,
        window.okp-settings-window.is-dark .okp-info-section,
        window.okp-settings-window.is-dark .okp-update-action-surface {
            background: rgba(255, 255, 255, 0.035);
            border-color: rgba(255, 255, 255, 0.07);
        }

        window.okp-settings-window.is-dark .okp-about-version-chip,
        window.okp-settings-window.is-dark .okp-about-tag,
        window.okp-settings-window.is-dark .okp-about-copy-button {
            background: rgba(255, 255, 255, 0.07);
            border-color: rgba(255, 255, 255, 0.07);
            color: rgba(255, 255, 255, 0.94);
        }

        window.okp-settings-window.is-dark .okp-about-channel-chip,
        window.okp-settings-window.is-dark .okp-about-tag.is-accent,
        window.okp-settings-window.is-dark .okp-settings-state-pill,
        window.okp-settings-window.is-dark .okp-shortcut-badge,
        window.okp-settings-window.is-dark .okp-info-track-current {
            background: alpha(@okp_accent, 0.16);
            color: @okp_accent;
        }

        window.okp-settings-window.is-dark .okp-about-link-button,
        window.okp-settings-window.is-dark .okp-about-link-arrow,
        window.okp-settings-window.is-dark button.okp-shortcut-reset,
        window.okp-settings-window.is-dark .okp-info-row.is-highlight .okp-info-value {
            color: @okp_accent;
        }

        window.okp-settings-window.is-dark .okp-about-link-dot {
            color: rgba(255, 255, 255, 0.42);
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
            background: rgba(255, 255, 255, 0.05);
            border-color: rgba(255, 255, 255, 0.07);
            color: rgba(255, 255, 255, 0.90);
        }

        window.okp-settings-window.is-dark .okp-settings-button:hover,
        window.okp-settings-window.is-dark dropdown.okp-history-retention button:hover,
        window.okp-settings-window.is-dark button.okp-settings-track-row:hover,
        window.okp-settings-window.is-dark button.okp-shortcut-chip:hover,
        window.okp-settings-window.is-dark .okp-about-copy-button:hover {
            background: rgba(255, 255, 255, 0.09);
        }

        scale.okp-settings-scale highlight {
            background: @okp_teal;
        }

        window.okp-settings-window.is-dark scale.okp-settings-scale highlight,
        window.okp-settings-window.is-dark button.okp-settings-switch.is-active {
            background: @okp_accent;
        }

        window.okp-settings-window.is-dark button.okp-settings-switch {
            background: rgba(255, 255, 255, 0.20);
        }

        window.okp-settings-window.is-dark button.okp-settings-switch:focus-visible {
            box-shadow: 0 0 0 2px alpha(@okp_accent, 0.45);
        }

        window.okp-settings-window.is-dark .okp-settings-segmented {
            background: rgba(255, 255, 255, 0.06);
        }

        window.okp-settings-window.is-dark button.okp-settings-segment-button {
            color: rgba(255, 255, 255, 0.58);
        }

        window.okp-settings-window.is-dark button.okp-settings-segment-button:hover {
            background: rgba(255, 255, 255, 0.06);
        }

        window.okp-settings-window.is-dark button.okp-settings-segment-button.is-selected {
            background: rgba(255, 255, 255, 0.12);
            color: rgba(255, 255, 255, 0.94);
            box-shadow: 0 1px 3px rgba(0, 0, 0, 0.35);
        }

        window.okp-settings-window.is-dark scrollbar,
        window.okp-settings-window.is-dark scrollbar trough {
            background: transparent;
            border: none;
        }

        window.okp-settings-window.is-dark scrollbar slider {
            background: rgba(255, 255, 255, 0.24);
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
    use super::OKP_STYLESHEET;
    use std::collections::HashSet;

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
        // The generic adwaita blue must not reappear on any surface: the light
        // settings chrome shares the dark player's single teal brand accent.
        for stray in ["#0067c0", "0, 103, 192"] {
            assert!(
                !OKP_STYLESHEET.contains(stray),
                "stray adwaita-blue literal `{stray}` found; use @okp_teal"
            );
        }

        // Brand accent colours live only in their `@define-color`; every other
        // use goes through a token so the palette retints from one edit.
        for once in ["#28b3aa", "#37cfc5", "#10938a"] {
            let count = OKP_STYLESHEET.matches(once).count();
            assert!(
                count <= 1,
                "accent colour `{once}` should only appear in its @define-color, found {count}"
            );
        }
        for gone in ["40, 179, 170", "16, 147, 138"] {
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
            "font-size: 30px;",
            "font-size: 13.5px;",
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
        assert!(OKP_STYLESHEET.contains("padding: 0 12px;"));
        assert!(OKP_STYLESHEET.contains("padding: 0 16px;"));
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
            "background: rgba(22, 22, 25, 0.50);",
            "background: rgba(22, 22, 25, 0.60);",
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
