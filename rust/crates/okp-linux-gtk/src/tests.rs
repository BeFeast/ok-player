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
fn shared_linux_icon_remains_a_transparent_launcher_mark() {
    let packaging = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packaging/linux");
    let icon = fs::read_to_string(packaging.join("com.befeast.okplayer.svg"))
        .expect("shared app icon should be readable");
    assert!(icon.contains("<circle"));
    assert!(icon.contains("#28b3aa"));
    assert!(!icon.contains("<rect"), "icon must not bake in a tile");
    assert!(!icon.contains("<polygon"), "icon must not be a play glyph");
}

#[test]
fn linux_packaging_installs_only_the_shared_desktop_icon() {
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
        assert!(!contents.contains("com.befeast.okplayer.about.svg"));
    }
}

#[test]
fn settings_shell_matches_windows_reference_geometry() {
    assert_eq!(SETTINGS_REFERENCE_WIDTH, 760);
    assert_eq!(SETTINGS_REFERENCE_HEIGHT, 560);
    assert_eq!(SETTINGS_TITLEBAR_HEIGHT, 42);
    assert_eq!(SETTINGS_BODY_HEIGHT, 518);
    assert_eq!(SETTINGS_RAIL_WIDTH, 192);
    assert_eq!(SETTINGS_CONTENT_WIDTH, 568);
    assert_eq!(
        SETTINGS_RAIL_WIDTH + SETTINGS_CONTENT_WIDTH,
        SETTINGS_REFERENCE_WIDTH
    );
    assert_eq!(CAPTIONLESS_DRAG_HEIGHT, 42);
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

    // The visual smoke fixture must exercise the summary strip, the section
    // list, and the track list so screenshots catch regressions in each.
    assert!(sample.path.is_some());
    let section_titles: Vec<&str> = sample
        .sections
        .iter()
        .map(|section| section.title.as_str())
        .collect();
    assert!(section_titles.contains(&"File"));
    assert!(section_titles.contains(&"Video"));

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

    // The summary derives an HDR chip and the Video section carries the row it
    // condenses, so both the accent row and the chip stay covered.
    assert_eq!(
        media_info_value(&sample, "Video", "Dynamic Range").map(media_info_hdr_summary),
        Some("HDR".to_owned())
    );
    let chip_labels: Vec<&str> = media_info_summary_chips(&sample)
        .iter()
        .map(|(label, _)| *label)
        .collect();
    assert!(chip_labels.contains(&"HDR"));
}

