use super::*;
use okp_test_fixtures::unique_temp_dir;

fn local_item(path: &str) -> PlaylistItem {
    PlaylistItem::Local(PathBuf::from(path))
}

fn url_item(url: &str) -> PlaylistItem {
    PlaylistItem::Url(url.to_owned())
}

fn write_jpeg_header(path: &Path) {
    fs::write(path, b"\xff\xd8\xffok-player").expect("test jpeg header should be written");
}

fn write_png_header(path: &Path) {
    fs::write(path, b"\x89PNG\r\n\x1a\nokp!").expect("test png header should be written");
}

#[test]
fn about_display_version_keeps_about_layout_compact() {
    assert_eq!(about_display_version("0.1.0-linux-alpha.77"), "0.1.0");
    assert_eq!(about_display_version("1.0.0"), "1.0.0");
}

#[test]
fn linux_gapless_setting_is_honestly_deferred() {
    assert_eq!(
        LINUX_GAPLESS_CAPABILITY,
        GaplessPlaybackCapability::Deferred
    );

    let setting = gapless_setting_state(true, LINUX_GAPLESS_CAPABILITY);
    assert!(!setting.enabled);
    assert!(!setting.can_toggle);
    assert_eq!(setting.state_label, "Deferred");
    assert_eq!(setting.action_label, "Unavailable");
    assert!(setting.detail.contains("after end-of-file"));
}

#[test]
fn available_gapless_setting_reflects_the_persisted_preference() {
    let off = gapless_setting_state(false, GaplessPlaybackCapability::Available);
    assert!(!off.enabled);
    assert!(off.can_toggle);
    assert_eq!(off.state_label, "Off");
    assert_eq!(off.action_label, "Turn on");

    let on = gapless_setting_state(true, GaplessPlaybackCapability::Available);
    assert!(on.enabled);
    assert_eq!(on.state_label, "On");
    assert_eq!(on.action_label, "Turn off");
}

#[test]
fn floating_volume_control_keeps_a_fixed_osc_footprint() {
    assert_eq!(VOLUME_RESTING_SIZE, 34);
    assert_eq!(VOLUME_WICK_WIDTH, 18);
    assert_eq!(VOLUME_WICK_HEIGHT, 3);
    assert_eq!(VOLUME_TRACK_WIDTH, 122);
    assert_eq!(VOLUME_TRACK_HEIGHT, 6);
    assert_eq!(VOLUME_THUMB_SIZE, 14);
    assert_eq!(VOLUME_CAPSULE_OFFSET, 10);
    assert_eq!(VOLUME_HOVER_GRACE_MS, 220);
    assert_eq!(VOLUME_COLLAPSE_MS, 120);
}

#[test]
fn timeline_layers_share_one_vertical_center_at_normal_and_live_widths() {
    for width in [300, 900] {
        let geometry = timeline_geometry(width, TIMELINE_HEIGHT, 0.25, 0.70);
        assert_eq!(geometry.trough.y, geometry.buffered.y);
        assert_eq!(geometry.trough.y, geometry.played.y);
        assert_eq!(geometry.trough.height, geometry.buffered.height);
        assert_eq!(geometry.trough.height, geometry.played.height);
        assert_eq!(geometry.trough.center_y(), geometry.buffered.center_y());
        assert_eq!(geometry.trough.center_y(), geometry.played.center_y());
        assert_eq!(geometry.trough.center_y(), geometry.thumb_center_y);
        assert_eq!(geometry.trough.height, TIMELINE_RAIL_HEIGHT);
    }
}

#[test]
fn floating_volume_css_pins_capsule_geometry_motion_and_state_colors() {
    let css = include_str!("css.rs");
    for required in [
        "padding: 9px 13px;",
        "border-radius: 13px;",
        "background: rgba(28, 28, 32, 0.72);",
        "min-width: 122px;",
        "min-height: 14px;",
        "transition: opacity 150ms cubic-bezier(0.25, 0.1, 0.25, 1), transform 150ms cubic-bezier(0.25, 0.1, 0.25, 1);",
        "transition: opacity 120ms cubic-bezier(0.25, 0.1, 0.25, 1), transform 120ms cubic-bezier(0.25, 0.1, 0.25, 1);",
        "#F0B840",
    ] {
        assert!(css.contains(required), "missing volume CSS: {required}");
    }

    let controls = include_str!("controls.rs");
    assert!(controls.contains("(0.157, 0.702, 0.667)")); // #28B3AA
    assert!(controls.contains("VolumeState::unity_fraction()"));
}

#[test]
fn mute_is_a_bindable_m_shortcut() {
    assert_eq!(ShortcutAction::Mute.id(), "mute");
    assert_eq!(ShortcutAction::Mute.default_shortcut(), "M");
    assert!(
        shortcuts::default_bindings().iter().any(|binding| {
            binding.action == ShortcutAction::Mute && binding.chord.label() == "M"
        })
    );
}

#[test]
fn volume_wheel_and_arrow_events_map_to_the_canonical_steps() {
    assert_eq!(volume_scroll_delta(-1.0, false), Some(1.0));
    assert_eq!(volume_scroll_delta(1.0, false), Some(-1.0));
    assert_eq!(volume_scroll_delta(-1.0, true), Some(0.1));
    assert_eq!(volume_scroll_delta(1.0, true), Some(-0.1));
    assert_eq!(volume_scroll_delta(0.0, false), None);
    assert_eq!(volume_key_delta(gdk::Key::Up), Some(1.0));
    assert_eq!(volume_key_delta(gdk::Key::Right), Some(1.0));
    assert_eq!(volume_key_delta(gdk::Key::Down), Some(-1.0));
    assert_eq!(volume_key_delta(gdk::Key::Left), Some(-1.0));
    assert_eq!(volume_key_delta(gdk::Key::space), None);
}

#[test]
fn about_channel_separates_linux_release_track_from_version() {
    assert_eq!(about_display_channel("0.1.0-linux-alpha.77"), "Linux alpha");
    assert_eq!(about_hero_channel("Linux alpha"), "ALPHA");
    assert_eq!(about_display_channel("1.0.0-linux"), "Linux");
    assert_eq!(about_hero_channel("Linux"), "LINUX");
}

#[test]
fn launcher_identity_resolves_without_replacing_the_about_illustration() {
    // Welcome, MPRIS, and desktop packaging share the launcher mark. About is
    // deliberately bespoke: five graduated frame ticks resolve into playback.
    let path = app_icon_path().expect("shared app icon should resolve");
    assert!(path.is_file(), "resolved icon path should exist: {path:?}");
    assert_eq!(
        path.file_name().and_then(|name| name.to_str()),
        Some("com.befeast.okplayer.svg")
    );
    assert_eq!(ABOUT_FRAME_TICKS.len(), 5);
    assert_eq!(ABOUT_FRAME_TICK_OPACITY, [0.10, 0.18, 0.30, 0.44, 0.62]);
}

#[test]
fn linux_identity_preserves_launcher_tile_and_separate_about_illustration() {
    let packaging = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packaging/linux");
    let icon = fs::read_to_string(packaging.join("com.befeast.okplayer.svg"))
        .expect("shared app icon should be readable");
    assert!(icon.contains("<rect"));
    assert!(icon.contains("#15a89d"));
    assert!(icon.contains("cx=\"46\" cy=\"48\" r=\"33\""));
    assert!(icon.contains("x=\"92\" y=\"12\" width=\"15\" height=\"72\" rx=\"4\""));
    assert!(icon.contains("M111 14 L111 82 L161 48 Z"));
    assert!(!icon.contains("<text"));

    let fixed_icons = packaging.join("icons/hicolor");
    for (size, expected_path, expected_transform, forbidden_shape) in [
        (
            64,
            "M111 13 L111 83 L162 48 Z",
            "translate(11 20.5454545) scale(0.2386363636)",
            "M9 7 L9 17",
        ),
        (
            48,
            "M111 12 L111 84 L163 48 Z",
            "translate(8.4166667 15.5) scale(0.1770833333)",
            "M9 7 L9 17",
        ),
        (
            32,
            "M111 11 L111 85 L164 48 Z",
            "translate(5.9166667 10.5) scale(0.1145833333)",
            "M9 7 L9 17",
        ),
        (
            24,
            "M9 7 L9 17 L17 12 Z",
            "translate(4.5 4.5) scale(0.625)",
            "<circle",
        ),
        (
            16,
            "M9 7 L9 17 L16 12 Z",
            "translate(3 3) scale(0.4166666667)",
            "<circle",
        ),
    ] {
        let icon = fs::read_to_string(
            fixed_icons
                .join(format!("{size}x{size}/apps"))
                .join("com.befeast.okplayer.svg"),
        )
        .expect("fixed-size app icon should be readable");
        assert!(icon.contains(expected_path), "wrong {size}px icon variant");
        assert!(
            icon.contains(expected_transform),
            "wrong {size}px optical fit"
        );
        assert!(
            !icon.contains(forbidden_shape),
            "forbidden geometry in {size}px icon"
        );
        assert!(!icon.contains("<text"), "{size}px icon uses a font glyph");
    }
    assert_eq!(ABOUT_FRAME_TICKS.len(), 5);
    assert_eq!(ABOUT_FRAME_TICK_OPACITY, [0.10, 0.18, 0.30, 0.44, 0.62]);
}

#[test]
fn linux_packaging_installs_launcher_identity_asset() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let desktop =
        fs::read_to_string(root.join("rust/packaging/linux/com.befeast.okplayer.desktop"))
            .expect("desktop entry should be readable");
    assert!(
        desktop
            .lines()
            .any(|line| line == "Icon=com.befeast.okplayer")
    );

    for script in ["package-linux-deb.sh", "package-linux-velopack.sh"] {
        let contents = fs::read_to_string(root.join("scripts").join(script))
            .expect("packaging script should be readable");
        assert!(contents.contains("com.befeast.okplayer.svg"));
        assert!(contents.contains("for size in 16 24 32 48 64"));
        assert!(contents.contains("usr/share/icons/hicolor/${size}x${size}/apps"));
    }
}

#[test]
fn gtk_identity_uses_vector_widgets_instead_of_host_font_marks() {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let history = fs::read_to_string(source.join("history_view.rs"))
        .expect("history view source should be readable");
    let window =
        fs::read_to_string(source.join("window.rs")).expect("window source should be readable");
    let branding =
        fs::read_to_string(source.join("branding.rs")).expect("branding source should be readable");

    assert!(history.contains("launcher_brand_tile(48"));
    assert!(history.contains("canonical_brand_mark(20, 11"));
    assert!(!history.contains("gtk::Label::new(Some(\"▶\"))"));
    assert!(!history.contains("gtk::Label::new(Some(\"OK\"))"));
    assert!(window.contains("canonical_brand_mark(20, 11"));
    assert!(!window.contains("media-playback-start-symbolic"));
    assert!(branding.contains("full_mark_for_icon_size"));
    assert!(branding.contains("CANONICAL_FULL_MARK"));
    assert!(branding.contains("draw_launcher_brand_tile"));
    assert!(branding.contains("cairo::LinearGradient::new"));
    assert!(branding.contains("gdk::MemoryTexture::new"));
    assert!(branding.contains("gtk::Picture::for_paintable"));
    assert!(window.contains("apply_gtk_theme_preview();"));
}

#[test]
fn volume_and_audio_track_actions_use_distinct_icon_identities() {
    let controls = include_str!("controls.rs");

    // Volume keeps the speaker/level glyph in both resting and reactive states.
    assert!(
        controls.contains("gtk::Image::from_icon_name(\"audio-volume-high-symbolic\")"),
        "volume control must keep the speaker/level icon"
    );

    // The audio-track/output action reads as a distinct semantic (headphones),
    // never a second speaker glyph, so the two OSC buttons stay tellable apart.
    assert!(
        controls.contains(".icon_name(\"audio-headphones-symbolic\")"),
        "audio-track/output action must use the distinct headphones glyph"
    );
    assert!(
        !controls.contains("audio-speakers-symbolic"),
        "regression guard: no second speaker glyph on the audio action"
    );

    // Distinct accessible names and tooltips: Volume versus Audio tracks / output.
    assert!(controls.contains("gtk::accessible::Property::Label(\"Volume\")"));
    assert!(controls.contains("set_tooltip_text(Some(\"Audio tracks / output\"))"));
    assert!(controls.contains("gtk::accessible::Property::Label(\"Audio tracks / output\")"));
}

#[test]
fn settings_stays_in_the_width_safe_bottom_more_menu() {
    let window = include_str!("window.rs");
    let controls = include_str!("controls.rs");
    let popovers = include_str!("track_popovers.rs");

    assert!(!window.contains("gtk::Button::from_icon_name(\"emblem-system-symbolic\")"));
    assert!(window.contains("persistent_widgets: Vec::new()"));
    assert!(controls.contains("more_slot.set_size_request(32, 1)"));
    assert!(controls.contains("chrome.add_overlay(&controls.more_button)"));
    assert!(popovers.contains("command_button(\"Settings...\", false)"));
    assert_eq!(
        window
            .matches("add_css_class(\"okp-top-chrome-motion\")")
            .count(),
        4
    );
    assert!(window.contains("transient_controls.append(&pin);"));
    assert!(
        window.contains(
            "scrim.upcast(),\n            title_content.upcast(),\n            transient_controls.upcast(),"
        )
    );
    assert!(!window.contains("chrome.add_linked_motion_widget(window_chrome.widget())"));

    let chrome = include_str!("main.rs");
    assert!(chrome.contains("widget.set_sensitive(revealed);"));

    let css = include_str!("css.rs");
    assert!(!css.contains("button.okp-player-settings-control.is-isolated"));
}

#[test]
fn settings_shell_matches_windows_reference_geometry() {
    assert_eq!(SETTINGS_REFERENCE_WIDTH, 760);
    assert_eq!(SETTINGS_REFERENCE_HEIGHT, 560);
    assert_eq!(SETTINGS_TITLEBAR_HEIGHT, 42);
    assert_eq!(SETTINGS_RAIL_WIDTH, 192);
    assert_eq!(SETTINGS_CONTENT_WIDTH, 568);
    assert_eq!(
        SETTINGS_RAIL_WIDTH + SETTINGS_CONTENT_WIDTH,
        SETTINGS_REFERENCE_WIDTH
    );
    assert_eq!(CAPTIONLESS_DRAG_HEIGHT, 42);
}

#[test]
fn settings_natural_height_is_capped_inside_the_monitor() {
    assert_eq!(settings_window_height_cap_for_monitor(1080), 1032);
    assert_eq!(settings_window_height_cap_for_monitor(900), 852);
    assert_eq!(settings_window_height_cap_for_monitor(648), 600);
    assert_eq!(settings_window_height_cap_for_monitor(32), 1);
}

#[test]
fn about_diagnostics_contains_only_app_engine_and_host_groups() {
    let snapshot = AboutSnapshot {
        version: "0.1.0".to_owned(),
        package_version: "0.1.0-linux-alpha.113".to_owned(),
        channel: "Linux alpha".to_owned(),
        build: "abcdef0".to_owned(),
        license: "GPL-3.0-or-later".to_owned(),
        libmpv: "0.40.0".to_owned(),
        ffmpeg: "8.0".to_owned(),
        render_api: "libmpv render".to_owned(),
        graphics: "OpenGL · GTK GLArea".to_owned(),
        hwdec: "auto-safe".to_owned(),
        os: "Linux".to_owned(),
        gtk: "4.20.0".to_owned(),
        cpu: "x86_64".to_owned(),
        install: "Deb installer".to_owned(),
    };
    let text = about_diagnostics_text(&snapshot);
    assert!(text.contains("\nEngine\n"));
    assert!(text.contains("\nHost\n"));
    assert!(!text.contains("\nUpdates\n"));
    assert!(!text.contains("  Updates"));
}

#[test]
fn settings_initial_page_env_accepts_known_pages_only() {
    assert_eq!(normalized_settings_page(" Shortcuts "), Some("shortcuts"));
    assert_eq!(normalized_settings_page("about"), Some("about"));
    assert_eq!(normalized_settings_page("native-caption"), None);
}

