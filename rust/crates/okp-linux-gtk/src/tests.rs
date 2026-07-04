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
fn empty_surface_logo_resolves_to_the_bundled_app_icon() {
    // The welcome surface anchors the OK Player identity on the app icon tile.
    // If the bundled SVG stops resolving, the logo silently falls back to the
    // themed icon (blank outside an installed icon theme), so guard the asset.
    let path = empty_surface_logo_path().expect("welcome surface icon should resolve");
    assert!(path.is_file(), "resolved icon path should exist: {path:?}");
    assert_eq!(
        path.file_name().and_then(|name| name.to_str()),
        Some("com.befeast.okplayer.svg")
    );
}

#[test]
fn settings_shell_matches_windows_reference_geometry() {
    assert_eq!(SETTINGS_REFERENCE_WIDTH, 744);
    assert_eq!(SETTINGS_REFERENCE_HEIGHT, 1030);
    assert_eq!(SETTINGS_RAIL_WIDTH, 192);
    assert_eq!(SETTINGS_CONTENT_WIDTH, 552);
    assert_eq!(
        SETTINGS_RAIL_WIDTH + SETTINGS_CONTENT_WIDTH,
        SETTINGS_REFERENCE_WIDTH
    );
    assert_eq!(CAPTIONLESS_DRAG_HEIGHT, 32);
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
        timeline_marks(&[], &[0.0, 90.0, f64::NAN], AbLoopState::default()),
        vec![TimelineMark {
            time: 90.0,
            kind: TimelineMarkKind::Bookmark,
        }]
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
        ..LaunchArgs::default()
    };

    assert!(load_launch_args(&state, &launch));

    let state = state.borrow();
    assert_eq!(state.current_file, Some(PathBuf::from("/media/a.mkv")));
    assert_eq!(state.current_url, None);
    assert_eq!(state.playlist.items(), launch.items.as_slice());
}

#[test]
fn launch_args_parse_the_companion_resume_and_track_hints() {
    // PRD §13.1: `--resume` (timecode or seconds) plus `--sub`/`--audio` track ids.
    let launch = parse_launch_args_from(
        [
            "--resume",
            "1:30",
            "/media/movie.mkv",
            "--audio",
            "2",
            "--sub",
            "3",
        ]
        .into_iter()
        .map(Into::into),
    );

    assert_eq!(launch.items, vec![local_item("/media/movie.mkv")]);
    assert_eq!(launch.resume, Some(90.0));
    assert_eq!(launch.audio_track, Some(TrackSelection::Id(2)));
    assert_eq!(launch.sub_track, Some(TrackSelection::Id(3)));
    // `--sub 3` is a track hint, not a subtitle file.
    assert!(launch.subtitles.is_empty());
}

#[test]
fn launch_args_sub_with_a_path_stays_a_subtitle_file_not_a_track_hint() {
    let launch = parse_launch_args_from(
        ["/media/movie.mkv", "--sub", "/media/forced.ass"]
            .into_iter()
            .map(Into::into),
    );

    assert_eq!(launch.subtitles, vec![PathBuf::from("/media/forced.ass")]);
    assert_eq!(launch.sub_track, None);
}

#[test]
fn launch_track_hints_apply_over_the_pending_preferences() {
    let state = Rc::new(RefCell::new(PlayerState {
        current_file: Some(PathBuf::from("/media/a.mkv")),
        ..PlayerState::default()
    }));
    let launch = LaunchArgs {
        audio_track: Some(TrackSelection::Id(2)),
        sub_track: Some(TrackSelection::Off),
        ..LaunchArgs::default()
    };

    apply_launch_playback_overrides(&state, &launch);

    let state = state.borrow();
    let (path, prefs) = state
        .pending_preferences
        .as_ref()
        .expect("launch track hints should queue preferences");
    assert_eq!(path, &PathBuf::from("/media/a.mkv"));
    assert_eq!(prefs.audio_enabled, Some(true));
    assert_eq!(prefs.audio_track_id, Some(2));
    assert_eq!(prefs.subtitle_enabled, Some(false));
    assert_eq!(prefs.subtitle_track_id, None);
}

#[test]
fn explicit_launch_resume_is_queued_even_in_a_private_session() {
    // The companion, not history, asked for this position, so private mode does not gate it.
    let state = Rc::new(RefCell::new(PlayerState {
        current_file: Some(PathBuf::from("/media/a.mkv")),
        private_session: true,
        ..PlayerState::default()
    }));
    let launch = LaunchArgs {
        items: vec![local_item("/media/a.mkv")],
        resume: Some(42.0),
        ..LaunchArgs::default()
    };

    apply_launch_playback_overrides(&state, &launch);

    assert_eq!(
        state.borrow().pending_explicit_resume,
        Some((PathBuf::from("/media/a.mkv"), 42.0))
    );
}

#[test]
fn try_pending_resume_waits_when_duration_is_below_the_explicit_target() {
    // A provisional (small) duration must not consume the explicit target — it stays queued
    // for a later, larger duration (progressive / network media).
    let state = Rc::new(RefCell::new(PlayerState {
        current_file: Some(PathBuf::from("/media/a.mkv")),
        pending_explicit_resume: Some((PathBuf::from("/media/a.mkv"), 700.0)),
        ..PlayerState::default()
    }));

    try_pending_resume(&state, 600.0);

    assert_eq!(
        state.borrow().pending_explicit_resume,
        Some((PathBuf::from("/media/a.mkv"), 700.0))
    );
}

#[test]
fn try_pending_resume_drops_a_target_left_over_from_another_file() {
    let state = Rc::new(RefCell::new(PlayerState {
        current_file: Some(PathBuf::from("/media/a.mkv")),
        pending_explicit_resume: Some((PathBuf::from("/media/old.mkv"), 45.0)),
        ..PlayerState::default()
    }));

    try_pending_resume(&state, 600.0);

    assert!(state.borrow().pending_explicit_resume.is_none());
}

#[test]
fn try_pending_resume_discards_a_barely_started_remembered_position() {
    // 10s of 600s is under the 5% floor, so the remembered target is dropped, not applied.
    let state = Rc::new(RefCell::new(PlayerState {
        current_file: Some(PathBuf::from("/media/a.mkv")),
        pending_resume: Some((PathBuf::from("/media/a.mkv"), 10.0)),
        ..PlayerState::default()
    }));

    try_pending_resume(&state, 600.0);

    assert!(state.borrow().pending_resume.is_none());
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
    assert_eq!(up_to_date.about_status_text(), "Up to date");
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
        available.about_status_text(),
        "Available: 0.1.0-linux-alpha.46"
    );
    assert_eq!(
        available.settings_status_text(true),
        "Available: 0.1.0-linux-alpha.46"
    );
    assert_eq!(available.action_label(), "Install .deb");
    assert!(available.pending_update().is_some());

    let failed =
        LinuxUpdateStatus::from_check_result(&LinuxUpdateCheckResult::Failed("no feed".into()));
    assert_eq!(failed.about_status_text(), "Update check failed");
    assert_eq!(
        failed.settings_status_text(true),
        "Update check failed: no feed"
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