#[test]
fn media_info_hdr_summary_keeps_leading_format_token() {
    // The live producer emits "HDR (transfer, primaries)"; the leading token
    // is what the chip shows.
    assert_eq!(media_info_hdr_summary("HDR (PQ / ST 2084, BT.2020)"), "HDR");
    assert_eq!(
        media_info_hdr_summary("HDR10 · BT.2020 · SMPTE ST 2084 (PQ)"),
        "HDR10"
    );
    assert_eq!(media_info_hdr_summary("Dolby Vision"), "Dolby Vision");
    assert_eq!(media_info_hdr_summary(""), "");
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
    assert_eq!(tracks[1].title, "Episode 2.mkv");
    assert_eq!(tracks[1].duration_us, Some(42_000_000));
    assert_eq!(
        mpris_tracklist_target_for_id(&state, tracks[1].id.as_str()),
        Some((1, PlaylistItem::Local(second.clone())))
    );

    let snapshot = mpris_snapshot_from_state(&state, None);
    assert_eq!(snapshot.current_track_id, Some(snapshot.track_id.clone()));
    assert!(snapshot.tracklist_track_ids().contains(&snapshot.track_id));
    assert!(mpris_metadata(&snapshot).contains_key("mpris:trackid"));
    assert!(mpris_track_metadata(&tracks[1]).contains_key("mpris:trackid"));

    state.current_file = Some(third);
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
        timeline_marks(&[], &[0.0, 90.0, f64::NAN], AbLoopState::default(), 300.0),
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
        timeline_marks(&chapters, &[900.0], AbLoopState::default(), 0.0),
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
fn track_label_shows_tags_without_a_selection_prefix() {
    // Selection is shown by the row's leading check, so the label must never
    // prepend "On " (which used to shift long titles and break alignment).
    let subtitle = Track {
        id: 3,
        kind: TrackKind::Subtitle,
        selected: true,
        external: true,
        default: false,
        title: Some("English (SDH)".to_owned()),
        lang: None,
        codec: None,
        audio_channels: None,
    };
    assert_eq!(track_label(&subtitle), "English (SDH) · EXT");

    let audio = Track {
        id: 1,
        kind: TrackKind::Audio,
        selected: true,
        external: false,
        default: true,
        title: Some("English".to_owned()),
        lang: None,
        codec: Some("eac3".to_owned()),
        audio_channels: Some("5.1".to_owned()),
    };
    assert_eq!(track_label(&audio), "English · 5.1 · EAC3");
}

#[test]
fn subtitle_track_label_distinguishes_webvtt_srt_and_embedded_sources() {
    let webvtt = Track {
        id: 3,
        kind: TrackKind::Subtitle,
        selected: true,
        external: true,
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
        default: true,
        codec: Some("ass".to_owned()),
        ..webvtt
    };
    assert_eq!(track_label(&embedded), "English · ASS · Default");
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
        reserved_notices: Vec::new(),
    };

    assert!(load_launch_args(&state, &launch));

    let state = state.borrow();
    assert_eq!(state.current_file, Some(PathBuf::from("/media/a.mkv")));
    assert_eq!(state.current_url, None);
    assert_eq!(state.playlist.items(), launch.items.as_slice());
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

/// Build a `PlayerState` whose only interesting field is the load-state surface, so the
/// network/live-stream transition tests read clearly without the full mpv/playlist setup.
fn load_state_fixture(load_state: network_media::MediaLoadState) -> Rc<RefCell<PlayerState>> {
    Rc::new(RefCell::new(PlayerState {
        media_load_state: load_state,
        ..PlayerState::default()
    }))
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
    // for the failure dialog's Retry action.
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
        state.last_load_url.as_deref(),
        Some("https://example.com/live.m3u8")
    );
    assert!(state.last_load_error.is_none());
    assert!(!state.load_failure_presented);
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
fn set_load_failure_transitions_and_rearms_dialog() {
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
        state.last_load_url.as_deref(),
        Some("https://example.com/missing.mp4")
    );
    assert_eq!(state.last_load_error.as_deref(), Some("libmpv error 404"));
    assert!(!state.load_failure_presented);
}

#[test]
fn set_local_load_failure_does_not_arm_url_dialog() {
    // A local-file failure transitions the surface (the overlay line) but leaves
    // last_load_url None, so the URL-only failure dialog never pops for it.
    let state = Rc::new(RefCell::new(PlayerState::default()));
    set_local_load_failure(&state, "libmpv error 7".to_owned());

    let state = state.borrow();
    assert_eq!(
        state.media_load_state,
        network_media::MediaLoadState::Failed
    );
    assert!(state.last_load_url.is_none());
    assert_eq!(state.last_load_error.as_deref(), Some("libmpv error 7"));
    assert!(!state.load_failure_presented);
}

#[test]
fn pending_load_failure_yields_url_once_then_guards() {
    // The first poll after a failure hands the dialog the URL + reason; subsequent
    // polls see the presented guard and do nothing, so the dialog pops once.
    let state = load_state_fixture(network_media::MediaLoadState::Failed);
    set_load_failure(
        &state,
        "https://example.com/live.m3u8".to_owned(),
        "libmpv error 412".to_owned(),
    );

    let first = pending_load_failure(&state);
    assert_eq!(first.as_ref().unwrap().0, "https://example.com/live.m3u8");
    assert_eq!(first.as_ref().unwrap().1, "libmpv error 412");

    // The guard is now set — a second poll within the same failure yields nothing.
    assert!(pending_load_failure(&state).is_none());
}