#[test]
fn media_info_preview_sample_covers_the_polished_surfaces() {
    let sample = media_info_preview_sample();

    // The visual smoke fixture must exercise both tabs and both track groups so
    // screenshots catch regressions across the complete modal.
    assert!(sample.path.is_some());
    let section_titles: Vec<&str> = sample
        .sections
        .iter()
        .map(|section| section.title.as_str())
        .collect();
    assert!(section_titles.contains(&"File"));
    assert!(section_titles.contains(&"Video"));
    assert!(section_titles.contains(&"Playback"));

    assert!(
        sample
            .tracks
            .iter()
            .any(|track| track.kind == TrackKind::Audio)
    );
    assert!(
        sample
            .tracks
            .iter()
            .any(|track| track.kind == TrackKind::Subtitle)
    );
    assert!(sample.tracks.iter().any(|track| track.external));

    // The fixture showcases both caption slots so the media surface (and its
    // screenshot) proves the primary and secondary subtitle read distinctly.
    assert!(
        sample
            .tracks
            .iter()
            .any(|track| track.kind == TrackKind::Subtitle && track.detail.starts_with("Primary")),
        "media info preview should name a primary subtitle"
    );
    assert!(
        sample
            .tracks
            .iter()
            .any(|track| track.kind == TrackKind::Subtitle
                && track.detail.starts_with("Secondary")),
        "media info preview should name a secondary subtitle"
    );

    let stream_sections = media_info_stream_sections(&sample);
    let stream_titles: Vec<&str> = stream_sections
        .iter()
        .map(|section| section.title.as_str())
        .collect();
    assert!(!stream_titles.contains(&"Playback"));

    let stats_sections = media_info_stats_sections(&sample);
    let stats_titles: Vec<&str> = stats_sections
        .iter()
        .map(|section| section.title.as_str())
        .collect();
    assert_eq!(
        stats_titles,
        vec!["Decode · Render", "Live · Performance", "Display · Output"]
    );
}

#[test]
fn media_info_modal_geometry_matches_reference_and_narrow_clamp() {
    assert_eq!(media_info_modal_geometry(1120, 680), (720, 571));
    assert_eq!(media_info_modal_geometry(480, 540), (441, 453));
    assert_eq!(media_info_modal_geometry(700, 400), (644, 336));
}

#[test]
fn media_info_identity_is_app_owned_cairo_geometry() {
    assert_eq!(MEDIA_INFO_IDENTITY_BADGE_SIZE, 38);
    assert_eq!(MEDIA_INFO_IDENTITY_VIEWBOX_SIZE, 20.0);
    assert_eq!(MEDIA_INFO_IDENTITY_RING_RADIUS, 7.5);

    let source = include_str!("media_info.rs");
    let identity_constructor = source
        .split("fn media_info_identity()")
        .nth(1)
        .and_then(|source| source.split("fn draw_media_info_identity(").next())
        .expect("media-info identity constructor should remain inspectable");
    assert!(identity_constructor.contains("gtk::DrawingArea::new()"));
    assert!(identity_constructor.contains("set_draw_func(draw_media_info_identity)"));
    assert!(!identity_constructor.contains("from_icon_name"));
    assert!(!source.contains("help-about-symbolic"));
}

#[test]
fn media_info_modal_classes_have_scoped_css() {
    let stylesheet = include_str!("css.rs");
    for class in [
        "okp-media-info-modal-layer",
        "okp-media-info-backdrop",
        "okp-media-info-card",
        "okp-media-info-header",
        "okp-media-info-identity",
        "okp-media-info-title",
        "okp-media-info-subtitle",
        "okp-media-info-close",
        "okp-media-info-tabs",
        "okp-media-info-tab-strip",
        "okp-media-info-tab",
        "okp-media-info-stack",
        "okp-media-info-scroller",
        "okp-media-info-content",
        "okp-media-info-grid",
        "okp-media-info-empty",
        "okp-media-info-footer",
        "okp-media-info-path",
        "okp-media-info-copy",
        "okp-media-info-done",
    ] {
        assert!(
            stylesheet.contains(&format!(".{class}")),
            "Media Information class {class} must have modal-scoped CSS"
        );
    }
}

#[test]
fn both_media_info_menu_entries_use_the_in_player_modal_entry_point() {
    let source = include_str!("track_popovers.rs");
    let more = source
        .split_once("pub(crate) fn more_popover_content")
        .expect("More popover implementation")
        .1
        .split_once("pub(crate) fn advanced_command_popover_content")
        .expect("advanced popover follows More")
        .0;
    let advanced = source
        .split_once("pub(crate) fn advanced_command_popover_content")
        .expect("advanced popover implementation")
        .1
        .split_once("pub(crate) fn track_popover_content")
        .expect("track popover helper follows advanced popover")
        .0;

    assert_eq!(more.matches("open_media_info_window(").count(), 1);
    assert_eq!(advanced.matches("open_media_info_window(").count(), 1);
    assert!(!source.contains("show_media_info_window"));
}

#[test]
fn media_info_row_highlights_active_hdr_only() {
    assert!(media_info_row_is_highlight(
        "Dynamic Range",
        "HDR (PQ / ST 2084, BT.2020)"
    ));
    assert!(media_info_row_is_highlight("dynamic range", "Dolby Vision"));
    assert!(!media_info_row_is_highlight("Dynamic Range", "No"));
    assert!(!media_info_row_is_highlight("Dynamic Range", "SDR"));
    assert!(!media_info_row_is_highlight("Codec", "HEVC (H.265)"));
}

#[test]
fn mpris_snapshot_reports_stopped_without_media() {
    let snapshot = MprisSnapshot::default();

    assert_eq!(snapshot.playback_status(), "Stopped");
    assert!(!snapshot.has_media);
    assert_eq!(snapshot.position_us, 0);
}

#[test]
fn mpris_metadata_contains_core_track_fields() {
    let snapshot = MprisSnapshot {
        has_media: true,
        paused: false,
        position_us: 1_000_000,
        duration_us: Some(30_000_000),
        volume: 1.0,
        can_go_next: true,
        can_go_previous: true,
        title: "subtest.mkv".to_owned(),
        uri: Some("file:///tmp/subtest.mkv".to_owned()),
        art_url: Some("file:///tmp/subtest.jpg".to_owned()),
        ..MprisSnapshot::default()
    };

    let metadata = mpris_metadata(&snapshot);

    assert_eq!(snapshot.playback_status(), "Playing");
    assert!(metadata.contains_key("mpris:trackid"));
    assert!(metadata.contains_key("mpris:length"));
    assert!(metadata.contains_key("xesam:title"));
    assert!(metadata.contains_key("xesam:url"));
    assert!(metadata.contains_key("mpris:artUrl"));
}

#[test]
fn mpris_invalidations_cover_shell_state_without_position_spam() {
    let previous = MprisSnapshot::default();
    let next = MprisSnapshot {
        has_media: true,
        paused: false,
        position_us: 1_000_000,
        duration_us: Some(30_000_000),
        volume: 0.75,
        can_go_next: true,
        can_go_previous: true,
        title: "subtest.mkv".to_owned(),
        uri: Some("file:///tmp/subtest.mkv".to_owned()),
        ..MprisSnapshot::default()
    };

    let invalidated = mpris_invalidated_properties(&previous, &next);

    assert!(invalidated.contains(&"PlaybackStatus"));
    assert!(invalidated.contains(&"Metadata"));
    assert!(invalidated.contains(&"CanPlay"));
    assert!(invalidated.contains(&"CanPause"));
    assert!(invalidated.contains(&"CanSeek"));
    assert!(invalidated.contains(&"CanGoNext"));
    assert!(invalidated.contains(&"CanGoPrevious"));
    assert!(invalidated.contains(&"Volume"));
    assert!(!invalidated.contains(&"Position"));

    let moved = MprisSnapshot {
        position_us: 2_000_000,
        ..next.clone()
    };

    assert!(mpris_invalidated_properties(&next, &moved).is_empty());
}

#[test]
fn mpris_metadata_invalidates_when_art_url_changes() {
    let previous = MprisSnapshot {
        has_media: true,
        paused: false,
        position_us: 1_000_000,
        duration_us: Some(30_000_000),
        title: "song.flac".to_owned(),
        uri: Some("file:///tmp/song.flac".to_owned()),
        art_url: Some("file:///tmp/old-cover.jpg".to_owned()),
        ..MprisSnapshot::default()
    };
    let next = MprisSnapshot {
        art_url: Some("file:///tmp/new-cover.jpg".to_owned()),
        ..previous.clone()
    };

    assert_eq!(
        mpris_invalidated_properties(&previous, &next),
        vec!["Metadata"]
    );
}

#[test]
fn mpris_sidecar_art_prefers_same_named_image() {
    let root = unique_temp_dir("okp-mpris-art-same-name");
    fs::create_dir_all(&root).expect("test folder should be created");
    let media = root.join("Track 01.flac");
    let folder_cover = root.join("cover.jpg");
    let same_named = root.join("Track 01.png");
    fs::write(&media, []).expect("test media should be written");
    write_jpeg_header(&folder_cover);
    write_png_header(&same_named);

    assert_eq!(mpris_sidecar_art_path(&media), Some(same_named.clone()));
    assert_eq!(
        mpris_sidecar_art_url(&media),
        Some(local_file_uri(&same_named))
    );

    fs::remove_dir_all(root).expect("test folder should be removed");
}

#[test]
fn mpris_sidecar_art_uses_folder_priority_and_skips_junk() {
    let root = unique_temp_dir("okp-mpris-art-folder");
    fs::create_dir_all(&root).expect("test folder should be created");
    let media = root.join("Episode 1.mkv");
    let bad_cover = root.join("cover.jpg");
    let folder_cover = root.join("folder.jpg");
    let poster = root.join("poster.png");
    fs::write(&media, []).expect("test media should be written");
    fs::write(&bad_cover, []).expect("junk cover should be written");
    write_jpeg_header(&folder_cover);
    write_png_header(&poster);

    assert_eq!(mpris_sidecar_art_path(&media), Some(folder_cover));

    fs::remove_dir_all(root).expect("test folder should be removed");
}

#[test]
fn mpris_local_art_prefers_sidecar_before_embedded_art() {
    let root = unique_temp_dir("okp-mpris-art-sidecar-first");
    fs::create_dir_all(&root).expect("test folder should be created");
    let media = root.join("Song.flac");
    let sidecar = root.join("Song.jpg");
    fs::write(&media, b"not a real flac").expect("test media should be written");
    write_jpeg_header(&sidecar);

    assert_eq!(mpris_local_art_url(&media), Some(local_file_uri(&sidecar)));

    let key = mpris_embedded_art_cache_key(&media).expect("media key should be available");
    let cache = MPRIS_EMBEDDED_ART_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    assert!(
        !cache
            .lock()
            .expect("embedded art cache should lock")
            .contains_key(&key)
    );

    fs::remove_dir_all(root).expect("test folder should be removed");
}

#[test]
fn mpris_embedded_art_is_audio_only() {
    let root = unique_temp_dir("okp-mpris-art-video-skip");
    fs::create_dir_all(&root).expect("test folder should be created");
    let media = root.join("Movie.mkv");
    fs::write(&media, b"not a real video").expect("test media should be written");

    assert_eq!(mpris_embedded_art_url(&media), None);

    fs::remove_dir_all(root).expect("test folder should be removed");
}

#[test]
fn mpris_embedded_art_cache_path_changes_when_media_changes() {
    let root = unique_temp_dir("okp-mpris-art-cache-key");
    fs::create_dir_all(&root).expect("test folder should be created");
    let media = root.join("Song.flac");
    let cache_dir = root.join("cache");
    fs::write(&media, [1_u8]).expect("test media should be written");
    let before = mpris_embedded_art_cache_key(&media).expect("cache key should resolve");

    fs::write(&media, [1_u8, 2_u8]).expect("test media should be updated");
    let after = mpris_embedded_art_cache_key(&media).expect("updated cache key should resolve");

    assert_ne!(before.len, after.len);
    assert_ne!(
        mpris_embedded_art_cache_path_in_dir(&before, &cache_dir),
        mpris_embedded_art_cache_path_in_dir(&after, &cache_dir)
    );

    fs::remove_dir_all(root).expect("test folder should be removed");
}

#[test]
fn mpris_app_icon_art_fallback_is_available_in_dev_tree() {
    let path = mpris_app_icon_art_path().expect("app icon should resolve in dev or installed tree");

    assert!(path.is_file());
    assert!(mpris_app_icon_art_url().is_some());
}

#[test]
fn nfo_title_projects_only_to_the_current_local_item() {
    let current = PathBuf::from("/media/Movie.mkv");
    let other = PlaylistItem::Local(PathBuf::from("/media/Bonus.mkv"));
    let current_item = PlaylistItem::Local(current.clone());
    let state = PlayerState {
        current_file: Some(current),
        current_nfo_title: okp_core::nfo_metadata::NfoTitleState::Resolved(Some(
            "Curated Movie Title".to_owned(),
        )),
        ..PlayerState::default()
    };

    assert_eq!(current_media_title(&state), "Curated Movie Title");
    assert_eq!(
        playlist_item_title(&state, &current_item),
        "Curated Movie Title"
    );
    assert_eq!(playlist_item_title(&state, &other), "Bonus.mkv");
}

#[test]
fn nfo_results_require_matching_generation_and_path() {
    let result = NfoTitleResult {
        source_generation: 7,
        media_path: PathBuf::from("/media/Movie.mkv"),
        title: Some("Curated Movie Title".to_owned()),
    };

    assert!(nfo_result_matches(
        &result,
        7,
        Some(Path::new("/media/Movie.mkv"))
    ));
    assert!(!nfo_result_matches(
        &result,
        8,
        Some(Path::new("/media/Movie.mkv"))
    ));
    assert!(!nfo_result_matches(
        &result,
        7,
        Some(Path::new("/media/Other.mkv"))
    ));
    assert!(!nfo_result_matches(&result, 7, None));
}

#[test]
fn nfo_job_reads_same_basename_sidecar_off_thread() {
    let root = unique_temp_dir("okp-linux-nfo-job");
    fs::create_dir_all(&root).expect("test folder should be created");
    let media = root.join("Movie.mkv");
    fs::write(&media, b"media").expect("test media should be written");
    fs::write(
        root.join("Movie.nfo"),
        b"<movie><title>Curated Movie Title</title></movie>",
    )
    .expect("test nfo should be written");

    let jobs = NfoTitleJobs::default();
    jobs.resolve(11, media.clone());
    let result = jobs
        .recv_timeout(Duration::from_secs(2))
        .expect("worker should return the local sidecar result");

    assert_eq!(result.source_generation, 11);
    assert_eq!(result.media_path, media);
    assert_eq!(result.title.as_deref(), Some("Curated Movie Title"));

    fs::remove_dir_all(root).expect("test folder should be removed");
}

#[test]
fn source_changes_reset_nfo_state_before_async_resolution() {
    let state = Rc::new(RefCell::new(PlayerState {
        current_nfo_title: okp_core::nfo_metadata::NfoTitleState::Resolved(Some(
            "Old Title".to_owned(),
        )),
        ..PlayerState::default()
    }));

    let path = PathBuf::from("/media/Movie.mkv");
    remember_loaded_media_with_playlist(&state, path.clone(), vec![PlaylistItem::Local(path)]);
    assert_eq!(
        state.borrow().current_nfo_title,
        okp_core::nfo_metadata::NfoTitleState::Pending
    );

    remember_loaded_url(&state, "https://example.com/live.m3u8".to_owned());
    assert_eq!(
        state.borrow().current_nfo_title,
        okp_core::nfo_metadata::NfoTitleState::NotApplicable
    );
}

#[test]
fn mpris_tracklist_window_limits_context_around_current_track() {
    assert_eq!(mpris_tracklist_window(3, 1), (0, 3));
    assert_eq!(
        mpris_tracklist_window(30, 0),
        (0, MPRIS_TRACKLIST_CONTEXT_LIMIT)
    );
    assert_eq!(
        mpris_tracklist_window(30, 25),
        (9, 9 + MPRIS_TRACKLIST_CONTEXT_LIMIT)
    );
}

