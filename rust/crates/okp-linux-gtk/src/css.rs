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

        window.okp-player-window {
            background: @okp_bg;
        }

        .okp-window-chrome {
            min-height: 40px;
            background: transparent;
        }

        .okp-window-drag-zone {
            min-height: 40px;
            background: transparent;
        }

        .okp-player-window-controls {
            min-height: 32px;
            border-radius: 12px;
            background: rgba(14, 15, 18, 0.42);
            border: 1px solid rgba(255, 255, 255, 0.07);
        }

        .okp-player-window-controls button,
        button.okp-player-window-control {
            min-width: 42px;
            min-height: 32px;
            padding: 0;
            border: none;
            border-radius: 8px;
            background: transparent;
            box-shadow: none;
            color: rgba(255, 255, 255, 0.84);
            font-size: 15px;
            font-weight: 400;
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

        .okp-video-plane {
            background: @okp_bg;
        }

        .okp-empty-surface {
            background: alpha(@okp_bg, 0.97);
        }

        .okp-empty-panel {
            padding: 38px 44px 30px 44px;
            border-radius: 12px;
            border: 1px solid rgba(255, 255, 255, 0.15);
            background: linear-gradient(180deg, rgba(25, 29, 35, 0.98), rgba(14, 17, 22, 0.98));
            box-shadow: 0 24px 64px rgba(0, 0, 0, 0.58);
        }

        .okp-empty-panel.is-drop-target {
            border-color: alpha(@okp_accent, 0.82);
            background: linear-gradient(180deg, rgba(19, 46, 47, 0.95), rgba(13, 32, 33, 0.95));
            box-shadow: 0 0 0 2px alpha(@okp_accent, 0.22), 0 30px 80px rgba(0, 0, 0, 0.55);
        }

        .okp-empty-logo {
            margin-bottom: 2px;
        }

        .okp-empty-wordmark {
            margin-top: 12px;
        }

        .okp-empty-wordmark-ok {
            color: rgba(255, 255, 255, 0.98);
            font-size: 30px;
            font-weight: 800;
        }

        .okp-empty-wordmark-player {
            color: rgba(255, 255, 255, 0.72);
            font-size: 30px;
            font-weight: 300;
        }

        .okp-empty-tagline {
            margin-top: 8px;
            color: rgba(255, 255, 255, 0.72);
            font-size: 13px;
        }

        /* The loading / buffering / error overlay for a network source. Sits over the
         * black video plane while a stream opens or after it fails; never captures
         * pointer events so the transport chrome stays clickable. */
        .okp-media-state-overlay {
            background: transparent;
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

        window.okp-history-window,
        .okp-history-root {
            background: #0b0c0f;
        }

        window.okp-history-window .okp-settings-window-control,
        window.okp-history-window .okp-settings-window-control-glyph {
            color: rgba(255, 255, 255, 0.78);
        }

        window.okp-history-window .okp-settings-window-control:hover {
            background: rgba(255, 255, 255, 0.08);
        }

        window.okp-history-window button.okp-settings-window-control:hover .okp-settings-window-control-glyph {
            color: #ffffff;
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

        .okp-controls {
            padding: 7px 8px;
            border-radius: 12px;
            background: rgba(13, 15, 19, 0.94);
            border: 1px solid rgba(255, 255, 255, 0.16);
            box-shadow: 0 16px 42px rgba(0, 0, 0, 0.52);
        }

        .okp-command-cluster {
            padding: 0;
            background: transparent;
        }

        .okp-transport-group {
            padding: 0;
            border-radius: 12px;
            background: transparent;
        }

        .okp-timeline-group {
            min-height: 36px;
            padding: 0 2px;
        }

        separator.okp-control-separator {
            min-width: 1px;
            margin: 5px 2px;
            background: rgba(255, 255, 255, 0.13);
        }

        button.okp-control-button,
        menubutton.okp-control-button > button {
            min-width: 34px;
            min-height: 32px;
            padding: 0 9px;
            border-radius: 9px;
            border: 1px solid transparent;
            background: transparent;
            box-shadow: none;
            color: rgba(255, 255, 255, 0.86);
            font-size: 12px;
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
            min-width: 42px;
            border-radius: 11px;
            background: alpha(@okp_accent, 0.92);
            color: #ffffff;
        }

        button.okp-play-button:hover {
            background: alpha(@okp_accent_bright, 0.96);
        }

        button.okp-play-button:disabled {
            background: alpha(@okp_accent, 0.28);
            color: rgba(255, 255, 255, 0.68);
        }

        button.okp-transport-button {
            min-width: 34px;
        }

        button.okp-chip-button,
        menubutton.okp-chip-button > button {
            min-width: 48px;
            background: rgba(255, 255, 255, 0.035);
        }

        button.okp-icon-button,
        menubutton.okp-icon-button > button {
            min-width: 34px;
            padding: 0;
        }

        menubutton.okp-speed-chip > button {
            min-width: 56px;
            background: rgba(255, 255, 255, 0.08);
            color: alpha(@okp_accent, 0.98);
            font-feature-settings: 'tnum';
        }

        .okp-control-button.is-selected {
            background: alpha(@okp_accent, 0.22);
        }

        .okp-time-label {
            min-width: 50px;
            color: rgba(255, 255, 255, 0.92);
            font-size: 12px;
            font-feature-settings: 'tnum';
        }

        .okp-status-toast {
            padding: 9px 14px;
            border-radius: 10px;
            background: rgba(14, 15, 18, 0.94);
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

        .okp-seek {
            min-width: 120px;
        }

        scale.okp-seek trough,
        scale.okp-volume trough {
            min-height: 3px;
            border-radius: 999px;
            background: rgba(255, 255, 255, 0.34);
            border: none;
        }

        scale.okp-seek highlight,
        scale.okp-volume highlight {
            min-height: 3px;
            border-radius: 999px;
            background: @okp_accent;
        }

        scale.okp-seek slider,
        scale.okp-volume slider {
            min-width: 13px;
            min-height: 13px;
            margin: -5px;
            border-radius: 999px;
            background: rgba(255, 255, 255, 0.96);
            box-shadow: 0 2px 8px rgba(0, 0, 0, 0.42);
        }

        scale.okp-seek mark indicator {
            min-width: 2px;
            min-height: 7px;
            border-radius: 999px;
            background: rgba(255, 255, 255, 0.42);
        }

        scale.okp-seek marks.top mark indicator {
            background: rgba(255, 255, 255, 0.42);
        }

        scale.okp-seek marks.bottom mark indicator {
            min-width: 3px;
            min-height: 9px;
            background: rgba(233, 176, 74, 0.96);
        }

        scale.okp-seek mark label {
            color: rgba(233, 176, 74, 0.98);
            font-size: 9.5px;
            font-weight: 800;
            font-feature-settings: 'tnum';
        }

        /* Strip the default popover chrome so the seek-hover preview shows only
           its own dark card, matching the normalized track popovers instead of a
           stock light popover frame. */
        popover.okp-seek-popover,
        popover.okp-seek-popover > contents {
            padding: 0;
            background: transparent;
            border: none;
            box-shadow: none;
        }

        popover.okp-seek-popover > arrow {
            min-width: 0;
            min-height: 0;
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

        .okp-volume {
            min-width: 92px;
        }

        .okp-up-next-panel {
            padding: 12px;
            border-radius: 12px;
            background: rgba(12, 13, 17, 0.94);
            border: 1px solid rgba(255, 255, 255, 0.10);
            box-shadow: 0 22px 58px rgba(0, 0, 0, 0.48);
        }

        .okp-side-panel-header {
            padding: 2px 2px 4px 2px;
        }

        .okp-up-next-title {
            color: rgba(255, 255, 255, 0.94);
            font-size: 17px;
            font-weight: 760;
        }

        .okp-up-next-summary {
            color: rgba(255, 255, 255, 0.54);
            font-size: 11.5px;
        }

        .okp-side-panel-tabs {
            margin-top: 6px;
            padding: 3px;
            border-radius: 10px;
            background: rgba(255, 255, 255, 0.055);
            border: 1px solid rgba(255, 255, 255, 0.055);
        }

        button.okp-side-panel-tab {
            min-height: 30px;
            padding: 0 10px;
            border-radius: 8px;
            border: none;
            background: transparent;
            box-shadow: none;
            color: rgba(255, 255, 255, 0.64);
            font-size: 12px;
            font-weight: 650;
        }

        button.okp-side-panel-tab:hover {
            background: rgba(255, 255, 255, 0.07);
            color: rgba(255, 255, 255, 0.86);
        }

        button.okp-side-panel-tab.is-selected {
            background: alpha(@okp_accent, 0.22);
            color: rgba(255, 255, 255, 0.96);
        }

        button.okp-side-panel-tab:focus-visible {
            outline: none;
            box-shadow: inset 0 0 0 1px alpha(@okp_accent, 0.6);
        }

        .okp-up-next-list {
            background: transparent;
        }

        .okp-up-next-list row {
            background: transparent;
        }

        .okp-panel-heading-row {
            padding: 8px 10px 3px 10px;
        }

        .okp-panel-heading {
            color: rgba(255, 255, 255, 0.42);
            font-size: 10.5px;
            font-weight: 720;
        }

        .okp-panel-empty-row {
            min-height: 64px;
            margin: 4px 2px;
            padding: 18px 14px;
            border-radius: 10px;
            background: rgba(255, 255, 255, 0.02);
            border: 1px dashed rgba(255, 255, 255, 0.14);
        }

        .okp-panel-empty {
            color: rgba(255, 255, 255, 0.56);
            font-size: 12.5px;
        }

        .okp-up-next-row {
            min-height: 42px;
            margin: 2px 0;
            padding: 9px 10px;
            border-radius: 9px;
            border: 1px solid transparent;
            background: rgba(255, 255, 255, 0.035);
            color: rgba(255, 255, 255, 0.78);
        }

        .okp-chapter-row {
            min-height: 58px;
        }

        .okp-chapter-thumb {
            min-width: 88px;
            min-height: 50px;
            border-radius: 7px;
            background: rgba(255, 255, 255, 0.08);
            border: 1px solid rgba(255, 255, 255, 0.06);
        }

        .okp-chapter-thumb.is-pending {
            background: rgba(255, 255, 255, 0.045);
            border-style: dashed;
            border-color: rgba(255, 255, 255, 0.09);
        }

        .okp-chapter-thumb-placeholder {
            color: rgba(255, 255, 255, 0.26);
        }

        .okp-bookmark-row .okp-bookmark-icon {
            color: alpha(@okp_accent, 0.92);
        }

        .okp-add-bookmark-row {
            background: transparent;
            border-style: dashed;
            border-color: rgba(255, 255, 255, 0.16);
            color: rgba(255, 255, 255, 0.62);
        }

        .okp-add-bookmark-row:hover {
            background: alpha(@okp_accent, 0.12);
            border-color: alpha(@okp_accent, 0.48);
            color: rgba(255, 255, 255, 0.92);
        }

        .okp-add-bookmark-row .okp-add-bookmark-icon {
            color: alpha(@okp_accent, 0.92);
        }

        .okp-add-files-row {
            background: transparent;
            border-style: dashed;
            border-color: rgba(255, 255, 255, 0.16);
            color: rgba(255, 255, 255, 0.62);
        }

        .okp-add-files-row:hover {
            background: alpha(@okp_accent, 0.12);
            border-color: alpha(@okp_accent, 0.48);
            color: rgba(255, 255, 255, 0.92);
        }

        .okp-add-files-row .okp-add-files-icon {
            color: alpha(@okp_accent, 0.92);
        }

        /* The lone now-playing card at the top of a short queue has no reorder /
           remove controls, so give it a touch more breathing room than a regular
           queue row so it reads as a pinned card rather than a stripped row. */
        .okp-now-playing-pinned-row {
            min-height: 46px;
        }

        .okp-up-next-row.is-current .okp-chapter-thumb {
            border-color: alpha(@okp_accent, 0.55);
        }

        .okp-up-next-row:hover {
            background: rgba(255, 255, 255, 0.08);
        }

        .okp-up-next-row.is-current {
            background: alpha(@okp_accent, 0.18);
            border-color: alpha(@okp_accent, 0.32);
            color: rgba(255, 255, 255, 0.96);
        }

        .okp-up-next-row.is-behind {
            background: transparent;
            color: rgba(255, 255, 255, 0.44);
        }

        .okp-up-next-row.is-behind:hover {
            background: rgba(255, 255, 255, 0.05);
            color: rgba(255, 255, 255, 0.72);
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
            color: rgba(255, 255, 255, 0.28);
        }

        .okp-up-next-drag-handle-icon {
            -gtk-icon-size: 16px;
        }

        .okp-up-next-row:hover .okp-up-next-drag-handle,
        .okp-up-next-row.is-drop-target .okp-up-next-drag-handle {
            color: rgba(255, 255, 255, 0.78);
        }

        .okp-up-next-lane {
            min-width: 46px;
        }

        .okp-up-next-index {
            min-width: 22px;
            color: rgba(255, 255, 255, 0.40);
            font-size: 11px;
            font-weight: 620;
            font-feature-settings: 'tnum';
        }

        .okp-up-next-source-icon {
            color: rgba(255, 255, 255, 0.50);
        }

        .okp-up-next-row.is-current .okp-up-next-source-icon {
            color: rgba(255, 255, 255, 0.82);
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
            color: alpha(@okp_accent, 0.98);
            font-size: 11px;
            font-weight: 760;
            font-feature-settings: 'tnum';
        }

        .okp-up-next-row.is-current .okp-up-next-marker {
            color: rgba(255, 255, 255, 0.90);
        }

        .okp-up-next-file {
            color: inherit;
            font-size: 13px;
        }

        .okp-up-next-actions {
            min-width: 104px;
        }

        button.okp-up-next-action-button {
            min-width: 24px;
            min-height: 24px;
            padding: 0;
            border: none;
            border-radius: 5px;
            background: transparent;
            box-shadow: none;
            color: rgba(255, 255, 255, 0.58);
        }

        button.okp-up-next-action-button:hover {
            background: rgba(255, 255, 255, 0.10);
            color: rgba(255, 255, 255, 0.90);
        }

        button.okp-up-next-action-button:disabled {
            color: rgba(255, 255, 255, 0.18);
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
            background: rgba(255, 255, 255, 0.22);
        }

        .okp-track-popover-content {
            padding: 10px;
            background: #121317;
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
            border-radius: 10px;
            background: #121317;
            border: 1px solid rgba(255, 255, 255, 0.12);
            box-shadow: 0 18px 46px rgba(0, 0, 0, 0.46);
        }

        popover.okp-track-popover arrow {
            min-width: 0;
            min-height: 0;
            background: transparent;
            border: none;
            box-shadow: none;
        }

        .okp-track-popover-scroll {
            background: #121317;
        }

        .okp-track-popover-title {
            margin: 0 4px 8px 4px;
            color: rgba(255, 255, 255, 0.94);
            font-size: 13.5px;
            font-weight: 760;
        }

        .okp-track-group-title {
            margin: 10px 4px 4px 4px;
            color: rgba(255, 255, 255, 0.42);
            font-size: 10.5px;
            font-weight: 720;
        }

        .okp-track-subgroup-title {
            margin: 6px 4px 2px 4px;
            color: rgba(255, 255, 255, 0.34);
            font-size: 10px;
            font-weight: 640;
        }

        button.okp-track-row {
            min-height: 34px;
            padding: 6px 9px;
            border-radius: 7px;
            background: transparent;
            border: 1px solid transparent;
            box-shadow: none;
            color: rgba(255, 255, 255, 0.82);
        }

        button.okp-track-row:hover {
            background: rgba(255, 255, 255, 0.08);
            color: rgba(255, 255, 255, 0.96);
        }

        button.okp-track-row:active {
            background: rgba(255, 255, 255, 0.12);
        }

        button.okp-track-row.is-selected {
            background: alpha(@okp_accent, 0.16);
            border-color: alpha(@okp_accent, 0.30);
            color: rgba(255, 255, 255, 0.98);
        }

        button.okp-track-row.is-selected .okp-track-row-label {
            font-weight: 640;
        }

        button.okp-track-row:disabled {
            background: transparent;
            border-color: transparent;
            color: rgba(255, 255, 255, 0.30);
        }

        button.okp-track-row:focus-visible {
            outline: none;
            box-shadow: inset 0 0 0 1px alpha(@okp_accent, 0.6);
        }

        .okp-track-check {
            min-width: 14px;
            color: alpha(@okp_accent, 0.98);
        }

        button.okp-track-row:disabled .okp-track-check {
            color: rgba(255, 255, 255, 0.30);
        }

        .okp-track-empty {
            margin: 4px 6px 6px 6px;
            padding: 12px 12px;
            border-radius: 8px;
            border: 1px dashed rgba(255, 255, 255, 0.14);
            background: rgba(255, 255, 255, 0.02);
            color: rgba(255, 255, 255, 0.50);
            font-size: 12.5px;
        }

        .okp-track-divider {
            margin: 6px 3px;
            background: rgba(255, 255, 255, 0.07);
        }

        .okp-sub-adjust-row {
            margin: 0 2px;
        }

        .okp-sub-adjust-label {
            color: rgba(255, 255, 255, 0.62);
            font-size: 12px;
        }

        .okp-sub-adjust-value {
            color: rgba(255, 255, 255, 0.9);
            font-size: 12px;
            font-feature-settings: 'tnum';
        }

        entry.okp-sub-adjust-entry {
            min-width: 74px;
            min-height: 28px;
            padding: 4px 7px;
            border-radius: 6px;
            border: 1px solid rgba(255, 255, 255, 0.14);
            background: rgba(255, 255, 255, 0.08);
            color: rgba(255, 255, 255, 0.9);
            font-feature-settings: 'tnum';
        }

        entry.okp-sub-adjust-entry:focus {
            border-color: alpha(@okp_accent, 0.72);
            box-shadow: 0 0 0 2px alpha(@okp_accent, 0.16);
        }

        entry.okp-sub-adjust-entry.is-error {
            border-color: alpha(@okp_danger_bright, 0.88);
            box-shadow: 0 0 0 2px alpha(@okp_danger_bright, 0.18);
        }

        .okp-sub-adjust-unit {
            color: rgba(255, 255, 255, 0.58);
            font-size: 12px;
        }

        .okp-sub-adjust-button {
            min-width: 44px;
            min-height: 28px;
            padding: 4px 7px;
            border-radius: 6px;
            background: rgba(255, 255, 255, 0.08);
            color: rgba(255, 255, 255, 0.86);
        }

        .okp-sub-adjust-button:hover {
            background: rgba(255, 255, 255, 0.13);
        }

        .okp-info-window {
            background: @okp_light_bg;
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

        /* The copyable summary on the URL load-failure dialog. Kept dimmer than the
         * title and monospace-friendly so the URL stays readable and selectable. */
        .okp-load-failure-detail {
            color: rgba(255, 255, 255, 0.78);
            font-size: 13px;
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

        window.okp-settings-window > contents,
        window.okp-info-window > contents {
            background: transparent;
            box-shadow: none;
            border: none;
        }

        window.okp-settings-window headerbar,
        window.okp-info-window headerbar,
        window.okp-settings-window decoration,
        window.okp-info-window decoration {
            min-height: 0;
            margin: 0;
            padding: 0;
            border: none;
            background: transparent;
            box-shadow: none;
        }

        .okp-info-root {
            background: @okp_light_bg;
            color: @okp_ink;
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

        .okp-settings-search {
            min-height: 16px;
            margin-bottom: 6px;
            padding: 7px 10px;
            border-radius: 7px;
            background: #ffffff;
            border: 1px solid rgba(0, 0, 0, 0.09);
            color: rgba(0, 0, 0, 0.40);
        }

        .okp-settings-search-label {
            color: rgba(0, 0, 0, 0.40);
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 12px;
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

        .okp-info-page {
            background: @okp_light_bg;
        }

        .okp-info-hero {
            min-height: 82px;
        }

        .okp-info-eyebrow {
            color: rgba(0, 0, 0, 0.40);
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 11px;
            font-weight: 600;
            letter-spacing: 0;
        }

        .okp-info-title {
            color: @okp_ink;
            font-family: 'Segoe UI Variable Display', 'Segoe UI', sans-serif;
            font-size: 28px;
            font-weight: 650;
        }

        .okp-info-path {
            color: rgba(0, 0, 0, 0.46);
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 12px;
        }

        .okp-info-content {
            padding-right: 4px;
        }

        window.okp-info-window scrolledwindow {
            background: @okp_light_bg;
        }

        window.okp-info-window scrollbar {
            background: transparent;
            border: none;
        }

        window.okp-info-window scrollbar trough {
            background: transparent;
            border: none;
        }

        window.okp-info-window scrollbar slider {
            min-width: 4px;
            border-radius: 999px;
            background: rgba(0, 0, 0, 0.22);
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

        button.okp-about-toggle {
            min-width: 39px;
            min-height: 22px;
            padding: 3px;
            border: none;
            border-radius: 999px;
            background: #ccd5dc;
            box-shadow: none;
        }

        button.okp-about-toggle.is-active {
            background: @okp_teal;
        }

        .okp-about-toggle-knob {
            min-width: 16px;
            min-height: 16px;
            border-radius: 999px;
            background: #ffffff;
        }

        /* The single stock GtkSwitch (hardware decode) is retinted so it lights
           up in OK teal instead of the host theme's accent, matching the brand
           toggle above. Only the track/knob colours are overridden; GTK keeps
           its own switch geometry and the state-set handler is untouched. */
        switch.okp-settings-switch {
            border: none;
            background: #ccd5dc;
            box-shadow: none;
        }

        switch.okp-settings-switch:checked {
            background: @okp_teal;
        }

        switch.okp-settings-switch > slider {
            background: #ffffff;
            border: none;
            box-shadow: none;
            outline: none;
        }

        /* Strip the native focus ring only for pointer/programmatic focus;
           keyboard focus keeps a visible marker via the :focus-visible rule
           below, so tabbing to the switch never leaves it unmarked. */
        switch.okp-settings-switch:focus:not(:focus-visible),
        switch.okp-settings-switch:focus:not(:focus-visible) > slider {
            outline: none;
        }

        switch.okp-settings-switch:focus-visible {
            outline: none;
            box-shadow: 0 0 0 2px alpha(@okp_teal, 0.35);
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

        .okp-info-summary {
            padding: 0;
        }

        .okp-info-chip {
            min-width: 78px;
            padding: 8px 10px;
            border-radius: 8px;
            background: #ffffff;
            border: 1px solid rgba(0, 0, 0, 0.06);
        }

        .okp-info-chip-label {
            color: rgba(0, 0, 0, 0.40);
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 10px;
            font-weight: 600;
            letter-spacing: 0;
        }

        .okp-info-chip-value {
            color: @okp_ink;
            font-family: 'Cascadia Code', 'Cascadia Mono', monospace;
            font-size: 12px;
            font-weight: 600;
            font-feature-settings: 'tnum';
        }

        flowbox.okp-info-summary {
            padding: 0;
        }

        .okp-info-summary flowboxchild {
            min-height: 0;
            padding: 0;
            border-radius: 8px;
            background: transparent;
        }

        .okp-info-summary flowboxchild:selected,
        .okp-info-summary flowboxchild:focus {
            background: transparent;
            box-shadow: none;
            outline: none;
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

        window.okp-settings-window.is-dark .okp-settings-search,
        window.okp-settings-window.is-dark entry.okp-shortcuts-search {
            background: rgba(255, 255, 255, 0.05);
            border-color: rgba(255, 255, 255, 0.09);
            color: rgba(255, 255, 255, 0.74);
        }

        window.okp-settings-window.is-dark .okp-settings-search-label {
            color: rgba(255, 255, 255, 0.42);
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
        window.okp-settings-window.is-dark .okp-shortcut-action-title,
        window.okp-settings-window.is-dark .okp-info-chip-value {
            color: rgba(255, 255, 255, 0.94);
        }

        window.okp-settings-window.is-dark .okp-about-tagline,
        window.okp-settings-window.is-dark .okp-about-row-label,
        window.okp-settings-window.is-dark .okp-info-label,
        window.okp-settings-window.is-dark .okp-update-status,
        window.okp-settings-window.is-dark .okp-info-track-detail {
            color: rgba(255, 255, 255, 0.56);
        }

        window.okp-settings-window.is-dark .okp-about-byline,
        window.okp-settings-window.is-dark .okp-about-card-title,
        window.okp-settings-window.is-dark .okp-about-row-detail,
        window.okp-settings-window.is-dark .okp-shortcut-action-id,
        window.okp-settings-window.is-dark .okp-info-section-title,
        window.okp-settings-window.is-dark .okp-info-chip-label {
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
        window.okp-settings-window.is-dark .okp-info-chip {
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
        window.okp-settings-window.is-dark .okp-settings-button {
            background: rgba(255, 255, 255, 0.05);
            border-color: rgba(255, 255, 255, 0.07);
            color: rgba(255, 255, 255, 0.90);
        }

        window.okp-settings-window.is-dark .okp-settings-button:hover,
        window.okp-settings-window.is-dark button.okp-settings-track-row:hover,
        window.okp-settings-window.is-dark button.okp-shortcut-chip:hover,
        window.okp-settings-window.is-dark .okp-about-copy-button:hover {
            background: rgba(255, 255, 255, 0.09);
        }

        scale.okp-settings-scale highlight {
            background: @okp_teal;
        }

        window.okp-settings-window.is-dark scale.okp-settings-scale highlight,
        window.okp-settings-window.is-dark switch.okp-settings-switch:checked,
        window.okp-settings-window.is-dark button.okp-about-toggle.is-active {
            background: @okp_accent;
        }

        window.okp-settings-window.is-dark switch.okp-settings-switch,
        window.okp-settings-window.is-dark button.okp-about-toggle {
            background: rgba(255, 255, 255, 0.20);
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
        window.okp-settings-window.is-high-contrast .okp-settings-button {
            background: #000000;
            border-color: #ffffff;
            color: #ffffff;
        }

        window.okp-settings-window.is-high-contrast .okp-settings-titlebar-label,
        window.okp-settings-window.is-high-contrast .okp-settings-search-label,
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
        window.okp-settings-window.is-high-contrast .okp-shortcut-action-title,
        window.okp-settings-window.is-high-contrast .okp-shortcut-action-id {
            color: #ffffff;
        }

        window.okp-settings-window.is-high-contrast .okp-settings-search,
        window.okp-settings-window.is-high-contrast entry.okp-shortcuts-search,
        window.okp-settings-window.is-high-contrast button.okp-shortcut-chip,
        window.okp-settings-window.is-high-contrast textview.okp-mpv-conf-editor,
        window.okp-settings-window.is-high-contrast textview.okp-mpv-conf-editor text {
            background: #000000;
            border-color: #ffffff;
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
    fn stylesheet_braces_are_balanced() {
        assert_eq!(
            OKP_STYLESHEET.matches('{').count(),
            OKP_STYLESHEET.matches('}').count(),
            "unbalanced CSS braces in the stylesheet"
        );
    }
}