#[test]
fn pending_load_failure_skips_idle_loading_and_playing() {
    for state in [
        load_state_fixture(network_media::MediaLoadState::Idle),
        load_state_fixture(network_media::MediaLoadState::Loading),
        load_state_fixture(network_media::MediaLoadState::Playing),
    ] {
        assert!(
            pending_load_failure(&state).is_none(),
            "non-failure should not pop"
        );
    }
}

#[test]
fn set_local_load_failure_clears_stale_url_from_a_previous_load() {
    // A URL loaded earlier arms `last_load_url` for its Retry action. If a later
    // local-file `load_path` returns `Err`, the local failure must not leave the
    // old URL in place — otherwise the next poll would treat the local failure as
    // a URL failure and open Retry / Open another for the previous stream.
    let state = Rc::new(RefCell::new(PlayerState::default()));
    set_load_failure(
        &state,
        "https://example.com/live.m3u8".to_owned(),
        "libmpv error 412".to_owned(),
    );
    assert!(state.borrow().last_load_url.is_some());

    set_local_load_failure(&state, "libmpv error 7".to_owned());

    let state = state.borrow();
    assert_eq!(
        state.media_load_state,
        network_media::MediaLoadState::Failed
    );
    assert!(state.last_load_url.is_none());
    assert_eq!(state.last_load_error.as_deref(), Some("libmpv error 7"));
}

#[test]
fn apply_endfile_error_marks_current_source_failed() {
    // A fresh EndFile::Error for the current URL transitions the surface to Failed
    // and stores the short reason; the URL failure dialog pops once from the poll.
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
    assert_eq!(state.last_load_error.as_deref(), Some("libmpv error 412"));
    assert!(!state.load_failure_presented);
}

#[test]
fn apply_endfile_error_drops_stale_error_for_a_superseded_source() {
    // URL A fails, then the user starts URL B before the next poll drains the
    // queue. A's stale EndFile::Error carries A's path; the current source is B,
    // so the error must not fail B or arm the dialog with A's reason.
    let state = Rc::new(RefCell::new(PlayerState {
        current_url: Some("https://example.com/b.m3u8".to_owned()),
        media_load_state: network_media::MediaLoadState::Loading,
        last_load_url: Some("https://example.com/b.m3u8".to_owned()),
        ..PlayerState::default()
    }));

    apply_endfile_error(&state, 412, Some("https://example.com/a.m3u8"));

    let state = state.borrow();
    // The surface stays Loading for B; B's retry URL and the presented guard are
    // untouched, so B's own lifecycle (FileLoaded / its own EndFile) resolves it.
    assert_eq!(
        state.media_load_state,
        network_media::MediaLoadState::Loading
    );
    assert!(state.last_load_error.is_none());
    assert_eq!(
        state.last_load_url.as_deref(),
        Some("https://example.com/b.m3u8")
    );
    assert!(!state.load_failure_presented);
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
    assert_eq!(state.last_load_error.as_deref(), Some("libmpv error 412"));
}

#[test]
fn pending_load_failure_skips_local_failures_without_url() {
    // A local-file failure has no retry URL, so the URL-only dialog does not pop.
    let state = load_state_fixture(network_media::MediaLoadState::Failed);
    set_local_load_failure(&state, "libmpv error 7".to_owned());
    assert!(pending_load_failure(&state).is_none());
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
    assert!(state.last_load_url.is_none());
    assert!(state.last_load_error.is_none());
    assert!(!state.load_failure_presented);
}

#[test]
fn load_failure_actions_offer_retry_open_another_and_copy_details() {
    // The ordered action row the failure dialog renders is the contract from the
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
    assert_eq!(
        network_media::failure_detail("https://example.com/live.m3u8", "libmpv error 412"),
        "OK Player could not open the stream.\nURL: https://example.com/live.m3u8\nReason: libmpv error 412"
    );
    // An empty reason is omitted so a transient failure still copies a clean line.
    assert_eq!(
        network_media::failure_detail("https://example.com/live.m3u8", ""),
        "OK Player could not open the stream.\nURL: https://example.com/live.m3u8"
    );
}