#[test]
fn mpris_tracklist_metadata_uses_current_track_id() {
    let root = unique_temp_dir("okp-mpris-tracklist");
    fs::create_dir_all(&root).expect("test folder should be created");
    let first = root.join("Episode 1.mkv");
    let second = root.join("Episode 2.mkv");
    let third = root.join("Episode 3.mkv");
    fs::write(&first, []).expect("test media should be written");
    fs::write(&second, []).expect("test media should be written");
    fs::write(&third, []).expect("test media should be written");

    let mut state = PlayerState {
        current_file: Some(second.clone()),
        current_nfo_title: okp_core::nfo_metadata::NfoTitleState::Resolved(Some(
            "Episode Two Curated".to_owned(),
        )),
        playlist: Playlist::from_items(
            vec![
                PlaylistItem::Local(first.clone()),
                PlaylistItem::Local(second.clone()),
                PlaylistItem::Local(third.clone()),
            ],
            Some(&PlaylistItem::Local(second.clone())),
            false,
        ),
        ..PlayerState::default()
    };

    let tracks = mpris_tracklist_from_state(&state, Some(42_000_000));

    assert_eq!(tracks.len(), 3);
    assert_eq!(tracks[1].title, "Episode Two Curated");
    assert_eq!(tracks[1].duration_us, Some(42_000_000));
    assert_eq!(
        mpris_tracklist_target_for_id(&state, tracks[1].id.as_str()),
        Some((1, PlaylistItem::Local(second.clone())))
    );

    let snapshot = mpris_snapshot_from_state(&state, None);
    assert_eq!(snapshot.title, "Episode Two Curated");
    assert_eq!(snapshot.current_track_id, Some(snapshot.track_id.clone()));
    assert!(snapshot.tracklist_track_ids().contains(&snapshot.track_id));
    assert!(mpris_metadata(&snapshot).contains_key("mpris:trackid"));
    assert!(mpris_track_metadata(&tracks[1]).contains_key("mpris:trackid"));

    state.current_file = Some(third);
    state.current_nfo_title = okp_core::nfo_metadata::NfoTitleState::NotApplicable;
    let moved = mpris_snapshot_from_state(&state, None);
    assert_ne!(snapshot.current_track_id, moved.current_track_id);
    assert!(mpris_tracklist_replaced_signal(&snapshot, &moved).is_some());
    assert!(mpris_tracklist_invalidated_properties(&snapshot, &moved).is_empty());

    fs::remove_dir_all(root).expect("test folder should be removed");
}

#[test]
fn mpris_tracklist_replaced_invalidates_tracks_when_playlist_changes() {
    let first = PlaylistItem::Url("https://example.test/one.mp3".to_owned());
    let second = PlaylistItem::Url("https://example.test/two.mp3".to_owned());
    let previous = MprisSnapshot {
        tracklist: vec![MprisTrack {
            id: mpris_tracklist_id_for_item(0, &first),
            title: first.display_name(),
            uri: mpris_playlist_item_uri(&first),
            duration_us: None,
            art_url: None,
        }],
        current_track_id: Some(mpris_tracklist_id_for_item(0, &first)),
        ..MprisSnapshot::default()
    };
    let next = MprisSnapshot {
        tracklist: vec![
            previous.tracklist[0].clone(),
            MprisTrack {
                id: mpris_tracklist_id_for_item(1, &second),
                title: second.display_name(),
                uri: mpris_playlist_item_uri(&second),
                duration_us: None,
                art_url: None,
            },
        ],
        ..previous.clone()
    };

    assert_eq!(
        mpris_tracklist_invalidated_properties(&previous, &next),
        vec!["Tracks"]
    );
    let (tracks, current_track) =
        mpris_tracklist_replaced_signal(&previous, &next).expect("playlist should change");
    assert_eq!(tracks.len(), 2);
    assert_eq!(Some(current_track), previous.current_track_id);
}

#[test]
fn mpris_seeked_signal_tracks_large_position_jumps_only() {
    let previous = MprisSnapshot {
        has_media: true,
        paused: false,
        position_us: 1_000_000,
        duration_us: Some(30_000_000),
        volume: 1.0,
        can_go_next: false,
        can_go_previous: false,
        title: "subtest.mkv".to_owned(),
        uri: Some("file:///tmp/subtest.mkv".to_owned()),
        ..MprisSnapshot::default()
    };

    let normal_tick = MprisSnapshot {
        position_us: previous.position_us + 200_000,
        ..previous.clone()
    };
    assert_eq!(mpris_seeked_position(&previous, &normal_tick), None);

    let seek_jump = MprisSnapshot {
        position_us: previous.position_us + 5_000_000,
        ..previous.clone()
    };
    assert_eq!(
        mpris_seeked_position(&previous, &seek_jump),
        Some(6_000_000)
    );

    let different_media = MprisSnapshot {
        position_us: 0,
        title: "other.mkv".to_owned(),
        uri: Some("file:///tmp/other.mkv".to_owned()),
        ..previous.clone()
    };
    assert_eq!(mpris_seeked_position(&previous, &different_media), None);
}

#[test]
fn mpris_volume_setter_maps_to_mpv_percent_range() {
    assert_eq!(mpris_volume_to_mpv_percent(0.0), Some(0.0));
    assert_eq!(mpris_volume_to_mpv_percent(0.42), Some(42.0));
    assert_eq!(mpris_volume_to_mpv_percent(1.0), Some(100.0));
    assert_eq!(mpris_volume_to_mpv_percent(2.0), Some(130.0));
    assert_eq!(mpris_volume_to_mpv_percent(-1.0), Some(0.0));
    assert_eq!(mpris_volume_to_mpv_percent(f64::NAN), None);
    assert_eq!(mpris_volume_to_mpv_percent(f64::INFINITY), None);
}

#[test]
fn mpris_play_mode_properties_follow_player_state() {
    assert_eq!(mpris_loop_status(RepeatMode::Off), "None");
    assert_eq!(mpris_loop_status(RepeatMode::One), "Track");
    assert_eq!(mpris_loop_status(RepeatMode::All), "Playlist");
    assert_eq!(mpris_repeat_mode("None"), Some(RepeatMode::Off));
    assert_eq!(mpris_repeat_mode("Track"), Some(RepeatMode::One));
    assert_eq!(mpris_repeat_mode("Playlist"), Some(RepeatMode::All));
    assert_eq!(mpris_repeat_mode("bad"), None);

    let previous = MprisSnapshot::default();
    let next = MprisSnapshot {
        rate: 1.25,
        repeat_mode: RepeatMode::All,
        shuffle: true,
        ..MprisSnapshot::default()
    };
    let invalidated = mpris_invalidated_properties(&previous, &next);

    assert!(invalidated.contains(&"Rate"));
    assert!(invalidated.contains(&"LoopStatus"));
    assert!(invalidated.contains(&"Shuffle"));
}

#[test]
fn mpris_rate_setter_maps_to_supported_speed_range() {
    assert_eq!(mpris_rate_to_mpv_speed(0.1), Some(0.25));
    assert_eq!(mpris_rate_to_mpv_speed(1.25), Some(1.25));
    assert_eq!(mpris_rate_to_mpv_speed(9.0), Some(4.0));
    assert_eq!(mpris_rate_to_mpv_speed(f64::NAN), None);
    assert_eq!(mpris_rate_to_mpv_speed(f64::INFINITY), None);
}

#[test]
fn raw_mpv_config_parser_accepts_key_value_lines() {
    assert_eq!(
        parse_raw_mpv_config(
            "\
# comment
scale=ewa_lanczossharp
--profile=gpu-hq
script-opts=osc-layout=bottombar
"
        ),
        Ok(vec![
            ("scale".to_owned(), "ewa_lanczossharp".to_owned()),
            ("profile".to_owned(), "gpu-hq".to_owned()),
            ("script-opts".to_owned(), "osc-layout=bottombar".to_owned()),
        ])
    );
}

#[test]
fn raw_mpv_config_parser_reports_missing_value_separator_line() {
    let error =
        parse_raw_mpv_config("scale=ewa\nbad line\nprofile=gpu-hq").expect_err("line should fail");

    assert_eq!(error.line, 2);
    assert!(error.message.contains("key=value"));
}

#[test]
fn raw_mpv_config_parser_rejects_invalid_names() {
    let error = parse_raw_mpv_config("script/opts=value").expect_err("name should fail");

    assert_eq!(error.line, 1);
    assert!(error.message.contains("Option names"));
}

#[test]
fn raw_mpv_config_parser_rejects_protected_options() {
    let error = parse_raw_mpv_config("--vo=gpu").expect_err("vo should be managed");

    assert_eq!(error.line, 1);
    assert!(error.message.contains("managed by OK Player"));

    assert!(parse_raw_mpv_config("VO=gpu").is_err());

    for option in [
        "sub-scale=1.4",
        "sub-pos=90",
        "sub-ass-override=force",
        "secondary-sub-ass-override=strip",
        "sub-border-style=background-box",
        "SUB-BACK-COLOR=0/0/0/0.7",
        "sub-outline-size=6",
        "sub-shadow-color=#000000",
    ] {
        let error = parse_raw_mpv_config(option).expect_err("subtitle option should be managed");
        assert!(error.message.contains("managed by OK Player"));
    }
}

#[test]
fn raw_mpv_config_parser_rejects_nul_values() {
    let error = parse_raw_mpv_config("profile=gpu-hq\0").expect_err("nul should fail");

    assert_eq!(error.line, 1);
    assert!(error.message.contains("NUL"));
}

#[test]
fn desktop_mime_parser_keeps_registered_types() {
    let desktop_entry = "\
[Desktop Entry]
Name=OK Player
MimeType=video/mp4;video/x-matroska;audio/flac;
";

    assert_eq!(
        parse_desktop_mime_types(desktop_entry),
        vec![
            "video/mp4".to_owned(),
            "video/x-matroska".to_owned(),
            "audio/flac".to_owned(),
        ]
    );
    assert_eq!(count_registered_key_media_mimes(desktop_entry), 3);
}

#[test]
fn default_app_match_is_exact_desktop_id() {
    assert!(default_app_matches_ok_player(
        "com.befeast.okplayer.desktop"
    ));
    assert!(!default_app_matches_ok_player("vlc.desktop"));
    assert!(!default_app_matches_ok_player("com.befeast.okplayer"));
}

#[test]
fn desktop_scheme_parser_detects_reserved_ok_player_scheme() {
    let with_scheme = "\
[Desktop Entry]
MimeType=video/mp4;x-scheme-handler/ok-player;
";
    let without_scheme = "\
[Desktop Entry]
MimeType=video/mp4;audio/flac;
";
    assert!(desktop_registers_uri_scheme(with_scheme));
    assert!(!desktop_registers_uri_scheme(without_scheme));
}

#[test]
fn packaged_desktop_entry_advertises_reserved_scheme() {
    // The installed .deb ships this desktop file; if it stops advertising the reserved
    // scheme, `xdg-open ok-player://…` no longer routes to OK Player (PRD §13.4).
    let desktop = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../packaging/linux/com.befeast.okplayer.desktop"),
    )
    .expect("packaged desktop entry should be readable");
    assert!(
        desktop_registers_uri_scheme(&desktop),
        "desktop entry MimeType must include {LINUX_URI_SCHEME_MIME}"
    );
    assert!(
        desktop.contains("Exec=ok-player %U"),
        "Exec must pass URIs (%U) so the scheme handler receives the request"
    );
}

#[test]
fn uri_scheme_row_reflects_registration_and_default_handler() {
    // Not advertised by an installed desktop entry — the reservation is not in place.
    assert!(matches!(
        uri_scheme_row_content(false, None),
        ("Missing", _, IntegrationStatus::Bad)
    ));
    assert!(matches!(
        uri_scheme_row_content(false, Some(true)),
        ("Missing", _, IntegrationStatus::Bad)
    ));
    // Advertised is the reservation, so it reads Good whatever the default handler is; the
    // default only flavors the detail line.
    for default in [None, Some(true), Some(false)] {
        assert!(
            matches!(
                uri_scheme_row_content(true, default),
                ("Reserved", _, IntegrationStatus::Good)
            ),
            "registered scheme should read Good for default={default:?}"
        );
    }
    assert!(
        uri_scheme_row_content(true, Some(true))
            .1
            .contains("handled by OK Player")
    );
}

#[test]
fn reserved_uri_notice_intercepts_scheme_but_leaves_media_urls() {
    // A reserved request yields a local diagnostic naming the parsed command...
    let reserved = reserved_uri_notice("ok-player://open?path=/media/a.mkv")
        .expect("ok-player:// request should be reported");
    assert!(reserved.contains("reserved"));
    assert!(reserved.contains("open"));
    // ...a malformed request is reported too, not silently accepted...
    assert!(reserved_uri_notice("ok-player://").is_some());
    // ...and ordinary media stays untouched so file/URL open behavior is unchanged.
    assert_eq!(reserved_uri_notice("https://example.test/a.mp4"), None);
    assert_eq!(reserved_uri_notice("/media/movie.mkv"), None);
}

#[test]
fn youtube_resolver_probe_mirrors_a_path_lookup() {
    // The Open URL surface gates YouTube on this probe, so it must agree with a direct PATH
    // lookup for the named resolver and never panic when the tool is absent.
    assert_eq!(
        youtube_resolver_available(),
        find_executable(youtube_open::YOUTUBE_RESOLVER).is_some()
    );
}

