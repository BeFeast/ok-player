use super::*;

pub(crate) fn install_css() {
    let Some(display) = gdk::Display::default() else {
        return;
    };

    let provider = gtk::CssProvider::new();
    provider.load_from_data(
        "
        .okp-root {
            background: #050507;
        }

        window.okp-player-window {
            background: #050507;
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
            box-shadow: inset 0 0 0 1px rgba(40, 179, 170, 0.65);
        }

        button.okp-player-window-close:hover {
            background: rgba(219, 59, 59, 0.86);
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
            background: #050507;
        }

        .okp-empty-surface {
            background: rgba(5, 5, 7, 0.94);
        }

        .okp-empty-panel {
            padding: 40px 44px 34px 44px;
            border-radius: 18px;
            border: 1px solid rgba(255, 255, 255, 0.09);
            background: linear-gradient(180deg, rgba(23, 25, 30, 0.94), rgba(13, 14, 18, 0.94));
            box-shadow: 0 30px 80px rgba(0, 0, 0, 0.55);
        }

        .okp-empty-panel.is-drop-target {
            border-color: rgba(40, 179, 170, 0.82);
            background: linear-gradient(180deg, rgba(19, 46, 47, 0.95), rgba(13, 32, 33, 0.95));
            box-shadow: 0 0 0 2px rgba(40, 179, 170, 0.22), 0 30px 80px rgba(0, 0, 0, 0.55);
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
            color: rgba(255, 255, 255, 0.60);
            font-size: 30px;
            font-weight: 300;
        }

        .okp-empty-tagline {
            margin-top: 8px;
            color: rgba(255, 255, 255, 0.52);
            font-size: 13px;
        }

        .okp-empty-actions {
            margin-top: 26px;
        }

        .okp-empty-primary-button,
        .okp-empty-secondary-button {
            min-height: 38px;
            padding: 8px 16px;
            border-radius: 10px;
            border: 1px solid transparent;
            box-shadow: none;
            font-size: 13px;
            font-weight: 650;
        }

        .okp-empty-primary-button {
            background: #28b3aa;
            color: #041110;
        }

        .okp-empty-primary-button:hover {
            background: #37cfc5;
        }

        .okp-empty-primary-button:active {
            background: #229a92;
        }

        .okp-empty-primary-button:focus-visible,
        .okp-empty-secondary-button:focus-visible {
            outline: none;
            box-shadow: 0 0 0 2px rgba(40, 179, 170, 0.55);
        }

        .okp-empty-secondary-button {
            background: rgba(255, 255, 255, 0.06);
            border-color: rgba(255, 255, 255, 0.08);
            color: rgba(255, 255, 255, 0.84);
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
            color: rgba(255, 255, 255, 0.34);
            font-size: 11.5px;
        }

        .okp-controls {
            padding: 8px 10px;
            border-radius: 18px;
            background: rgba(13, 14, 18, 0.86);
            border: 1px solid rgba(255, 255, 255, 0.11);
            box-shadow: 0 18px 48px rgba(0, 0, 0, 0.48);
        }

        .okp-control-group {
            padding: 3px;
            border-radius: 14px;
            background: rgba(255, 255, 255, 0.045);
            border: 1px solid rgba(255, 255, 255, 0.055);
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
            background: rgba(40, 179, 170, 0.24);
            border-color: rgba(40, 179, 170, 0.42);
            color: rgba(255, 255, 255, 0.98);
        }

        button.okp-control-button:disabled,
        menubutton.okp-control-button > button:disabled {
            background: transparent;
            border-color: transparent;
            color: rgba(255, 255, 255, 0.32);
        }

        button.okp-control-button:focus-visible,
        menubutton.okp-control-button > button:focus-visible {
            outline: none;
            box-shadow: 0 0 0 2px rgba(40, 179, 170, 0.55);
        }

        button.okp-play-button {
            min-width: 42px;
            border-radius: 11px;
            background: rgba(40, 179, 170, 0.92);
            color: #ffffff;
        }

        button.okp-play-button:hover {
            background: rgba(55, 207, 197, 0.96);
        }

        button.okp-play-button:disabled {
            background: rgba(255, 255, 255, 0.11);
            color: rgba(255, 255, 255, 0.34);
        }

        button.okp-transport-button {
            min-width: 34px;
        }

        button.okp-chip-button,
        menubutton.okp-chip-button > button {
            min-width: 48px;
            background: rgba(255, 255, 255, 0.055);
        }

        button.okp-icon-button,
        menubutton.okp-icon-button > button {
            min-width: 34px;
            padding: 0;
        }

        menubutton.okp-speed-chip > button {
            min-width: 56px;
            background: rgba(255, 255, 255, 0.08);
            color: rgba(40, 179, 170, 0.98);
            font-feature-settings: 'tnum';
        }

        .okp-control-button.is-selected {
            background: rgba(40, 179, 170, 0.22);
        }

        .okp-time-label {
            min-width: 50px;
            color: rgba(255, 255, 255, 0.84);
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

        .okp-seek {
            min-width: 120px;
        }

        scale.okp-seek trough,
        scale.okp-volume trough {
            min-height: 3px;
            border-radius: 999px;
            background: rgba(255, 255, 255, 0.23);
            border: none;
        }

        scale.okp-seek highlight,
        scale.okp-volume highlight {
            min-height: 3px;
            border-radius: 999px;
            background: #28b3aa;
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
            background: rgba(40, 179, 170, 0.22);
            color: rgba(255, 255, 255, 0.96);
        }

        button.okp-side-panel-tab:focus-visible {
            outline: none;
            box-shadow: inset 0 0 0 1px rgba(40, 179, 170, 0.6);
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

        .okp-up-next-row.is-current .okp-chapter-thumb {
            border-color: rgba(40, 179, 170, 0.55);
        }

        .okp-up-next-row:hover {
            background: rgba(255, 255, 255, 0.08);
        }

        .okp-up-next-row.is-current {
            background: rgba(40, 179, 170, 0.18);
            border-color: rgba(40, 179, 170, 0.32);
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
            background: rgba(40, 179, 170, 0.22);
            border-color: rgba(40, 179, 170, 0.62);
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
            background: #28b3aa;
            color: #041110;
            font-size: 9.5px;
            font-weight: 800;
            letter-spacing: 0;
        }

        .okp-next-badge {
            padding: 1px 7px;
            border-radius: 999px;
            background: rgba(40, 179, 170, 0.18);
            color: rgba(40, 179, 170, 0.98);
            font-size: 9.5px;
            font-weight: 760;
            letter-spacing: 0;
        }

        .okp-up-next-marker {
            color: rgba(40, 179, 170, 0.98);
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
            background: rgba(40, 179, 170, 0.16);
            border-color: rgba(40, 179, 170, 0.30);
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
            box-shadow: inset 0 0 0 1px rgba(40, 179, 170, 0.6);
        }

        .okp-track-check {
            min-width: 14px;
            color: rgba(40, 179, 170, 0.98);
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
            border-color: rgba(40, 179, 170, 0.72);
            box-shadow: 0 0 0 2px rgba(40, 179, 170, 0.16);
        }

        entry.okp-sub-adjust-entry.is-error {
            border-color: rgba(255, 104, 104, 0.88);
            box-shadow: 0 0 0 2px rgba(255, 104, 104, 0.18);
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
            background: #eef4f9;
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
            border: 1px solid rgba(40, 179, 170, 0.42);
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
            background: rgba(40, 179, 170, 0.28);
            border-color: rgba(40, 179, 170, 0.48);
        }

        window.okp-command-dialog .okp-info-label {
            color: rgba(255, 255, 255, 0.62);
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 12px;
            font-weight: 500;
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
            background: #eef4f9;
            color: #161616;
        }

        .okp-settings-root {
            background: #eef4f9;
            color: #161616;
            border: none;
            border-radius: 0;
        }

        .okp-settings-rail-frame {
            background: #eaf0f5;
        }

        .okp-settings-rail {
            padding: 16px 10px 14px 10px;
            background: #eaf0f5;
            border-right: 1px solid #dde3e7;
        }

        .okp-settings-rail-title {
            margin-left: 5px;
            margin-bottom: 20px;
            color: #3b3f42;
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 12.5px;
            font-weight: 400;
        }

        .okp-settings-search {
            min-height: 16px;
            margin-bottom: 11px;
            padding: 7px 10px;
            border-radius: 7px;
            background: #f9fbfc;
            border: 1px solid #d5dce2;
            color: #6c747a;
        }

        .okp-settings-search-label {
            color: #6c747a;
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
            color: #161616;
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 12px;
        }

        entry.okp-shortcuts-search:focus {
            border-color: rgba(0, 103, 192, 0.68);
            box-shadow: 0 0 0 1px rgba(0, 103, 192, 0.18);
        }

        .okp-settings-nav-row {
            min-height: 18px;
            padding: 8px 10px;
            border: none;
            border-radius: 7px;
            background: transparent;
            box-shadow: none;
            color: #3f464b;
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 12.5px;
            font-weight: 400;
        }

        .okp-settings-nav-row:hover {
            background: rgba(0, 0, 0, 0.035);
        }

        .okp-settings-nav-row.is-selected {
            background: #cfe5e8;
            box-shadow: inset 3px 0 0 #10938a;
            color: #0a655f;
            font-weight: 600;
        }

        .okp-settings-nav-icon {
            min-width: 16px;
            min-height: 16px;
            color: inherit;
        }

        .okp-settings-rail-divider {
            margin: 6px 9px 8px;
            background: #dbe2e7;
        }

        .okp-captionless-window-drag-layer {
            min-height: 32px;
            background: transparent;
        }

        .okp-settings-window-controls {
            min-height: 32px;
        }

        .okp-settings-window-control {
            min-width: 48px;
            min-height: 32px;
            padding: 0;
            border: none;
            border-radius: 0;
            background: transparent;
            box-shadow: none;
            color: #161616;
        }

        .okp-settings-window-control:hover {
            background: rgba(0, 0, 0, 0.06);
        }

        .okp-settings-window-control-glyph {
            min-width: 10px;
            min-height: 10px;
            color: #161616;
        }

        button.okp-settings-window-control:hover .okp-settings-window-control-glyph {
            color: #161616;
        }

        button.okp-settings-window-close:hover {
            background: #c42b1c;
        }

        button.okp-settings-window-close:hover .okp-settings-window-control-glyph {
            color: #ffffff;
        }

        .okp-settings-stack {
            background: #eef4f9;
        }

        .okp-settings-scroller {
            background: #eef4f9;
        }

        .okp-settings-page {
            padding: 70px 44px 28px 24px;
        }

        .okp-info-page {
            background: #eef4f9;
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
            color: #161616;
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
            background: #eef4f9;
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
            padding: 70px 44px 28px 24px;
            background: #eef4f9;
        }

        .okp-about-identity {
            min-height: 112px;
        }

        .okp-about-illustration {
            min-width: 118px;
            min-height: 94px;
        }

        .okp-about-wordmark {
            color: #161616;
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
            color: #161616;
            font-family: 'Cascadia Code', 'Cascadia Mono', monospace;
            font-size: 11.5px;
            font-weight: 600;
            font-feature-settings: 'tnum';
        }

        .okp-about-channel-chip {
            padding: 4px 9px;
            border-radius: 6px;
            background: rgba(16, 147, 138, 0.12);
            color: #0a655f;
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
            color: #161616;
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
            background: rgba(16, 147, 138, 0.12);
            color: #0a655f;
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
            color: #161616;
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
            color: #161616;
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
            background: #0067c0;
        }

        .okp-about-toggle-knob {
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
            color: #0a655f;
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 12px;
            font-weight: 600;
        }

        .okp-about-link-arrow {
            color: #0a655f;
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
            color: #161616;
            font-family: 'Cascadia Code', 'Cascadia Mono', monospace;
            font-size: 12px;
            font-weight: 500;
            caret-color: #0067c0;
        }

        textview.okp-mpv-conf-editor selection,
        textview.okp-mpv-conf-editor text selection {
            background: rgba(0, 103, 192, 0.24);
            color: #161616;
        }

        .okp-settings-switch-row {
            min-height: 42px;
            padding: 10px;
            border-radius: 8px;
            background: #ffffff;
            border: 1px solid rgba(0, 0, 0, 0.06);
        }

        .okp-settings-state-pill {
            min-width: 34px;
            padding: 3px 8px;
            border-radius: 999px;
            background: rgba(16, 147, 138, 0.12);
            color: #0a655f;
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
            background: rgba(16, 147, 138, 0.12);
            color: #0a655f;
        }

        .okp-integration-state-pill.is-warning {
            background: rgba(176, 118, 0, 0.14);
            color: #6f4b00;
        }

        .okp-integration-state-pill.is-bad {
            background: rgba(196, 43, 28, 0.12);
            color: #9a1f15;
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
            color: #161616;
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
            color: #161616;
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
            color: #0a655f;
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
            color: #9a1f15;
        }

        .okp-shortcut-action-title {
            color: #161616;
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
            background: rgba(16, 147, 138, 0.12);
            color: #0a655f;
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
            color: #161616;
        }

        button.okp-shortcut-chip:hover {
            background: #f1f5f8;
        }

        button.okp-shortcut-chip.is-secondary {
            min-width: 66px;
        }

        button.okp-shortcut-chip.is-empty {
            background: transparent;
            border-color: rgba(16, 147, 138, 0.18);
            color: #0a655f;
        }

        button.okp-shortcut-chip.is-empty:hover {
            background: rgba(16, 147, 138, 0.08);
        }

        button.okp-shortcut-chip.is-capturing {
            background: rgba(0, 103, 192, 0.12);
            border-color: rgba(0, 103, 192, 0.52);
        }

        button.okp-shortcut-chip.is-conflict {
            background: rgba(196, 43, 28, 0.10);
            border-color: rgba(196, 43, 28, 0.42);
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
            color: #0a655f;
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 12px;
            font-weight: 600;
        }

        button.okp-shortcut-reset:hover {
            background: rgba(16, 147, 138, 0.08);
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
            background: #0067c0;
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
            color: #161616;
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 12px;
        }

        .okp-settings-button:hover {
            background: #f8fafb;
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
            -gtk-icon-size: 15px;
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
            color: #161616;
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 12px;
            font-weight: 400;
        }

        button.okp-settings-track-row:hover {
            background: #f1f5f8;
        }

        button.okp-settings-track-row.is-selected {
            background: rgba(16, 147, 138, 0.12);
            border-color: rgba(16, 147, 138, 0.24);
            color: #0a655f;
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
            background: rgba(16, 147, 138, 0.10);
            border-color: rgba(16, 147, 138, 0.18);
        }

        .okp-info-track-kind {
            color: #0a655f;
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 11px;
            font-weight: 600;
            letter-spacing: 0;
        }

        .okp-info-track-title {
            color: #161616;
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 12.5px;
            font-weight: 600;
        }

        .okp-info-track-current {
            padding: 2px 6px;
            border-radius: 5px;
            background: rgba(16, 147, 138, 0.12);
            color: #0a655f;
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
            color: #161616;
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 12px;
            font-weight: 600;
        }

        .okp-info-footer-button:hover {
            background: #d9e1e7;
        }
        ",
    );
    gtk::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}