#[test]
fn find_executable_rejects_a_file_without_an_execute_bit() {
    use std::os::unix::fs::PermissionsExt;

    // A resolver that sits on PATH but is not runnable must not count as "found": mpv's ytdl
    // hook would spawn it and fail with a generic exec error instead of showing the deliberate
    // missing-tooling state. `find_executable` is named for a runnable program, so it gates on
    // the execute bit.
    let dir = unique_temp_dir("okp-find-executable");
    fs::create_dir_all(&dir).expect("temp dir should be created");
    let tool = dir.join("yt-dlp");
    fs::write(&tool, b"#!/bin/sh\n").expect("fixture tool should be written");

    let tool_path = tool.to_str().expect("temp path should be valid UTF-8");

    // Mode 0o644: readable data, no execute bit -> not an executable.
    fs::set_permissions(&tool, fs::Permissions::from_mode(0o644))
        .expect("non-executable mode should apply");
    assert_eq!(find_executable(tool_path), None);

    // Add the owner execute bit and the same file now resolves.
    fs::set_permissions(&tool, fs::Permissions::from_mode(0o755))
        .expect("executable mode should apply");
    assert_eq!(find_executable(tool_path).as_deref(), Some(tool.as_path()));

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn open_url_checks_reserved_scheme_before_youtube_classification() {
    use youtube_open::OpenUrlOutcome;

    // The dialog checks the reserved ok-player:// scheme first, so a reserved request is
    // reported and never reaches the YouTube / engine routing...
    assert!(reserved_uri_notice("ok-player://open").is_some());

    // ...a real YouTube link is not reserved and, with no resolver on the host, lands in the
    // deliberate missing-tooling state rather than a silent hand-off to libmpv...
    assert_eq!(reserved_uri_notice("https://youtu.be/abc123"), None);
    assert_eq!(
        youtube_open::resolve_open_url("https://youtu.be/abc123", false),
        OpenUrlOutcome::YouTubeToolingMissing
    );
    // ...and with the resolver present it plays via mpv's ytdl hook.
    assert_eq!(
        youtube_open::resolve_open_url("https://youtu.be/abc123", true),
        OpenUrlOutcome::PlayYouTube
    );

    // An arbitrary non-YouTube stream URL keeps the existing direct-to-engine outcome
    // regardless of the resolver probe (existing http(s):// playback stays intact).
    for available in [true, false] {
        assert_eq!(
            youtube_open::resolve_open_url("https://example.test/a.mp4", available),
            OpenUrlOutcome::PlayDirect
        );
    }
}

#[test]
fn launch_args_report_reserved_scheme_without_queuing_it_as_media() {
    let launch = parse_launch_args_from(
        [
            "ok-player://play?id=42",
            "https://example.test/a.mp4",
            "/media/b.mkv",
        ]
        .into_iter()
        .map(Into::into),
    );

    // The reserved request never becomes a playlist item, but the real media does.
    assert_eq!(
        launch.items,
        vec![
            url_item("https://example.test/a.mp4"),
            local_item("/media/b.mkv"),
        ]
    );
    let notice = launch
        .reserved_notice()
        .expect("reserved request should surface a launch notice");
    assert!(notice.contains("play"));
}

#[test]
fn timeline_marks_include_ab_loop_points() {
    let chapters = vec![
        Chapter {
            index: 0,
            time: 0.0,
            title: Some("Start".to_owned()),
        },
        Chapter {
            index: 1,
            time: 42.0,
            title: Some("Scene".to_owned()),
        },
        Chapter {
            index: 2,
            time: f64::NAN,
            title: None,
        },
    ];

    assert_eq!(
        timeline_marks(
            &chapters,
            &[],
            &[],
            AbLoopState {
                a: Some(0.0),
                b: Some(120.0),
            },
            300.0,
        ),
        vec![
            TimelineMark {
                time: 42.0,
                kind: TimelineMarkKind::Chapter,
            },
            TimelineMark {
                time: 0.0,
                kind: TimelineMarkKind::AbStart,
            },
            TimelineMark {
                time: 120.0,
                kind: TimelineMarkKind::AbEnd,
            },
        ]
    );
}

#[test]
fn timeline_marks_include_bookmarks_and_drop_edge_and_nonfinite() {
    // Bookmarks tick alongside chapters; a mark at 0.0 (the very left edge) and a
    // non-finite time are dropped, exactly like the chapter filter.
    assert_eq!(
        timeline_marks(
            &[],
            &[],
            &[0.0, 90.0, f64::NAN],
            AbLoopState::default(),
            300.0,
        ),
        vec![TimelineMark {
            time: 90.0,
            kind: TimelineMarkKind::Bookmark,
        }]
    );
}

#[test]
fn timeline_marks_drop_chapters_and_bookmarks_at_or_past_duration() {
    // Chapters and bookmarks must map inside the seek scale's [0, duration] range: a
    // mark at exactly the end collapses onto the right handle and one past it would
    // draw off the bar, so both are dropped. A mark comfortably inside survives.
    let chapters = vec![
        Chapter {
            index: 0,
            time: 30.0,
            title: Some("Inside".to_owned()),
        },
        Chapter {
            index: 1,
            time: 120.0,
            title: Some("At the end".to_owned()),
        },
        Chapter {
            index: 2,
            time: 150.0,
            title: Some("Past the end".to_owned()),
        },
    ];

    assert_eq!(
        timeline_marks(
            &chapters,
            &[],
            &[90.0, 120.0, 200.0],
            AbLoopState::default(),
            120.0
        ),
        vec![
            TimelineMark {
                time: 30.0,
                kind: TimelineMarkKind::Chapter,
            },
            TimelineMark {
                time: 90.0,
                kind: TimelineMarkKind::Bookmark,
            },
        ]
    );
}

#[test]
fn timeline_marks_keep_all_when_duration_unknown() {
    // Before the duration is observed (0.0) there is no upper bound to map against, so
    // every finite, positive mark is kept until the real length arrives.
    let chapters = vec![Chapter {
        index: 0,
        time: 500.0,
        title: None,
    }];

    assert_eq!(
        timeline_marks(&chapters, &[], &[900.0], AbLoopState::default(), 0.0),
        vec![
            TimelineMark {
                time: 500.0,
                kind: TimelineMarkKind::Chapter,
            },
            TimelineMark {
                time: 900.0,
                kind: TimelineMarkKind::Bookmark,
            },
        ]
    );
}

#[test]
fn timeline_marks_combine_degenerate_ab_loop_points() {
    assert_eq!(
        timeline_marks(
            &[],
            &[],
            &[],
            AbLoopState {
                a: Some(12.0),
                b: Some(12.25),
            },
            300.0,
        ),
        vec![TimelineMark {
            time: 12.125,
            kind: TimelineMarkKind::AbLoop,
        }]
    );
    assert!(should_combine_ab_loop_marks(12.0, 12.5));
    assert!(!should_combine_ab_loop_marks(12.0, 12.501));
}

#[test]
fn timeline_marks_keep_interval_ticks_distinct_and_in_range() {
    assert_eq!(
        timeline_marks(
            &[],
            &[0.0, 300.0, 600.0, 900.0],
            &[150.0],
            AbLoopState::default(),
            900.0,
        ),
        vec![
            TimelineMark {
                time: 300.0,
                kind: TimelineMarkKind::Interval,
            },
            TimelineMark {
                time: 600.0,
                kind: TimelineMarkKind::Interval,
            },
            TimelineMark {
                time: 150.0,
                kind: TimelineMarkKind::Bookmark,
            },
        ]
    );
}

#[test]
fn snapshot_interval_fallback_never_mixes_with_embedded_chapters() {
    let fallback = SidePanelSnapshot {
        duration: Some(3600.0),
        ..Default::default()
    };
    assert!(snapshot_has_chapter_surface(&fallback));
    assert_eq!(snapshot_interval_chapters(&fallback).len(), 12);

    let embedded = SidePanelSnapshot {
        chapters: vec![Chapter {
            index: 0,
            time: 0.0,
            title: Some("Intro".to_owned()),
        }],
        duration: Some(3600.0),
        ..Default::default()
    };
    assert!(snapshot_has_chapter_surface(&embedded));
    assert!(snapshot_interval_chapters(&embedded).is_empty());

    let unknown_duration = SidePanelSnapshot::default();
    assert!(!snapshot_has_chapter_surface(&unknown_duration));
    assert!(snapshot_interval_chapters(&unknown_duration).is_empty());
}

#[test]
fn detect_chapters_is_honestly_unavailable_without_an_engine() {
    assert_eq!(
        chapter_math::ChapterDetection::begin(SCENE_DETECTION_ENGINE_AVAILABLE),
        chapter_math::ChapterDetection::Unavailable
    );
}

#[test]
fn interval_preview_covers_interval_detection_and_bookmark_sources() {
    let sample = side_panel_interval_preview_sample();
    assert!(sample.chapters.is_empty());
    assert_eq!(snapshot_interval_chapters(&sample).len(), 12);
    assert_eq!(sample.detection, chapter_math::ChapterDetection::Idle);
    assert!(sample.current_file.is_some());
    assert!(!sample.bookmarks.is_empty());
}

#[test]
fn seek_hover_source_falls_back_to_timecode_only_for_streams() {
    let file = PathBuf::from("/media/films/Feature.mkv");

    // A local file is the thumbnail source.
    assert_eq!(
        seek_hover_source(Some(file.clone()), None),
        Some(Some(file.clone()))
    );

    // A stream previews the timecode and chapter but has no on-disk file to sample, so
    // the source resolves to `Some(None)` — the deliberate timecode-only fallback.
    assert_eq!(
        seek_hover_source(None, Some("https://stream.example.com/live.m3u8")),
        Some(None)
    );

    // With nothing loaded there is no preview at all.
    assert_eq!(seek_hover_source(None, None), None);
}

#[test]
fn chapter_at_time_maps_the_hover_time_to_the_last_started_chapter() {
    let chapters = vec![
        Chapter {
            index: 0,
            time: 0.0,
            title: Some("Cold Open".to_owned()),
        },
        Chapter {
            index: 1,
            time: 60.0,
            title: Some("Main Titles".to_owned()),
        },
        Chapter {
            index: 2,
            time: 180.0,
            title: Some("Finale".to_owned()),
        },
    ];

    // Before the first start there is no chapter to label the hover with.
    assert!(chapter_at_time(&chapters, -5.0).is_none());
    // Inside a chapter (and exactly on a boundary) resolves to that chapter.
    assert_eq!(chapter_at_time(&chapters, 0.0).map(|c| c.index), Some(0));
    assert_eq!(chapter_at_time(&chapters, 59.9).map(|c| c.index), Some(0));
    assert_eq!(chapter_at_time(&chapters, 60.0).map(|c| c.index), Some(1));
    assert_eq!(chapter_at_time(&chapters, 999.0).map(|c| c.index), Some(2));
    // No chapters means no hover label.
    assert!(chapter_at_time(&[], 42.0).is_none());
}

#[test]
fn current_chapter_index_follows_the_playhead_through_core() {
    let chapters = vec![
        Chapter {
            index: 0,
            time: 0.0,
            title: None,
        },
        Chapter {
            index: 1,
            time: 100.0,
            title: None,
        },
        Chapter {
            index: 2,
            time: 200.0,
            title: None,
        },
    ];

    assert_eq!(current_chapter_index(&chapters, Some(150.0)), Some(1));
    assert_eq!(current_chapter_index(&chapters, Some(250.0)), Some(2));
    // Before the first chapter, on a missing position, and on a non-finite
    // position there is no current chapter to highlight.
    assert_eq!(current_chapter_index(&chapters, Some(-5.0)), None);
    assert_eq!(current_chapter_index(&chapters, None), None);
    assert_eq!(current_chapter_index(&chapters, Some(f64::NAN)), None);
}

#[test]
fn always_on_top_backend_is_explicit_for_x11_and_unsupported_displays() {
    assert_eq!(
        always_on_top_backend("GdkX11Display"),
        AlwaysOnTopBackend::X11Ewmh
    );
    for display in ["GdkWaylandDisplay", "GdkBroadwayDisplay", ""] {
        assert_eq!(
            always_on_top_backend(display),
            AlwaysOnTopBackend::Unavailable,
            "{display}"
        );
    }
}

#[test]
fn native_display_interop_is_requested_only_for_wayland() {
    assert!(is_wayland_display("GdkWaylandDisplay"));
    for display in ["GdkX11Display", "GdkBroadwayDisplay", ""] {
        assert!(!is_wayland_display(display), "{display}");
    }
}

#[test]
fn track_label_shows_tags_without_a_selection_prefix() {
    // Selection is shown by the row's leading check, so the label must never
    // prepend "On " (which used to shift long titles and break alignment).
    let subtitle = Track {
        id: 3,
        kind: TrackKind::Subtitle,
        selected: true,
        external: true,
        external_filename: Some("/tmp/example.srt".to_owned()),
        default: false,
        title: Some("English (SDH)".to_owned()),
        lang: None,
        codec: None,
        audio_channels: None,
    };
    assert_eq!(track_label(&subtitle), "English (SDH) · SRT · EXT");

    let audio = Track {
        id: 1,
        kind: TrackKind::Audio,
        selected: true,
        external: false,
        external_filename: None,
        default: true,
        title: Some("English".to_owned()),
        lang: None,
        codec: Some("eac3".to_owned()),
        audio_channels: Some("5.1".to_owned()),
    };
    assert_eq!(track_label(&audio), "English · 5.1 · EAC3");
}

#[test]
fn subtitle_search_source_reports_explicit_no_track_and_format_states() {
    assert_eq!(
        selected_subtitle_search_source(&[], None, Some(Path::new("/media/Episode 1.mkv"))),
        SubtitleSearchSource::NoActiveTrack
    );

    let embedded = Track {
        id: 3,
        kind: TrackKind::Subtitle,
        selected: true,
        external: false,
        external_filename: None,
        default: false,
        title: Some("English".to_owned()),
        lang: None,
        codec: Some("subrip".to_owned()),
        audio_channels: None,
    };
    assert_eq!(
        selected_subtitle_search_source(&[embedded], None, Some(Path::new("/media/Episode 1.mkv"))),
        SubtitleSearchSource::NotExternal
    );

    let unsupported = Track {
        id: 4,
        kind: TrackKind::Subtitle,
        selected: true,
        external: true,
        external_filename: Some("/tmp/Episode 1.ass".to_owned()),
        default: false,
        title: Some("English".to_owned()),
        lang: None,
        codec: Some("ass".to_owned()),
        audio_channels: None,
    };
    assert_eq!(
        selected_subtitle_search_source(
            &[unsupported],
            None,
            Some(Path::new("/media/Episode 1.mkv"))
        ),
        SubtitleSearchSource::UnsupportedFormat
    );
}

#[test]
fn subtitle_search_source_resolves_relative_primary_external_track() {
    let primary = Track {
        id: 4,
        kind: TrackKind::Subtitle,
        selected: true,
        external: true,
        external_filename: Some("subs/Episode 1.srt".to_owned()),
        default: false,
        title: Some("English".to_owned()),
        lang: None,
        codec: Some("subrip".to_owned()),
        audio_channels: None,
    };
    let secondary = Track {
        id: 5,
        external_filename: Some("subs/Episode 1.es.lrc".to_owned()),
        ..primary.clone()
    };

    assert_eq!(
        selected_subtitle_search_source(
            &[secondary, primary.clone()],
            Some(5),
            Some(Path::new("/media/Episode 1.mkv"))
        ),
        SubtitleSearchSource::Available(PathBuf::from("/media/subs/Episode 1.srt"))
    );
    assert_eq!(
        selected_subtitle_search_source(&[primary], None, None),
        SubtitleSearchSource::MissingPath
    );
}

#[test]
fn player_popovers_keep_their_canonical_independent_widths() {
    assert_eq!(PlayerPopoverKind::Speed.width(), 120);
    assert_eq!(PlayerPopoverKind::Subtitles.width(), 262);
    assert_eq!(PlayerPopoverKind::Audio.width(), 248);
    assert_eq!(PlayerPopoverKind::More.width(), 210);

    let quick_widths = [
        PlayerPopoverKind::Speed.width(),
        PlayerPopoverKind::Subtitles.width(),
        PlayerPopoverKind::Audio.width(),
        PlayerPopoverKind::More.width(),
    ];
    assert!(!quick_widths.contains(&PlayerPopoverKind::AdvancedCommands.width()));
    assert_eq!(PlayerPopoverKind::AdvancedCommands.width(), 320);
}

#[test]
fn more_stays_curated_while_player_context_menu_keeps_legacy_commands() {
    let source = include_str!("track_popovers.rs");
    let more = source
        .split_once("pub(crate) fn more_popover_content")
        .expect("More popover implementation")
        .1
        .split_once("pub(crate) fn advanced_command_popover_content")
        .expect("advanced popover follows More")
        .0;
    let advanced = source
        .split_once("pub(crate) fn advanced_command_popover_content")
        .expect("advanced popover implementation")
        .1
        .split_once("pub(crate) fn track_popover_content")
        .expect("track popover helper follows advanced commands")
        .0;

    let curated_more = [
        "Open file...",
        "Close file",
        "Mini player",
        "A-B loop",
        "Screenshot with subtitles",
        "Copy frame to clipboard",
        "Media info...",
        "Settings...",
    ];
    assert_eq!(more.matches("command_button(").count(), curated_more.len());
    for label in curated_more {
        assert!(more.contains(&format!("command_button(\"{label}\"")));
    }
    assert!(!more.contains("Open URL..."));
    assert!(!more.contains("Clear History..."));
    assert!(!more.contains("Zoom in"));
    assert!(!more.contains("Deinterlace"));

    for label in [
        "Open URL...",
        "Open Folder...",
        "Open Playlist...",
        "Add to Queue...",
        "Play Next...",
        "Save Playlist...",
        "Settings...",
        "Media Info...",
        "Open File Location",
        "Go to Time...",
        "Copy Current Time",
        "Add Bookmark",
        "A-B loop",
        "Zoom in",
        "Zoom out",
        "Pan left",
        "Pan right",
        "Pan up",
        "Pan down",
        "Center image",
        "Rotate 90°",
        "Fill screen (crop bars)",
        "Deinterlace",
        "Reset video",
        "Save frame",
        "Save frame with subtitles",
        "Copy frame to clipboard",
        "Close Media",
        "Mini player",
        "Clear History...",
    ] {
        assert!(
            advanced.contains(label),
            "missing advanced command: {label}"
        );
    }
    for implementation_marker in [
        "VideoAspect::ALL",
        "VideoGeometryAction::SetAspect",
        "VideoGeometryAction::ZoomIn",
        "VideoGeometryAction::ToggleDeinterlace",
        "if video_available",
        "video_transform.action_enabled(true, action)",
        "No video track",
        "Open video to use geometry",
        "Enter Fullscreen",
        "Exit Fullscreen",
        "Private Session On",
        "Private Session Off",
        "repeat_mode_label(repeat_mode)",
        "Shuffle On",
        "Shuffle Off",
        "Auto-advance On",
        "Auto-advance Off",
    ] {
        assert!(
            advanced.contains(implementation_marker),
            "missing advanced command family: {implementation_marker}"
        );
    }

    let player_clicks = include_str!("mpv_bridge.rs");
    assert!(player_clicks.contains("context_click.set_button(gdk::BUTTON_SECONDARY)"));
    assert!(player_clicks.contains("context_root.pick(x, y, gtk::PickFlags::INSENSITIVE)"));
    assert!(player_clicks.contains("player_context_menu_target_is_interactive("));
    assert!(player_clicks.contains("show_player_context_menu("));
    assert!(player_clicks.contains("popover.set_parent(player_root)"));
    assert!(player_clicks.contains("connect_popover_chrome_pin(&popover, chrome)"));
    assert!(player_clicks.contains("advanced_command_popover_content("));

    let window = include_str!("window.rs");
    assert!(window.contains("connect_player_context_menu("));
}

#[test]
fn video_geometry_toasts_report_the_applied_core_state() {
    let mut geometry = VideoGeometry::default();
    geometry.apply(VideoGeometryAction::ZoomIn);
    assert_eq!(
        video_geometry_message(VideoGeometryAction::ZoomIn, geometry),
        "Zoom: 125%"
    );

    geometry.apply(VideoGeometryAction::RotateClockwise);
    assert_eq!(
        video_geometry_message(VideoGeometryAction::RotateClockwise, geometry),
        "Rotation: 90°"
    );

    geometry.apply(VideoGeometryAction::ToggleDeinterlace);
    assert_eq!(
        video_geometry_message(VideoGeometryAction::ToggleDeinterlace, geometry),
        "Deinterlace on"
    );
}

#[test]
fn compact_mode_keeps_the_render_surface_and_restores_standard_chrome() {
    let compact = include_str!("compact_mode.rs");
    for required in [
        "view-restore-symbolic",
        "window-close-symbolic",
        "media-playback-start-symbolic",
        "okp-compact-seek",
        "COMPACT_DEFAULT_SHORT_EDGE",
        "COMPACT_MIN_SHORT_EDGE",
        "set_window_always_on_top(&self.window, true)",
        ".set_visible(!compact && !self.window.is_fullscreen())",
        "self.standard_osc.set_visible(!compact)",
        "close_current_media(&close_state, &close_toast)",
        "restore_compact_mode",
    ] {
        assert!(
            compact.contains(required),
            "missing compact seam: {required}"
        );
    }
    assert!(!compact.contains("Mpv::new"));
    assert!(!compact.contains("load_media_path"));

    let window = include_str!("window.rs");
    assert!(window.contains("connect_mpv(&video_host"));
    assert!(window.contains("CompactMode::build("));
    assert_eq!(window.matches("connect_mpv(&video_host").count(), 1);

    let css = include_str!("css.rs");
    for required in [
        "border-radius: 14px;",
        "min-width: 28px;",
        "min-height: 28px;",
        "background: rgba(22, 22, 25, 0.56);",
        "background: @okp_accent;",
        "min-width: 80px;",
        "min-height: 20px;",
        "font-feature-settings: 'tnum';",
        "is-reduced-transparency",
        "is-high-contrast",
    ] {
        assert!(css.contains(required), "missing compact CSS: {required}");
    }
}

#[test]
fn player_context_menu_preserves_control_and_popover_interactions() {
    let player_clicks = include_str!("mpv_bridge.rs");
    for blocker in [
        "gtk::Button",
        "gtk::MenuButton",
        "gtk::Scale",
        "gtk::Scrollbar",
        "gtk::Entry",
        "gtk::TextView",
        "gtk::SpinButton",
        "gtk::DropDown",
        "gtk::Switch",
        "gtk::ListBoxRow",
        "gtk::Popover",
        "okp-time-label",
        "okp-timeline",
        "okp-volume-control",
        "okp-up-next-panel",
        "okp-resize-handle",
    ] {
        assert!(
            player_clicks.contains(blocker),
            "missing context-menu interaction blocker: {blocker}"
        );
    }
    assert!(player_clicks.contains("gesture.set_state(gtk::EventSequenceState::Claimed)"));
    assert!(player_clicks.contains("gtk::PropagationPhase::Bubble"));
    assert!(!player_clicks.contains("video_area.add_controller(context_click)"));
}

#[test]
fn subtitle_delay_projection_drives_quick_popover_and_settings_refresh() {
    // Both visible surfaces retain the exact projected delay instead of
    // immediately replacing it with the asynchronous mpv observer snapshot.
    let quick_popover = include_str!("track_popovers.rs");
    assert!(quick_popover.contains("button_delay.set(applied_delay)"));
    assert!(quick_popover.contains("format_label(applied_delay)"));

    let settings = include_str!("settings_pages.rs");
    assert!(settings.contains("projected_delay.set(applied_delay)"));
    assert!(settings.contains("format_label(projected_delay.get())"));

    let target = subtitle_delay_target(-0.25, SubtitleAdjustment::Delay(0.05))
        .expect("delay adjustment should produce an exact target");
    assert_eq!(target, -0.2);

    let next_target = subtitle_delay_target(target, SubtitleAdjustment::Delay(0.05))
        .expect("rapid delay adjustments should accumulate from the projection");
    okp_test_fixtures::assert_close(next_target, -0.15, f64::EPSILON);
}

#[test]
fn subtitle_presentation_surfaces_stay_curated_and_width_safe() {
    let settings = include_str!("settings_pages.rs");
    for choice in [
        "Small",
        "Normal",
        "Large",
        "Standard",
        "Raised",
        "High contrast",
    ] {
        assert!(
            settings.contains(choice),
            "missing Settings choice {choice}"
        );
    }
    assert!(settings.contains("okp-settings-segmented"));
    assert!(!settings.contains("sub-border-size"));
    assert!(!settings.contains("sub-back-color"));

    let popover = include_str!("track_popovers.rs");
    assert!(popover.contains("compact_subtitle_size_row"));
    assert!(popover.contains("compact_subtitle_style_row"));
    assert!(popover.contains("subtitle_preset_status_label"));
    assert!(popover.contains("More in Settings → Subtitles"));
    assert_eq!(subtitle_style_label("Default"), "Default");
    assert_eq!(subtitle_style_label("Contrast"), "High contrast");
    assert_eq!(format_scale(1.4), "140%");

    let css = include_str!("css.rs");
    assert!(css.contains(".okp-quick-style-row"));
    assert!(css.contains(".okp-subtitle-preset-status"));
    assert!(css.contains(".okp-settings-hint"));
}

#[test]
fn subtitle_preset_surfaces_explain_native_supported_and_fallback_states() {
    use okp_core::subtitle_tracks::{SubtitlePresetApplicability, SubtitlePresetFormat};

    let ass = SubtitlePresetApplicability::NativeStyle(SubtitlePresetFormat::Ass);
    assert_eq!(
        subtitle_preset_status_text(ass),
        "ASS native style; OK Player preset is not applied."
    );
    assert!(settings_subtitle_preset_hint(ass).contains("authored native styling"));

    let srt = SubtitlePresetApplicability::Applies(SubtitlePresetFormat::SubRip);
    assert_eq!(
        subtitle_preset_status_text(srt),
        "OK Player preset applies to this SRT track."
    );
    assert!(settings_subtitle_preset_hint(srt).contains("uses the selected"));

    let unknown = SubtitlePresetApplicability::Unsupported(SubtitlePresetFormat::Unknown);
    assert_eq!(
        subtitle_preset_status_text(unknown),
        "Style support is unavailable for this subtitle format."
    );
    assert!(settings_subtitle_preset_hint(unknown).contains("unavailable"));

    assert_eq!(
        subtitle_preset_status_text(SubtitlePresetApplicability::NoActiveTrack),
        "Select a subtitle track to use style presets."
    );
}

#[test]
fn subtitle_presentation_overrides_persist_the_applied_values() {
    let target = subtitle_delay_target(-0.25, SubtitleAdjustment::Delay(0.05))
        .expect("delay adjustment should produce an exact target");

    let observed = history::PlaybackPreferences {
        subtitle_delay: Some(-0.25),
        subtitle_scale: Some(1.2),
        audio_delay: Some(0.1),
        ..history::PlaybackPreferences::default()
    };

    let persisted = playback_preferences_with_overrides(observed, Some(target), Some(1.4), None);

    assert_eq!(persisted.subtitle_delay, Some(-0.2));
    assert_eq!(persisted.subtitle_scale, Some(1.4));
    assert_eq!(persisted.audio_delay, Some(0.1));
}

#[test]
fn new_source_load_resets_global_subtitle_scale_before_dispatch() {
    let mut mpv = Mpv::new().expect("libmpv must be loadable for the source-boundary test");
    mpv.start_event_pump();
    mpv.set_subtitle_scale(1.4)
        .expect("per-file subtitle size should apply");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
    while (mpv.observed_subtitle_scale() - 1.4).abs() >= 0.005
        && std::time::Instant::now() < deadline
    {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!((mpv.observed_subtitle_scale() - 1.4).abs() < 0.005);

    let mut dispatched = false;
    load_new_source_with_global_subtitle_scale(&mpv, 1.0, |mpv| {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
        while (mpv.observed_subtitle_scale() - 1.0).abs() >= 0.005
            && std::time::Instant::now() < deadline
        {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!((mpv.observed_subtitle_scale() - 1.0).abs() < 0.005);
        dispatched = true;
        Ok(())
    })
    .expect("source load should dispatch after the reset");

    assert!(dispatched);
}

#[test]
fn player_popovers_have_scoped_presenter_classes() {
    assert_eq!(PlayerPopoverKind::Speed.css_class(), "okp-speed-popover");
    assert_eq!(
        PlayerPopoverKind::Subtitles.css_class(),
        "okp-subtitle-popover"
    );
    assert_eq!(PlayerPopoverKind::Audio.css_class(), "okp-audio-popover");
    assert_eq!(PlayerPopoverKind::More.css_class(), "okp-more-popover");
    assert_eq!(
        PlayerPopoverKind::AdvancedCommands.css_class(),
        "okp-advanced-command-popover"
    );
}

#[test]
fn subtitle_track_label_distinguishes_webvtt_srt_and_native_styled_sources() {
    let webvtt = Track {
        id: 3,
        kind: TrackKind::Subtitle,
        selected: true,
        external: true,
        external_filename: Some("/tmp/example.vtt".to_owned()),
        default: false,
        title: Some("English".to_owned()),
        lang: Some("eng".to_owned()),
        codec: Some("webvtt".to_owned()),
        audio_channels: None,
    };
    assert_eq!(track_label(&webvtt), "English · WebVTT · EXT");

    let srt = Track {
        id: 4,
        codec: Some("subrip".to_owned()),
        ..webvtt.clone()
    };
    assert_eq!(track_label(&srt), "English · SRT · EXT");

    let embedded = Track {
        id: 5,
        external: false,
        external_filename: None,
        default: true,
        codec: Some("ass".to_owned()),
        ..webvtt
    };
    assert_eq!(
        track_label(&embedded),
        "English · ASS · Native style · Default"
    );

    let external_ssa = Track {
        id: 6,
        external: true,
        external_filename: Some("/tmp/example.ssa".to_owned()),
        default: false,
        codec: Some("ass".to_owned()),
        ..embedded
    };
    assert_eq!(
        track_label(&external_ssa),
        "English · SSA · Native style · EXT"
    );
}

#[test]
fn subtitle_track_label_names_and_flags_image_tracks() {
    // A PGS Blu-ray track the shell wires through the core classifier: named
    // cleanly and flagged Image so the picker never reads it as a text track the
    // appearance presets could restyle.
    let pgs = Track {
        id: 3,
        kind: TrackKind::Subtitle,
        selected: false,
        external: false,
        external_filename: None,
        default: true,
        title: Some("English SDH".to_owned()),
        lang: Some("eng".to_owned()),
        codec: Some("hdmv_pgs_subtitle".to_owned()),
        audio_channels: None,
    };
    assert_eq!(track_label(&pgs), "English SDH · PGS · Image · Default");

    let vobsub = Track {
        id: 4,
        title: None,
        lang: Some("fre".to_owned()),
        external: true,
        external_filename: Some("/tmp/movie.fr.sub".to_owned()),
        default: false,
        codec: Some("dvd_subtitle".to_owned()),
        ..pgs.clone()
    };
    assert_eq!(track_label(&vobsub), "fre · VobSub · Image · EXT");

    // A neighbouring SRT track keeps its text label with no Image flag.
    let srt = Track {
        id: 5,
        codec: Some("subrip".to_owned()),
        ..pgs
    };
    assert!(!track_label(&srt).contains("Image"));
}

#[test]
fn audio_track_label_surfaces_language_and_format_tags() {
    // A named commentary track keeps its title but now also exposes the
    // language code alongside the channel layout and codec.
    let commentary = Track {
        id: 2,
        kind: TrackKind::Audio,
        selected: false,
        external: false,
        external_filename: None,
        default: false,
        title: Some("Director's Commentary".to_owned()),
        lang: Some("eng".to_owned()),
        codec: Some("ac3".to_owned()),
        audio_channels: Some("2.0".to_owned()),
    };
    assert_eq!(
        track_label(&commentary),
        "Director's Commentary · ENG · 2.0 · AC3"
    );

    // An untitled foreign track falls back to its language code as the name and
    // does not repeat it as a trailing tag.
    let untitled = Track {
        id: 3,
        kind: TrackKind::Audio,
        selected: true,
        external: false,
        external_filename: None,
        default: false,
        title: None,
        lang: Some("jpn".to_owned()),
        codec: Some("aac".to_owned()),
        audio_channels: Some("5.1".to_owned()),
    };
    assert_eq!(track_label(&untitled), "jpn · 5.1 · AAC");
}

#[test]
fn audio_delay_toast_mirrors_the_subtitle_readout_with_its_own_label() {
    assert_eq!(audio_delay_toast(0.25), "Audio delay: +250 ms");
    assert_eq!(audio_delay_toast(-0.125), "Audio delay: -125 ms");
    assert_eq!(audio_delay_toast(0.0), "Audio delay: 0 ms");
}

#[test]
fn side_panel_preview_sample_covers_chapters_and_queue() {
    let sample = side_panel_preview_sample();

    assert!(sample.has_media);
    let current_file = sample
        .current_file
        .clone()
        .expect("preview has current file");

    // The Chapters surface must exercise the current-chapter state and a long
    // title that has to ellipsize inside the panel.
    let current = sample
        .current_chapter
        .expect("preview marks a current chapter");
    assert!(current < sample.chapters.len());
    assert!(
        sample.chapters.iter().any(|chapter| chapter
            .title
            .as_deref()
            .is_some_and(|title| title.len() > 40)),
        "a long chapter name should be present to prove ellipsize",
    );

    // The Up Next surface must exercise the played-behind, now-playing, and next
    // rows, so the current item sits in the middle of the queue (not first), and
    // the queue mixes local files with a stream URL for the file/URL treatment.
    let current_index = sample
        .playlist
        .iter()
        .position(|item| item.is_current(sample.current_file.as_deref(), None))
        .expect("current file is in the queue");
    assert!(
        current_index > 0 && current_index + 1 < sample.playlist.len(),
        "current item should have both a behind row and a next row around it",
    );
    assert_eq!(
        sample.playlist[current_index],
        PlaylistItem::Local(current_file)
    );
    assert!(
        sample
            .playlist
            .iter()
            .any(|item| matches!(item, PlaylistItem::Local(_)))
    );
    assert!(
        sample
            .playlist
            .iter()
            .any(|item| matches!(item, PlaylistItem::Url(_)))
    );
}

#[test]
fn side_panel_redline_matches_the_shipped_player_contract() {
    assert_eq!(SIDE_PANEL_WIDTH, 316);
    assert_eq!(SIDE_PANEL_TOP_INSET, 44);
    assert_eq!(SIDE_PANEL_BOTTOM_INSET, 80);
    assert_eq!(SIDE_PANEL_TRANSITION_MS, 250);
}

#[test]
fn side_panel_empty_up_next_sample_covers_the_short_queue_state() {
    // The PRD §2.6 "Empty (single URL / no folder)" state is what the Up Next
    // panel renders for a stream with no folder queue: the lone now-playing URL
    // pinned plus the "Add files to queue" affordance. The fixture must be a
    // single-item URL queue with no chapters and no bookmarks so the visual
    // smoke shot exercises exactly that short-queue path (and not the multi-item
    // queue or the chapters surface).
    let sample = side_panel_empty_up_next_sample();

    assert!(sample.has_media);
    assert!(sample.current_file.is_none());
    let url = sample
        .current_url
        .clone()
        .expect("preview is a stream with a current url");
    assert_eq!(sample.playlist.len(), 1, "short-queue fixture has one item");
    assert_eq!(sample.playlist[0], PlaylistItem::Url(url.clone()));
    assert!(sample.playlist[0].is_current(None, Some(url.as_str())));
    assert!(
        sample.chapters.is_empty(),
        "short-queue fixture has no chapters"
    );
    assert!(
        sample.bookmarks.is_empty(),
        "short-queue fixture has no bookmarks"
    );
    assert!(sample.current_chapter.is_none());
}

#[test]
fn side_panel_bookmarks_sample_keeps_bookmarks_in_the_first_viewport() {
    let sample = side_panel_bookmarks_sample();

    assert_eq!(sample.chapters.len(), 2);
    assert_eq!(sample.current_chapter, Some(1));
    assert_eq!(sample.bookmarks.len(), 3);
    assert!(sample.current_file.is_some());
}

#[test]
fn side_panel_empty_chapters_sample_supports_the_bookmark_affordance() {
    let sample = side_panel_empty_chapters_sample();

    assert!(sample.has_media);
    assert!(sample.current_file.is_some());
    assert!(sample.chapters.is_empty());
    assert!(sample.bookmarks.is_empty());
    assert_eq!(sample.playlist.len(), 1);
}

#[test]
fn lyrics_preview_sample_is_a_synced_sheet_with_a_mid_active_line() {
    let (document, position) = lyrics_preview_sample();

    assert!(
        document.has_timings,
        "preview fixture must be a synced sheet"
    );
    assert_eq!(document.lines.len(), 8);
    assert_eq!(document.title.as_deref(), Some("Neon Skyline"));
    assert_eq!(document.artist.as_deref(), Some("The Wander Club"));

    // The smoke screenshot relies on a mid-sheet line being the active (brightened) one, so the
    // fixture proves the surface has lines both above and below the highlight.
    let active =
        lrc::active_index(&document.lines, position).expect("preview position lands on a line");
    assert!(
        active > 0 && active + 1 < document.lines.len(),
        "active line should sit mid-sheet, got {active}",
    );
    assert_eq!(document.lines[active].text, "A neon skyline out of sight");
}

#[test]
fn lyrics_surface_gates_on_local_audio_only() {
    // The overlay reveals for a local audio file, and never for video, a stream, or no media — so
    // the video-first player stays untouched.
    let audio = Rc::new(RefCell::new(PlayerState {
        current_file: Some(PathBuf::from("/music/song.flac")),
        ..PlayerState::default()
    }));
    assert_eq!(
        current_audio_path(&audio),
        Some(PathBuf::from("/music/song.flac"))
    );

    let video = Rc::new(RefCell::new(PlayerState {
        current_file: Some(PathBuf::from("/films/movie.mkv")),
        ..PlayerState::default()
    }));
    assert!(current_audio_path(&video).is_none());

    let stream = Rc::new(RefCell::new(PlayerState {
        current_url: Some("https://example.com/live.mp3".to_owned()),
        ..PlayerState::default()
    }));
    assert!(current_audio_path(&stream).is_none());

    assert!(current_audio_path(&Rc::new(RefCell::new(PlayerState::default()))).is_none());
}

#[test]
fn sidecar_lyrics_next_to_audio_parse_as_a_synced_sheet() {
    // A local audio file with a matching `.lrc` resolves to synchronized lyrics (the shell's
    // discover → parse expression, end to end on disk).
    let dir = unique_temp_dir("okp-gtk-lyrics-synced");
    fs::create_dir_all(&dir).expect("temp dir");
    let media = dir.join("Song.mp3");
    fs::write(&media, b"x").expect("media file");
    fs::write(dir.join("Song.lrc"), "[00:01.00]one\n[00:02.50]two\n").expect("sidecar");

    let document = okp_core::lyrics::read_sidecar(&media)
        .map(|text| lrc::parse(Some(&text)))
        .unwrap_or_default();

    assert!(document.has_timings);
    assert_eq!(document.lines.len(), 2);
    assert_eq!(document.lines[1].text, "two");

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn missing_sidecar_resolves_to_an_empty_document_for_the_empty_state() {
    // A local audio file with no sidecar yields the calm empty state, never debug text: the
    // discover → parse expression produces the empty document `rebuild` renders as "No lyrics".
    let dir = unique_temp_dir("okp-gtk-lyrics-empty");
    fs::create_dir_all(&dir).expect("temp dir");
    let media = dir.join("Instrumental.flac");
    fs::write(&media, b"x").expect("media file");

    let document = okp_core::lyrics::read_sidecar(&media)
        .map(|text| lrc::parse(Some(&text)))
        .unwrap_or_default();

    assert!(document.is_empty());
    assert!(!document.has_timings);

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn ab_loop_message_describes_cycle_state() {
    assert_eq!(
        ab_loop_message(
            AbLoopState {
                a: Some(12.0),
                b: None,
            },
            false,
        ),
        Some("A-B loop: start at 00:12".to_owned())
    );
    assert_eq!(
        ab_loop_message(
            AbLoopState {
                a: Some(12.0),
                b: Some(42.0),
            },
            true,
        ),
        Some("A-B loop: 00:12 - 00:42".to_owned())
    );
    assert_eq!(
        ab_loop_message(AbLoopState::default(), true),
        Some("A-B loop cleared".to_owned())
    );
    assert_eq!(ab_loop_message(AbLoopState::default(), false), None);
}

#[test]
fn audio_device_restore_skips_auto_or_blank_devices() {
    assert!(!should_restore_audio_device(""));
    assert!(!should_restore_audio_device("  "));
    assert!(!should_restore_audio_device("auto"));
    assert!(should_restore_audio_device("pulse/alsa_output"));
}

#[test]
fn audio_device_restore_retry_is_bounded() {
    let pending = PendingAudioDeviceRestore::new("pulse/device".to_owned());

    let pending = next_audio_device_restore_retry(pending, 3).expect("first miss should retry");
    assert_eq!(pending.attempts, 1);

    let pending = next_audio_device_restore_retry(pending, 3).expect("second miss should retry");
    assert_eq!(pending.attempts, 2);

    assert_eq!(next_audio_device_restore_retry(pending, 3), None);
}

#[test]
fn playlist_save_path_adds_default_extension_only_when_missing() {
    assert_eq!(
        playlist_save_path(PathBuf::from("/tmp/OK Player Playlist")).as_path(),
        Path::new("/tmp/OK Player Playlist.m3u")
    );
    assert_eq!(
        playlist_save_path(PathBuf::from("/tmp/list.m3u8")).as_path(),
        Path::new("/tmp/list.m3u8")
    );
}

#[test]
fn playlist_path_detects_m3u_variants() {
    assert!(is_playlist_path(Path::new("/tmp/list.m3u")));
    assert!(is_playlist_path(Path::new("/tmp/list.M3U8")));
    assert!(!is_playlist_path(Path::new("/tmp/movie.mkv")));
}

#[test]
fn m3u_playlist_items_keep_urls_and_skip_subtitles_unknown_entries() {
    let entries = vec![
        "/media/ep2.mkv".to_owned(),
        "https://example.test/ep3.mp4".to_owned(),
        "/media/captions.srt".to_owned(),
        "/media/readme.txt".to_owned(),
        "/media/ep1.mp4".to_owned(),
    ];

    let items = playlist_items_from_m3u_entries(&entries);

    assert_eq!(
        items,
        vec![
            local_item("/media/ep2.mkv"),
            url_item("https://example.test/ep3.mp4"),
            local_item("/media/ep1.mp4")
        ]
    );
}

#[test]
fn launch_args_keep_ordered_media_urls_and_subtitles() {
    let launch = parse_launch_args_from(
        [
            "/media/b.mkv",
            "https://example.test/a.mp4",
            "/media/b.mkv",
            "/media/captions.srt",
            "--sub",
            "/media/forced.ass",
            "/media/readme.txt",
        ]
        .into_iter()
        .map(Into::into),
    );

    assert_eq!(
        launch.items,
        vec![
            local_item("/media/b.mkv"),
            url_item("https://example.test/a.mp4")
        ]
    );
    assert_eq!(
        launch.subtitles,
        vec![
            PathBuf::from("/media/captions.srt"),
            PathBuf::from("/media/forced.ass")
        ]
    );
    assert!(launch.playlists.is_empty());
    assert_eq!(launch.directives, LaunchDirectives::default());
}

#[test]
fn launch_args_parse_explicit_resume_and_track_hints_without_treating_ids_as_files() {
    let launch = parse_launch_args_from(
        [
            "--resume=1:30",
            "/media/movie.mkv",
            "--sub",
            "3",
            "--audio=2",
        ]
        .into_iter()
        .map(Into::into),
    );

    assert_eq!(launch.items, vec![local_item("/media/movie.mkv")]);
    assert_eq!(
        launch.directives,
        LaunchDirectives {
            resume_seconds: Some(90.0),
            subtitle: Some(launch_args::TrackSelection::Id(3)),
            audio: Some(launch_args::TrackSelection::Id(2)),
        }
    );
    assert!(launch.subtitles.is_empty());
}

#[test]
fn launch_subtitle_file_flag_remains_compatible_with_track_hint_parser() {
    let launch = parse_launch_args_from(
        ["/media/movie.mkv", "--sub", "/media/forced.ass"]
            .into_iter()
            .map(Into::into),
    );

    assert_eq!(launch.items, vec![local_item("/media/movie.mkv")]);
    assert_eq!(launch.subtitles, vec![PathBuf::from("/media/forced.ass")]);
    assert_eq!(launch.directives.subtitle, None);
}

#[test]
fn launch_args_decode_file_uris_and_detect_playlists() {
    let launch = parse_launch_args_from(
        [
            "file:///tmp/OK%20Player/movie.mkv",
            "file:///tmp/OK%20Player/list.m3u8",
            "file:///tmp/OK%20Player/subs.vtt",
        ]
        .into_iter()
        .map(Into::into),
    );

    assert_eq!(launch.items, vec![local_item("/tmp/OK Player/movie.mkv")]);
    assert_eq!(
        launch.playlists,
        vec![PathBuf::from("/tmp/OK Player/list.m3u8")]
    );
    assert_eq!(
        launch.subtitles,
        vec![PathBuf::from("/tmp/OK Player/subs.vtt")]
    );
}

#[test]
fn launch_args_resolve_relative_paths_against_command_line_cwd() {
    let launch = parse_launch_args_from_cwd(
        ["movie.mkv", "--sub", "subs.srt"]
            .into_iter()
            .map(Into::into),
        Some(Path::new("/tmp/OK Player")),
    );

    assert_eq!(launch.items, vec![local_item("/tmp/OK Player/movie.mkv")]);
    assert_eq!(
        launch.subtitles,
        vec![PathBuf::from("/tmp/OK Player/subs.srt")]
    );
}

#[test]
fn load_launch_args_uses_explicit_playlist_for_multiple_items() {
    let state = Rc::new(RefCell::new(PlayerState::default()));
    let launch = LaunchArgs {
        items: vec![
            local_item("/media/a.mkv"),
            url_item("https://example.test/b.mp4"),
        ],
        playlists: Vec::new(),
        subtitles: Vec::new(),
        directives: LaunchDirectives::default(),
        reserved_notices: Vec::new(),
    };

    assert!(load_launch_args(&state, &launch));

    let state = state.borrow();
    assert_eq!(state.current_file, Some(PathBuf::from("/media/a.mkv")));
    assert_eq!(state.current_url, None);
    assert_eq!(state.playlist.items(), launch.items.as_slice());
}

#[test]
fn explicit_launch_resume_overrides_remembered_position_for_one_open_only() {
    let path = PathBuf::from("/media/resume-precedence.mkv");
    let state = Rc::new(RefCell::new(PlayerState::default()));
    state
        .borrow_mut()
        .history
        .record(&path, 240.0, 600.0, false);

    state.borrow_mut().next_launch_directives = Some(LaunchDirectives {
        resume_seconds: Some(90.0),
        ..LaunchDirectives::default()
    });
    remember_loaded_media_with_playlist(
        &state,
        path.clone(),
        vec![PlaylistItem::Local(path.clone())],
    );
    let explicit = state.borrow().pending_resume.expect("explicit resume");
    assert_eq!(
        explicit.target.origin,
        launch_args::ResumeOrigin::ExplicitLaunch
    );
    assert_eq!(explicit.target.seconds, 90.0);

    // A later ordinary open has no leftover launch directive and resolves the unchanged history.
    remember_loaded_media_with_playlist(&state, path.clone(), vec![PlaylistItem::Local(path)]);
    let remembered = state.borrow().pending_resume.expect("remembered resume");
    assert_eq!(
        remembered.target.origin,
        launch_args::ResumeOrigin::Remembered
    );
    assert_eq!(remembered.target.seconds, 240.0);
}

#[test]
fn unique_media_paths_keeps_order_and_skips_non_media_duplicates() {
    let paths = vec![
        PathBuf::from("/media/a.mkv"),
        PathBuf::from("/media/a.mkv"),
        PathBuf::from("/media/subs.srt"),
        PathBuf::from("/media/b.flac"),
        PathBuf::from("/media/readme.txt"),
    ];

    assert_eq!(
        unique_media_paths(paths),
        vec![
            PathBuf::from("/media/a.mkv"),
            PathBuf::from("/media/b.flac")
        ]
    );
}

#[test]
fn selected_media_paths_keep_selection_order_and_skip_non_media() {
    let paths = vec![
        PathBuf::from("/media/b.mkv"),
        PathBuf::from("/media/subs.srt"),
        PathBuf::from("/media/a.mp4"),
        PathBuf::from("/media/b.mkv"),
        PathBuf::from("/media/list.m3u"),
    ];

    assert_eq!(
        selected_media_paths(&paths),
        vec![PathBuf::from("/media/b.mkv"), PathBuf::from("/media/a.mp4")]
    );
}

#[test]
fn selected_subtitle_paths_keep_order_and_deduplicate() {
    let paths = vec![
        PathBuf::from("/media/a.en.srt"),
        PathBuf::from("/media/movie.mkv"),
        PathBuf::from("/media/a.en.srt"),
        PathBuf::from("/media/a.signs.ass"),
    ];

    assert_eq!(
        selected_subtitle_paths(&paths),
        vec![
            PathBuf::from("/media/a.en.srt"),
            PathBuf::from("/media/a.signs.ass")
        ]
    );
}

#[test]
fn selected_playlist_path_picks_first_m3u_variant() {
    let paths = vec![
        PathBuf::from("/media/movie.mkv"),
        PathBuf::from("/media/queue.m3u8"),
        PathBuf::from("/media/other.m3u"),
    ];

    assert_eq!(
        selected_playlist_path(&paths),
        Some(PathBuf::from("/media/queue.m3u8"))
    );
}

#[test]
fn native_file_dialog_result_preserves_selected_local_path_order() {
    let files = gtk::gio::ListStore::new::<gtk::gio::File>();
    files.append(&gtk::gio::File::for_path("/media/Episode 2.mkv"));
    files.append(&gtk::gio::File::for_path("/media/Episode 10.mkv"));

    assert_eq!(
        native_file_dialog_paths(Ok(files.upcast())),
        NativeFileDialogResult::Selected(vec![
            PathBuf::from("/media/Episode 2.mkv"),
            PathBuf::from("/media/Episode 10.mkv"),
        ])
    );
}

#[test]
fn native_file_dialog_result_rejects_empty_or_non_local_selections() {
    let empty = gtk::gio::ListStore::new::<gtk::gio::File>();
    assert!(matches!(
        native_file_dialog_paths(Ok(empty.upcast())),
        NativeFileDialogResult::Failed(error) if error.contains("empty selection")
    ));

    let remote = gtk::gio::ListStore::new::<gtk::gio::File>();
    remote.append(&gtk::gio::File::for_uri("https://example.com/movie.mkv"));
    assert!(matches!(
        native_file_dialog_paths(Ok(remote.upcast())),
        NativeFileDialogResult::Failed(error) if error.contains("local filesystem path")
    ));
}

#[test]
fn native_file_dialog_result_treats_cancel_and_dismiss_as_no_op() {
    for error in [
        glib::Error::new(gtk::DialogError::Cancelled, "cancelled"),
        glib::Error::new(gtk::DialogError::Dismissed, "dismissed"),
        glib::Error::new(gtk::gio::IOErrorEnum::Cancelled, "cancelled"),
    ] {
        assert_eq!(
            native_file_dialog_paths(Err(error)),
            NativeFileDialogResult::Cancelled
        );
    }
}

#[test]
fn native_file_dialog_result_keeps_failures_visible_to_the_caller() {
    let result = native_file_dialog_paths(Err(glib::Error::new(
        gtk::DialogError::Failed,
        "portal unavailable",
    )));

    assert!(matches!(
        result,
        NativeFileDialogResult::Failed(error) if error.contains("portal unavailable")
    ));
}

#[test]
fn selected_media_paths_expands_folders_in_natural_order() {
    let root = unique_temp_dir("okp-folder-selection");
    fs::create_dir_all(&root).expect("test folder should be created");
    let first = root.join("Episode 1.mp4");
    let second = root.join("Episode 2.mkv");
    let tenth = root.join("Episode 10.mkv");
    fs::write(&tenth, []).expect("test media should be created");
    fs::write(&first, []).expect("test media should be created");
    fs::write(root.join("Episode 2.srt"), []).expect("test subtitle should be created");
    fs::write(&second, []).expect("test media should be created");
    fs::write(root.join("cover.jpg"), []).expect("test ignored file should be created");

    assert_eq!(
        selected_media_paths(std::slice::from_ref(&root)),
        vec![first, second, tenth]
    );

    fs::remove_dir_all(root).expect("test folder should be removed");
}

#[test]
fn selected_media_paths_expands_multiple_folders_in_selection_order() {
    let root = unique_temp_dir("okp-folder-multi-selection");
    let season_one = root.join("Season 1");
    let season_two = root.join("Season 2");
    fs::create_dir_all(&season_one).expect("first test folder should be created");
    fs::create_dir_all(&season_two).expect("second test folder should be created");
    let s1e1 = season_one.join("Episode 1.mkv");
    let s1e2 = season_one.join("Episode 2.mkv");
    let s2e1 = season_two.join("Episode 1.mkv");
    let s2e10 = season_two.join("Episode 10.mkv");
    fs::write(&s1e2, []).expect("test media should be created");
    fs::write(&s1e1, []).expect("test media should be created");
    fs::write(&s2e10, []).expect("test media should be created");
    fs::write(&s2e1, []).expect("test media should be created");

    assert_eq!(
        selected_media_paths(&[season_two, season_one]),
        vec![s2e1, s2e10, s1e1, s1e2]
    );

    fs::remove_dir_all(root).expect("test folders should be removed");
}

#[test]
fn load_selected_local_paths_uses_explicit_playlist_for_multiple_media() {
    let state = Rc::new(RefCell::new(PlayerState::default()));
    let paths = vec![
        PathBuf::from("/media/b.mkv"),
        PathBuf::from("/media/subs.srt"),
        PathBuf::from("/media/a.mp4"),
        PathBuf::from("/media/b.mkv"),
    ];

    assert!(load_selected_local_paths(&state, paths));

    let state = state.borrow();
    assert_eq!(state.current_file, Some(PathBuf::from("/media/b.mkv")));
    assert_eq!(
        state.playlist.items(),
        [local_item("/media/b.mkv"), local_item("/media/a.mp4")]
    );
}

#[test]
fn load_selected_local_paths_preserves_folder_playlist_for_single_media() {
    let root = unique_temp_dir("okp-selection");
    fs::create_dir_all(&root).expect("test folder should be created");
    let first = root.join("Episode 1.mkv");
    let second = root.join("Episode 2.mkv");
    let subtitle = root.join("Episode 2.srt");
    fs::write(&first, []).expect("test media should be created");
    fs::write(&second, []).expect("test media should be created");
    fs::write(&subtitle, []).expect("test subtitle should be created");

    let state = Rc::new(RefCell::new(PlayerState::default()));

    assert!(load_selected_local_paths(&state, vec![second.clone()]));

    let state_ref = state.borrow();
    assert_eq!(state_ref.current_file, Some(second.clone()));
    assert_eq!(
        state_ref.playlist.items(),
        [
            PlaylistItem::Local(first.clone()),
            PlaylistItem::Local(second.clone())
        ]
    );
    drop(state_ref);

    fs::remove_dir_all(root).expect("test folder should be removed");
}

#[test]
fn load_selected_local_paths_opens_folder_as_playlist() {
    let root = unique_temp_dir("okp-folder-load");
    fs::create_dir_all(&root).expect("test folder should be created");
    let first = root.join("Episode 1.mkv");
    let second = root.join("Episode 2.mkv");
    fs::write(&second, []).expect("test media should be created");
    fs::write(&first, []).expect("test media should be created");

    let state = Rc::new(RefCell::new(PlayerState::default()));

    assert!(load_selected_local_paths(&state, vec![root.clone()]));

    let state_ref = state.borrow();
    assert_eq!(state_ref.current_file, Some(first.clone()));
    assert_eq!(
        state_ref.playlist.items(),
        [
            PlaylistItem::Local(first.clone()),
            PlaylistItem::Local(second.clone())
        ]
    );
    drop(state_ref);

    fs::remove_dir_all(root).expect("test folder should be removed");
}

#[test]
fn playlist_drop_target_index_maps_before_after_slots() {
    assert_eq!(playlist_drop_target_index(0, 2, false), Some(1));
    assert_eq!(playlist_drop_target_index(0, 2, true), Some(2));
    assert_eq!(playlist_drop_target_index(3, 1, false), Some(1));
    assert_eq!(playlist_drop_target_index(3, 1, true), Some(2));
}

#[test]
fn playlist_drop_target_index_rejects_self_or_existing_slot() {
    assert_eq!(playlist_drop_target_index(2, 2, false), None);
    assert_eq!(playlist_drop_target_index(2, 2, true), None);
    assert_eq!(playlist_drop_target_index(1, 2, false), None);
    assert_eq!(playlist_drop_target_index(2, 1, true), None);
}

#[test]
fn deb_checksum_download_refuses_release_without_manifest() {
    let update = DebUpdate {
        version: "0.1.0-linux-alpha.46".to_owned(),
        name: "ok-player_0.1.0-linux-alpha.46_amd64.deb".to_owned(),
        url: "https://example.invalid/update.deb".to_owned(),
        size: Some(42),
        sums_url: None,
    };

    let error = download_deb_checksums(&update).expect_err("missing manifest should refuse");
    assert!(
        error.contains("does not publish SHA256SUMS"),
        "unexpected error: {error}"
    );
}

#[test]
fn staged_deb_with_one_flipped_byte_is_refused_and_discarded() {
    let cache_dir = unique_temp_dir("okp-deb-verify-tamper");
    fs::create_dir_all(&cache_dir).expect("cache dir should be created");
    let name = "ok-player_0.1.0-linux-alpha.46_amd64.deb";
    let payload = b"pretend this is a .deb archive".to_vec();
    let manifest = format!("{}  {name}\n", sha256sums::sha256_hex(&payload));
    let mut tampered = payload.clone();
    tampered[payload.len() / 2] ^= 0x01;

    let error = stage_verified_deb(&tampered, &manifest, name, &cache_dir)
        .expect_err("tampered payload should be refused");

    assert!(
        error.contains("Update integrity check failed"),
        "unexpected error: {error}"
    );
    assert!(
        error.contains("sha256 mismatch"),
        "unexpected error: {error}"
    );
    // Neither the staged temp file nor the install target may survive:
    // the install path only ever hands a successfully returned target
    // path to pkexec, and nothing verifiable-as-bad is left behind.
    assert!(!cache_dir.join(name).exists());
    assert!(!cache_dir.join(format!("{name}.part")).exists());
    fs::remove_dir_all(&cache_dir).expect("cache dir should be removed");
}

#[test]
fn staged_deb_tampered_on_disk_after_write_is_refused() {
    let cache_dir = unique_temp_dir("okp-deb-verify-disk");
    fs::create_dir_all(&cache_dir).expect("cache dir should be created");
    let name = "ok-player_0.1.0-linux-alpha.46_amd64.deb";
    let payload = b"pretend this is a .deb archive".to_vec();
    let manifest = format!("{}  {name}\n", sha256sums::sha256_hex(&payload));
    let staged = cache_dir.join(name);
    fs::write(&staged, &payload).expect("staged payload should be written");

    assert!(verify_staged_deb(&staged, name, &manifest).is_ok());

    let mut tampered = payload.clone();
    tampered[0] ^= 0x01;
    fs::write(&staged, &tampered).expect("tampered payload should be written");

    let error = verify_staged_deb(&staged, name, &manifest)
        .expect_err("on-disk tampering should be refused");
    assert!(
        error.contains("sha256 mismatch"),
        "unexpected error: {error}"
    );
    fs::remove_dir_all(&cache_dir).expect("cache dir should be removed");
}

#[test]
fn staged_deb_matching_manifest_is_finalized() {
    let cache_dir = unique_temp_dir("okp-deb-verify-ok");
    fs::create_dir_all(&cache_dir).expect("cache dir should be created");
    let name = "ok-player_0.1.0-linux-alpha.46_amd64.deb";
    let payload = b"pretend this is a .deb archive".to_vec();
    let manifest = format!(
        "{}  {name}\n{}  OK-Player-0.1.0-x86_64.AppImage\n",
        sha256sums::sha256_hex(&payload),
        sha256sums::sha256_hex(b"another asset")
    );

    let target = stage_verified_deb(&payload, &manifest, name, &cache_dir)
        .expect("verified payload should be staged");

    assert_eq!(target, cache_dir.join(name));
    assert_eq!(
        fs::read(&target).expect("target should be readable"),
        payload
    );
    assert!(!cache_dir.join(format!("{name}.part")).exists());
    fs::remove_dir_all(&cache_dir).expect("cache dir should be removed");
}

#[test]
fn deb_update_action_requests_install() {
    let update = PendingLinuxUpdate {
        manager: None,
        target: LinuxUpdateTarget::Deb(DebUpdate {
            version: "0.1.0-linux-alpha.46".to_owned(),
            name: "ok-player_0.1.0-linux-alpha.46_amd64.deb".to_owned(),
            url: "https://example.invalid/update.deb".to_owned(),
            size: Some(42),
            sums_url: None,
        }),
    };

    assert_eq!(update.action_label(), "Install .deb");
    assert_eq!(update.available_status(), "Available: 0.1.0-linux-alpha.46");
}

#[test]
fn linux_update_status_reflects_last_check_result() {
    let up_to_date = LinuxUpdateStatus::from_check_result(&LinuxUpdateCheckResult::UpToDate);
    assert_eq!(
        up_to_date.settings_status_text(true),
        "OK Player is up to date"
    );
    assert_eq!(up_to_date.action_label(), "Check for updates");
    assert!(up_to_date.pending_update().is_none());

    let update = PendingLinuxUpdate {
        manager: None,
        target: LinuxUpdateTarget::Deb(DebUpdate {
            version: "0.1.0-linux-alpha.46".to_owned(),
            name: "ok-player_0.1.0-linux-alpha.46_amd64.deb".to_owned(),
            url: "https://example.invalid/update.deb".to_owned(),
            size: Some(42),
            sums_url: None,
        }),
    };
    let available =
        LinuxUpdateStatus::from_check_result(&LinuxUpdateCheckResult::Available(update));
    assert_eq!(
        available.settings_status_text(true),
        "Available: 0.1.0-linux-alpha.46"
    );
    assert_eq!(available.action_label(), "Install .deb");
    assert!(available.pending_update().is_some());

    let failed =
        LinuxUpdateStatus::from_check_result(&LinuxUpdateCheckResult::Failed("no feed".into()));
    assert_eq!(
        failed.settings_status_text(true),
        "Update check failed: no feed"
    );
}

#[test]
fn fine_seek_readout_pairs_projected_timecode_with_frame_number() {
    // Fine seek forward on a 24 fps clip: the shared projection lands on the
    // exact target and the toast reports the matching frame number, exactly as
    // `seek_relative_with_readout` composes it from the observed snapshot.
    let playback = PlaybackState {
        time_pos: Some(30.0),
        duration: Some(120.0),
        container_fps: Some(24.0),
        ..PlaybackState::default()
    };
    let time = playback.time_pos.unwrap_or(0.0).max(0.0);
    let duration = playback.duration.unwrap_or(0.0).max(0.0);
    let target = seek_readout::seek_target(time, 5.0, duration);

    assert_eq!(target, 35.0);
    assert_eq!(
        seek_readout::format_readout(target, playback.container_fps),
        "00:35 · Frame 840"
    );
}

#[test]
fn frame_step_readout_reports_the_next_frame() {
    // Frame-forward from frame 120 (@24 fps) reports frame 121, mirroring
    // `frame_step_with_readout`.
    let playback = PlaybackState {
        time_pos: Some(5.0),
        duration: Some(120.0),
        container_fps: Some(24.0),
        ..PlaybackState::default()
    };
    let time = playback.time_pos.unwrap_or(0.0).max(0.0);
    let duration = playback.duration.unwrap_or(0.0).max(0.0);
    let target = seek_readout::frame_step_target(time, playback.container_fps, true, duration);

    assert_eq!(
        seek_readout::format_readout(target, playback.container_fps),
        "00:05 · Frame 121"
    );
}

#[test]
fn audio_only_seek_readout_shows_timecode_without_a_frame() {
    // Audio-only source: no container fps, so the readout is a bare timecode
    // and never emits a dangling "Frame" label.
    let playback = PlaybackState {
        time_pos: Some(58.0),
        duration: Some(240.0),
        container_fps: None,
        ..PlaybackState::default()
    };
    let time = playback.time_pos.unwrap_or(0.0).max(0.0);
    let duration = playback.duration.unwrap_or(0.0).max(0.0);
    let target = seek_readout::seek_target(time, 5.0, duration);

    assert_eq!(
        seek_readout::format_readout(target, playback.container_fps),
        "01:03"
    );
}

#[test]
fn deb_self_install_timeout_uses_positive_override_only() {
    assert_eq!(
        parse_deb_self_install_timeout(Some("5")),
        Duration::from_secs(5)
    );
    assert_eq!(
        parse_deb_self_install_timeout(Some("0")),
        DEB_SELF_INSTALL_TIMEOUT
    );
    assert_eq!(
        parse_deb_self_install_timeout(Some("soon")),
        DEB_SELF_INSTALL_TIMEOUT
    );
    assert_eq!(
        parse_deb_self_install_timeout(None),
        DEB_SELF_INSTALL_TIMEOUT
    );
}

#[test]
fn format_duration_unknown_shows_live_sentinel_in_shell() {
    // The shell wires the duration total through the core helper, so a stream that
    // never reports a duration renders the live `--:--` instead of the broken `00:00`.
    assert_eq!(time_code::format_duration(None), "--:--");
    assert_eq!(time_code::format_duration(Some(0.0)), "--:--");
    assert_eq!(time_code::format_duration(Some(f64::NAN)), "--:--");
    assert_eq!(time_code::format_duration(Some(90.0)), "01:30");
}

#[test]
fn is_live_or_unknown_duration_only_for_urls_without_duration() {
    // The shell decides progress-only / live readout from this predicate (pure core).
    assert!(network_media::is_live_or_unknown_duration(true, None));
    assert!(!network_media::is_live_or_unknown_duration(
        true,
        Some(120.0)
    ));
    assert!(!network_media::is_live_or_unknown_duration(false, None));
}

#[test]
fn load_url_transitions_to_loading_and_records_retry_url() {
    // Handing a URL to the engine flips the surface to Loading and remembers the URL
    // for the in-canvas failure card's Retry action.
    let state = Rc::new(RefCell::new(PlayerState::default()));
    remember_loaded_url_with_playlist(
        &state,
        "https://example.com/live.m3u8".to_owned(),
        vec![url_item("https://example.com/live.m3u8")],
    );

    let state = state.borrow();
    assert_eq!(
        state.media_load_state,
        network_media::MediaLoadState::Loading
    );
    assert_eq!(
        state.retry_load_source.as_ref(),
        Some(&network_media::LoadFailureSource::url(
            "https://example.com/live.m3u8"
        ))
    );
    assert!(state.last_load_error.is_none());
}

#[test]
fn load_path_transitions_to_loading_and_records_retry_path() {
    // Handing a local file to the engine uses the same Loading surface and records
    // the path for the in-canvas failure card's Retry action.
    let state = Rc::new(RefCell::new(PlayerState::default()));
    let path = PathBuf::from("/media/movie.mkv");
    remember_loaded_media_with_playlist(
        &state,
        path.clone(),
        vec![PlaylistItem::Local(path.clone())],
    );

    let state = state.borrow();
    assert_eq!(
        state.media_load_state,
        network_media::MediaLoadState::Loading
    );
    assert_eq!(
        state.retry_load_source.as_ref(),
        Some(&network_media::LoadFailureSource::local(path))
    );
    assert!(state.last_load_error.is_none());
}

#[test]
fn source_generation_advances_on_reload_and_clear() {
    let state = Rc::new(RefCell::new(PlayerState::default()));

    remember_loaded_url(&state, "https://example.com/live.m3u8".to_owned());
    assert_eq!(state.borrow().source_generation, 1);

    remember_loaded_url(&state, "https://example.com/live.m3u8".to_owned());
    assert_eq!(state.borrow().source_generation, 2);

    clear_loaded_media_state(&state);
    let state = state.borrow();
    assert_eq!(state.source_generation, 3);
    assert_eq!(state.seek_generation, 0);
}

#[test]
fn initial_window_fit_is_consumed_once_per_source_generation() {
    let state = Rc::new(RefCell::new(PlayerState::default()));
    let dimensions = VideoDimensions {
        width: 3840,
        height: 2160,
    };

    remember_loaded_url(&state, "https://example.com/movie.mp4".to_owned());
    assert!(observe_initial_window_fit(&state, Some(dimensions)));
    assert_eq!(
        take_initial_window_fit(&state),
        Some(window_fit::InitialFitRequest {
            source_generation: 1,
            video: window_fit::WindowSize {
                width: 3840,
                height: 2160,
            },
        })
    );
    assert!(!observe_initial_window_fit(&state, Some(dimensions)));
    assert_eq!(take_initial_window_fit(&state), None);

    remember_loaded_url(&state, "https://example.com/movie.mp4".to_owned());
    assert!(observe_initial_window_fit(&state, Some(dimensions)));
    assert_eq!(
        take_initial_window_fit(&state).map(|request| request.source_generation),
        Some(2)
    );
}

#[test]
fn missing_file_loaded_dimensions_do_not_consume_initial_window_fit() {
    let state = Rc::new(RefCell::new(PlayerState::default()));
    let dimensions = VideoDimensions {
        width: 1920,
        height: 1080,
    };

    remember_loaded_url(&state, "https://example.com/live.m3u8".to_owned());
    assert!(!observe_initial_window_fit(&state, None));
    assert_eq!(take_initial_window_fit(&state), None);
    assert!(observe_initial_window_fit(&state, Some(dimensions)));
    assert_eq!(
        take_initial_window_fit(&state).map(|request| request.video),
        Some(window_fit::WindowSize {
            width: 1920,
            height: 1080,
        })
    );
    assert!(!observe_initial_window_fit(&state, Some(dimensions)));
    assert_eq!(take_initial_window_fit(&state), None);
}

#[test]
fn set_load_failure_transitions_and_records_card_actions() {
    let state = Rc::new(RefCell::new(PlayerState::default()));
    set_load_failure(
        &state,
        "https://example.com/missing.mp4".to_owned(),
        "libmpv error 404".to_owned(),
    );

    let state = state.borrow();
    assert_eq!(
        state.media_load_state,
        network_media::MediaLoadState::Failed
    );
    assert_eq!(
        state.retry_load_source.as_ref(),
        Some(&network_media::LoadFailureSource::url(
            "https://example.com/missing.mp4"
        ))
    );
    assert_eq!(state.last_load_error.as_deref(), Some("libmpv error 404"));
}

#[test]
fn set_local_load_failure_records_retry_path() {
    // A local-file failure transitions the in-canvas surface and records the
    // path so Retry replays that same file.
    let state = Rc::new(RefCell::new(PlayerState::default()));
    let path = PathBuf::from("/media/movie.mkv");
    set_local_load_failure(&state, path.clone(), "libmpv error 7".to_owned());

    let state = state.borrow();
    assert_eq!(
        state.media_load_state,
        network_media::MediaLoadState::Failed
    );
    assert_eq!(
        state.retry_load_source.as_ref(),
        Some(&network_media::LoadFailureSource::local(path))
    );
    assert_eq!(state.last_load_error.as_deref(), Some("libmpv error 7"));
}

#[test]
fn set_local_load_failure_replaces_stale_url_from_a_previous_load() {
    // A URL loaded earlier arms Retry for the stream. If a later local-file
    // `load_path` returns `Err`, the local failure must replace the old URL.
    let state = Rc::new(RefCell::new(PlayerState::default()));
    set_load_failure(
        &state,
        "https://example.com/live.m3u8".to_owned(),
        "libmpv error 412".to_owned(),
    );
    assert!(state.borrow().retry_load_source.is_some());

    let path = PathBuf::from("/media/movie.mkv");
    set_local_load_failure(&state, path.clone(), "libmpv error 7".to_owned());

    let state = state.borrow();
    assert_eq!(
        state.media_load_state,
        network_media::MediaLoadState::Failed
    );
    assert_eq!(
        state.retry_load_source.as_ref(),
        Some(&network_media::LoadFailureSource::local(path))
    );
    assert_eq!(state.last_load_error.as_deref(), Some("libmpv error 7"));
}

#[test]
fn apply_endfile_error_marks_current_source_failed() {
    // A fresh EndFile::Error for the current URL transitions the surface to Failed
    // and stores the short reason for the in-canvas card.
    let state = Rc::new(RefCell::new(PlayerState {
        current_url: Some("https://example.com/live.m3u8".to_owned()),
        media_load_state: network_media::MediaLoadState::Loading,
        ..PlayerState::default()
    }));

    apply_endfile_error(&state, 412, Some("https://example.com/live.m3u8"));

    let state = state.borrow();
    assert_eq!(
        state.media_load_state,
        network_media::MediaLoadState::Failed
    );
    assert_eq!(
        state.retry_load_source.as_ref(),
        Some(&network_media::LoadFailureSource::url(
            "https://example.com/live.m3u8"
        ))
    );
    assert_eq!(state.last_load_error.as_deref(), Some("libmpv error 412"));
}

#[test]
fn apply_endfile_error_marks_current_local_source_failed() {
    // A fresh EndFile::Error for the current local file transitions the surface
    // to Failed and keeps the local source retryable.
    let path = PathBuf::from("/media/movie.mkv");
    let state = Rc::new(RefCell::new(PlayerState {
        current_file: Some(path.clone()),
        media_load_state: network_media::MediaLoadState::Loading,
        ..PlayerState::default()
    }));

    apply_endfile_error(&state, 7, Some("/media/movie.mkv"));

    let state = state.borrow();
    assert_eq!(
        state.media_load_state,
        network_media::MediaLoadState::Failed
    );
    assert_eq!(
        state.retry_load_source.as_ref(),
        Some(&network_media::LoadFailureSource::local(path))
    );
    assert_eq!(state.last_load_error.as_deref(), Some("libmpv error 7"));
}

#[test]
fn apply_endfile_error_drops_stale_error_for_a_superseded_source() {
    // URL A fails, then the user starts URL B before the next poll drains the
    // queue. A's stale EndFile::Error carries A's path; the current source is B,
    // so the error must not fail B or replace the card detail with A's reason.
    let state = Rc::new(RefCell::new(PlayerState {
        current_url: Some("https://example.com/b.m3u8".to_owned()),
        media_load_state: network_media::MediaLoadState::Loading,
        retry_load_source: Some(network_media::LoadFailureSource::url(
            "https://example.com/b.m3u8",
        )),
        ..PlayerState::default()
    }));

    apply_endfile_error(&state, 412, Some("https://example.com/a.m3u8"));

    let state = state.borrow();
    // The surface stays Loading for B and B's retry URL is untouched, so B's own
    // lifecycle (FileLoaded / its own EndFile) resolves it.
    assert_eq!(
        state.media_load_state,
        network_media::MediaLoadState::Loading
    );
    assert!(state.last_load_error.is_none());
    assert_eq!(
        state.retry_load_source.as_ref(),
        Some(&network_media::LoadFailureSource::url(
            "https://example.com/b.m3u8"
        ))
    );
}

#[test]
fn apply_endfile_error_drops_ended_path_when_no_source_is_current() {
    // A close can clear the current source before a late EndFile::Error is
    // drained. If the event still names a path, it is stale and must not reopen
    // the failed surface after the player returned to idle.
    let state = Rc::new(RefCell::new(PlayerState {
        media_load_state: network_media::MediaLoadState::Idle,
        ..PlayerState::default()
    }));

    apply_endfile_error(&state, 7, Some("/media/movie.mkv"));

    let state = state.borrow();
    assert_eq!(state.media_load_state, network_media::MediaLoadState::Idle);
    assert!(state.retry_load_source.is_none());
    assert!(state.last_load_error.is_none());
}

#[test]
fn apply_endfile_error_drops_missing_ended_path_when_no_source_is_current() {
    // mpv can omit the path on a late EndFile::Error. Once the player has no
    // current source, the missing path must not reopen a failed surface with no
    // retryable source.
    let state = Rc::new(RefCell::new(PlayerState {
        media_load_state: network_media::MediaLoadState::Idle,
        ..PlayerState::default()
    }));

    apply_endfile_error(&state, 7, None);

    let state = state.borrow();
    assert_eq!(state.media_load_state, network_media::MediaLoadState::Idle);
    assert!(state.retry_load_source.is_none());
    assert!(state.last_load_error.is_none());
}

#[test]
fn apply_endfile_error_falls_back_to_applying_when_path_is_missing() {
    // A missing path tag (mpv reported nothing to compare) falls back to applying
    // so the helper never under-reports a genuine failure just because the tag was
    // missing.
    let state = Rc::new(RefCell::new(PlayerState {
        current_url: Some("https://example.com/live.m3u8".to_owned()),
        media_load_state: network_media::MediaLoadState::Loading,
        ..PlayerState::default()
    }));

    apply_endfile_error(&state, 412, None);

    let state = state.borrow();
    assert_eq!(
        state.media_load_state,
        network_media::MediaLoadState::Failed
    );
    assert_eq!(
        state.retry_load_source.as_ref(),
        Some(&network_media::LoadFailureSource::url(
            "https://example.com/live.m3u8"
        ))
    );
    assert_eq!(state.last_load_error.as_deref(), Some("libmpv error 412"));
}

#[test]
fn clear_loaded_media_resets_load_state_surface() {
    let state = Rc::new(RefCell::new(PlayerState::default()));
    set_load_failure(
        &state,
        "https://example.com/live.m3u8".to_owned(),
        "libmpv error 412".to_owned(),
    );

    clear_loaded_media_state(&state);

    let state = state.borrow();
    assert_eq!(state.media_load_state, network_media::MediaLoadState::Idle);
    assert!(state.retry_load_source.is_none());
    assert!(state.last_load_error.is_none());
}

#[test]
fn load_failure_actions_offer_retry_open_another_and_copy_details() {
    // The ordered action row the failure card renders is the contract from the
    // core model — Retry first, Open another second, Copy details last.
    let labels: Vec<&'static str> = network_media::LOAD_FAILURE_ACTIONS
        .iter()
        .copied()
        .map(network_media::LoadFailureAction::label)
        .collect();
    assert_eq!(labels, ["Retry", "Open another", "Copy details"]);
}

#[test]
fn failure_detail_keeps_url_and_reason_out_of_primary_logs() {
    // The Copy details action writes a short summary, never raw internal logs.
    let source = network_media::LoadFailureSource::url("https://example.com/live.m3u8");
    assert_eq!(
        network_media::failure_detail(&source, "libmpv error 412"),
        "OK Player could not open the stream.\nURL: https://example.com/live.m3u8\nReason: libmpv error 412"
    );
    // An empty reason is omitted so a transient failure still copies a clean line.
    assert_eq!(
        network_media::failure_detail(&source, ""),
        "OK Player could not open the stream.\nURL: https://example.com/live.m3u8"
    );

    let source = network_media::LoadFailureSource::local("/media/movie.mkv");
    assert_eq!(
        network_media::failure_detail(&source, "libmpv error 7"),
        "OK Player could not open the media.\nPath: /media/movie.mkv\nReason: libmpv error 7"
    );
}

#[test]
fn video_double_click_fullscreen_contract_covers_each_gesture_state() {
    use fullscreen_toggle::{FullscreenAction, FullscreenToggle};
    use video_click::Intent;

    // Single click schedules the delayed play/pause and never toggles fullscreen.
    assert_eq!(video_click::release_intent(1), Intent::SchedulePlayPause);

    // A double click cancels that pending single click before toggling, so the
    // canvas never flashes a pause between the two presses.
    assert_eq!(
        video_click::release_intent(2),
        Intent::CancelPlayPauseAndToggleFullscreen
    );

    // A slow double click — two releases beyond the platform double-click window —
    // arrives as two independent single clicks (two play/pause), not a fullscreen
    // toggle. Both are honoured; neither is silently dropped.
    assert_eq!(video_click::release_intent(1), Intent::SchedulePlayPause);
    assert_eq!(video_click::release_intent(1), Intent::SchedulePlayPause);

    // A stationary double-click press stays under the move threshold, so the
    // second click still reaches the toggle instead of starting a window move.
    assert!(!video_click::drag_exceeds_move_threshold(0.0, 0.0, 6.0));
    assert!(video_click::drag_exceeds_move_threshold(6.0, 0.0, 6.0));

    // Entering and leaving fullscreen is decided from the eagerly-flipped intent,
    // so repeated double-clicks alternate even when the compositor's own state
    // has not caught up between them.
    let mut toggle = FullscreenToggle::new(false);
    assert_eq!(toggle.toggle(), FullscreenAction::Enter);
    assert_eq!(toggle.toggle(), FullscreenAction::Leave);

    // A maximized window is orthogonal: the fullscreen intent tracks only
    // fullscreen, and reconciles with the compositor's authoritative notify.
    toggle.observe(true);
    assert!(toggle.intended());
    assert_eq!(toggle.toggle(), FullscreenAction::Leave);
}

#[test]
fn fullscreen_toggle_wiring_decides_from_intent_not_the_lagging_platform_state() {
    // The double-click path routes through the intent policy, cancelling the
    // pending single click before toggling.
    let mpv_bridge = include_str!("mpv_bridge.rs");
    assert!(mpv_bridge.contains("video_click::release_intent(press_count)"));
    assert!(mpv_bridge.contains("toggle_fullscreen(&click_window, &click_state)"));

    // `toggle_fullscreen` decides Enter/Leave from the flipped intent rather than
    // reading the compositor's lagging `is_fullscreen`.
    let playback = include_str!("playback.rs");
    assert!(playback.contains("fullscreen_toggle.toggle()"));
    assert!(playback.contains("FullscreenAction::Enter => window.fullscreen()"));
    assert!(playback.contains("FullscreenAction::Leave => window.unfullscreen()"));

    // The `fullscreened` notify reconciles the intent so Escape / window-manager
    // toggles keep the next double-click honest.
    let window = include_str!("window.rs");
    assert!(window.contains(r#"connect_notify_local(Some("fullscreened")"#));
    assert!(window.contains(".observe(window.is_fullscreen())"));

    // The compact drag still promotes to a window move only past the shared
    // threshold, so a stationary double-click there never starts a move.
    let compact = include_str!("compact_mode.rs");
    assert!(compact.contains("video_click::drag_exceeds_move_threshold(offset_x, offset_y, 6.0)"));
}
