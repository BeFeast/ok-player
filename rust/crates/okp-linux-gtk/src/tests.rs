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
fn renderer_environment_is_selected_before_gtk_initialization() {
    let source = include_str!("main.rs");
    let renderer = source
        .find("configure_linux_renderer_environment();")
        .expect("main should configure rendering");
    let gtk = source
        .find("VelopackApp::build()")
        .expect("main should initialize Velopack and GTK");

    assert!(renderer < gtk);
}

#[test]
fn file_association_launches_present_before_media_delivery() {
    let window = include_str!("window.rs");
    let build = window
        .split("pub(crate) fn build_window")
        .nth(1)
        .and_then(|source| source.split("pub(crate) enum VideoHost").next())
        .expect("build_window source");
    let map_hook = build
        .find("window.connect_map")
        .expect("startup map hook should be installed");
    let player_hook = build
        .find("connect_mpv(&video_host")
        .expect("player readiness hook should be installed");
    let present = build
        .find("window.present();")
        .expect("startup should present the GTK window");
    assert!(map_hook < present);
    assert!(player_hook < present);
    assert!(build.contains("StartupLaunchDelivery::new"));
    assert!(!build.contains("defer_initial_map"));
    assert!(!window.contains("set_opacity(0.0)"));

    let secondary = window
        .split("pub(crate) fn open_runtime_launch_args")
        .nth(1)
        .expect("secondary launch handler");
    let present = secondary.find("runtime.window.present();").unwrap();
    let load = secondary.find("apply_launch_args").unwrap();
    assert!(
        present < load,
        "secondary launches must present before loading"
    );

    let bridge = include_str!("mpv_bridge.rs");
    assert!(bridge.contains("mark_startup_window_mapped"));
    assert!(bridge.contains("mark_startup_player_ready"));
    assert!(bridge.contains("delivering after map and player readiness"));
}

#[test]
fn player_close_returns_to_gtk_before_mpv_teardown() {
    let keyboard = include_str!("keyboard.rs");
    let close_handler = keyboard
        .split("window.connect_close_request")
        .nth(1)
        .and_then(|source| source.split("glib::Propagation::Proceed").next())
        .expect("main-window close handler");

    assert!(close_handler.contains("close_companion_windows"));
    assert!(close_handler.contains("save_current_progress"));
    assert!(close_handler.contains("set_visible(false)"));
    assert!(close_handler.contains("close_app.quit()"));
    assert!(close_handler.contains("glib::idle_add_local_once"));

    let (before_idle, idle_body) = close_handler
        .split_once("glib::idle_add_local_once")
        .expect("close_request must defer engine teardown to an idle callback");
    for forbidden in ["mpv.stop(", "with_mpv("] {
        assert!(
            !before_idle.contains(forbidden),
            "close_request must not enter libmpv before the shell unmaps: {forbidden}"
        );
    }
    assert!(
        before_idle.contains("mpv.take()"),
        "close_request must take the engine before hide/unrealize"
    );
    assert!(
        before_idle.find("mpv.take()").expect("take")
            < before_idle.find("set_visible(false)").expect("hide"),
        "engine must leave PlayerState before set_visible triggers unrealize"
    );
    assert!(
        idle_body.find("close_app.quit()").expect("quit")
            < idle_body.find("mem::forget").expect("forget engine"),
        "idle close path must quit GTK before leaking the engine across process exit"
    );
}

#[test]
fn flatpak_detection_accepts_the_runtime_id_or_sandbox_marker() {
    use std::ffi::OsStr;

    assert!(flatpak_install_detected(
        Some(OsStr::new("com.befeast.okplayer")),
        false
    ));
    assert!(flatpak_install_detected(None, true));
    assert!(!flatpak_install_detected(None, false));
    assert!(!flatpak_install_detected(Some(OsStr::new("")), false));
}

#[test]
fn dri_probe_requires_an_openable_render_or_card_entry() {
    let root = unique_temp_dir("okp-dri-probe");
    assert!(!accessible_dri_device_exists(root.path()));

    fs::write(root.path().join("unrelated"), b"not a DRI node")
        .expect("unrelated fixture should be written");
    assert!(!accessible_dri_device_exists(root.path()));

    fs::write(root.path().join("renderD128"), b"test DRI node")
        .expect("render-node fixture should be written");
    assert!(accessible_dri_device_exists(root.path()));
}

#[test]
fn software_renderer_failure_reuses_the_in_player_error_surface() {
    let state = Rc::new(RefCell::new(PlayerState::default()));

    fail_software_renderer(&state, "software renderer test failure");

    let state = state.borrow();
    assert_eq!(
        state.media_load_state,
        network_media::MediaLoadState::Failed
    );
    let diagnostic = state
        .last_load_diagnostic
        .as_ref()
        .expect("renderer failure should drive the existing error card");
    assert_eq!(diagnostic.title, "Graphics access unavailable");
    assert!(diagnostic.message.contains("GPU/DRI"));
    assert!(diagnostic.message.contains("--device=dri"));
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
fn video_hardware_decode_uses_the_shared_settings_switch_geometry() {
    assert_eq!(SETTINGS_SWITCH_WIDTH, 39);
    assert_eq!(SETTINGS_SWITCH_HEIGHT, 22);
    assert_eq!(SETTINGS_SWITCH_KNOB_SIZE, 16);
    assert_eq!(SETTINGS_SWITCH_WIDTH + 3 * 2, 45);
    assert_eq!(SETTINGS_SWITCH_HEIGHT + 3 * 2, 28);

    let css = include_str!("css.rs");
    for rule in [
        "button.okp-settings-switch {",
        "min-width: 39px;",
        "min-height: 22px;",
        "padding: 3px;",
        ".okp-settings-switch-knob {",
        "min-width: 16px;",
        "min-height: 16px;",
    ] {
        assert!(css.contains(rule), "missing Settings switch rule: {rule}");
    }

    let video_settings = include_str!("settings_pages.rs");
    let hwdec_row = video_settings
        .split("pub(crate) fn settings_hwdec_row")
        .nth(1)
        .and_then(|source| {
            source
                .split("pub(crate) fn settings_hdr_handling_row")
                .next()
        })
        .expect("hardware decode row source should remain available");
    assert!(hwdec_row.contains("settings_switch_button(enabled, \"Hardware decode\")"));
    assert!(hwdec_row.contains("set_settings_switch_active(button, enabled)"));
    assert!(!hwdec_row.contains("gtk::Switch"));

    let playback_row = video_settings
        .split("pub(crate) fn settings_playback_switch_row")
        .nth(1)
        .and_then(|source| source.split("pub(crate) fn settings_repeat_row").next())
        .expect("playback switch row source should remain available");
    assert!(playback_row.contains("settings_switch_button(active, title)"));

    let updates = include_str!("updates.rs");
    assert!(updates.contains("settings_switch_button(auto_check_enabled, \"Automatic checks\")"));

    let switch = include_str!("settings_switch.rs");
    assert!(switch.contains("AccessibleRole::Switch"));
    assert!(switch.contains("accessible::State::Checked"));
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
fn bare_space_is_the_canonical_player_command_with_lock_masks_ignored() {
    assert!(is_canonical_player_space(
        gdk::Key::space,
        gdk::ModifierType::empty()
    ));
    assert!(is_canonical_player_space(
        gdk::Key::space,
        gdk::ModifierType::LOCK_MASK
    ));
    assert!(!is_canonical_player_space(
        gdk::Key::space,
        gdk::ModifierType::SHIFT_MASK
    ));
    assert!(!is_canonical_player_space(
        gdk::Key::Return,
        gdk::ModifierType::empty()
    ));
}

#[test]
fn player_space_capture_precedes_button_activation_and_preserves_editors() {
    let keyboard = include_str!("keyboard.rs");
    for required in [
        "gtk::PropagationPhase::Capture",
        "connect_key_released",
        "gtk::Editable",
        "gtk::TextView",
        "is-capturing",
        "toggle_play_pause(&state)",
        "glib::Propagation::Stop",
    ] {
        assert!(
            keyboard.contains(required),
            "missing Space contract: {required}"
        );
    }
    assert!(!keyboard.contains("mpv.cycle_pause()"));

    let settings = include_str!("settings_window.rs");
    assert!(settings.contains("connect_companion_play_pause_space"));
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
        assert!(contents.contains("okp_use_linux_bundled_mpv package"));
        assert!(contents.contains("verify-linux-bundled-mpv.sh"));
        assert!(contents.contains("OKP_BUNDLED_MPV_RUNTIME_DIR"));
    }

    let velopack = fs::read_to_string(root.join("scripts/package-linux-velopack.sh"))
        .expect("Velopack packaging script should be readable");
    assert!(velopack.contains("okp-candidate\" stage-velopack"));
    assert!(velopack.contains("--appimage-extract"));
    assert!(!velopack.contains("$PACK_ID.AppImage"));
    assert!(!velopack.contains("$PACK_ID-$VERSION-linux-full.nupkg"));
}

#[test]
fn linux_packages_pin_and_bundle_the_embedded_wayland_mpv() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let prepare = fs::read_to_string(root.join("scripts/prepare-linux-bundled-mpv.sh"))
        .expect("bundled mpv preparation script should be readable");
    let local_build = fs::read_to_string(root.join("scripts/build-local-mpv.sh"))
        .expect("local mpv build script should be readable");
    let verify = fs::read_to_string(root.join("scripts/verify-linux-bundled-mpv.sh"))
        .expect("bundled mpv verification script should be readable");
    let collect = fs::read_to_string(root.join("scripts/collect-linux-bundled-mpv-runtime.sh"))
        .expect("bundled mpv runtime collector should be readable");
    let portability = fs::read_to_string(root.join("scripts/verify-linux-package-portability.sh"))
        .expect("cross-distro portability gate should be readable");
    let deb = fs::read_to_string(root.join("scripts/package-linux-deb.sh"))
        .expect("Debian packaging script should be readable");
    let velopack_package = fs::read_to_string(root.join("scripts/package-linux-velopack.sh"))
        .expect("Velopack packaging script should be readable");
    let portable_builder = fs::read_to_string(root.join("scripts/build-linux-portable-package.sh"))
        .expect("portable package builder should be readable");
    let portable_image = fs::read_to_string(root.join("scripts/linux-portable-builder.Dockerfile"))
        .expect("portable package builder image should be readable");
    let candidate = fs::read_to_string(root.join("scripts/build-linux-candidate.sh"))
        .expect("candidate builder should be readable");

    assert!(prepare.contains("UPSTREAM_TAG=\"v0.40.0\""));
    assert!(prepare.contains("UPSTREAM_COMMIT=\"e48ac7ce08462f5e33af6ef9deeac6fa87eef01e\""));
    assert!(local_build.contains("mpv-v0.40.0-wayland-embed.patch"));
    assert!(local_build.contains("mpv-v0.40.0-ffmpeg-8.patch"));
    assert!(collect.contains("patchelf --set-rpath '$ORIGIN'"));
    assert!(collect.contains("bundled-runtime.sha256"));
    assert!(collect.contains("okp_is_linux_platform_runtime"));
    assert!(collect.contains("okp_is_linux_namespaced_media_source"));
    assert!(collect.contains("patchelf --replace-needed"));
    assert!(collect.contains("patchelf --set-soname"));
    assert!(collect.contains("Refusing to queue target platform library"));
    assert!(verify.contains("wayland-embed-display"));
    assert!(verify.contains("Packaged binary resolved libmpv outside its payload"));
    assert!(verify.contains("libavcodec.so"));
    assert!(verify.contains("libokp-libjpeg.so"));
    assert!(verify.contains("unnamespaced media runtime"));
    assert!(verify.contains("okp_verify_linux_bundled_runtime_manifest"));
    assert!(portability.contains("debian:testing-slim"));
    assert!(portability.contains("ubuntu:26.04"));
    assert!(portability.contains("no-bundled-glibc-runtime"));
    assert!(portability.contains("portability ldd:"));
    assert!(portability.contains("smoke-linux-narrow-width.sh"));
    assert!(portability.contains("smoke-linux-fullscreen-chrome.sh"));
    assert!(portability.contains("portability media render:"));
    assert!(portability.contains("appimage-media-fullscreen"));
    assert!(portability.contains("debian-media-fullscreen"));
    assert!(portability.contains("portability build marker:"));
    assert!(portability.contains("EXPECTED_BUILD_MARKER"));
    assert!(portability.contains("dpkg-deb -f \"/artifacts/deb/$DEB_NAME\" Depends"));
    assert!(portability.contains("apt-get satisfy -y --no-install-recommends \"$depends\""));
    let dependency_install = portability
        .find("apt-get satisfy -y --no-install-recommends \"$depends\"")
        .expect("package dependencies should be installed in the target container");
    let appimage_container_check = portability
        .find("check_elf_tree \"$APP_ROOT\"\n")
        .expect("AppImage ELF tree should be checked in the target container");
    assert!(dependency_install < appimage_container_check);
    assert!(!deb.contains("libmpv2"));
    assert!(deb.contains("Recommends: ffmpeg"));
    assert!(deb.contains("libasound2 | libasound2t64"));
    assert!(!deb.contains("libjpeg-turbo8 | libjpeg62-turbo | libjpeg8"));
    assert!(deb.contains("libwebp7"));
    assert!(deb.contains("libwebpmux3"));
    assert!(deb.contains("libpng16-16 | libpng16-16t64"));
    assert!(deb.contains("libxss1"));
    assert!(deb.contains("libx11-6"));
    assert!(deb.contains("libxcursor1"));
    assert!(deb.contains("libxext6"));
    assert!(deb.contains("libxfixes3"));
    assert!(deb.contains("libxi6"));
    assert!(deb.contains("libxrandr2"));
    assert!(deb.contains("libwayland-cursor0"));
    assert!(deb.contains("libxkbcommon0"));
    assert!(deb.contains("libdecor-0-0"));
    for dependency in [
        "libcairo-gobject2",
        "libcairo2",
        "libdbus-1-3",
        "libffi8",
        "libfontconfig1",
        "libfreetype6",
        "libfribidi0",
        "libgdk-pixbuf-2.0-0",
        "libharfbuzz0b",
        "libpango-1.0-0",
        "libpangocairo-1.0-0",
        "libsystemd0",
        "libudev1",
        "libx11-xcb1",
        "libxcb-dri3-0",
        "libxcb-shape0",
        "libxcb-shm0",
        "libxcb-xfixes0",
        "libxcb1",
        "libxpresent1",
        "libxv1",
    ] {
        assert!(
            deb.contains(dependency),
            "missing Debian dependency: {dependency}"
        );
    }
    assert!(deb.contains("OKP_CANDIDATE_TOOLCHAIN_GATE_SCRIPTS="));
    assert!(velopack_package.contains("OKP_CANDIDATE_TOOLCHAIN_GATE_SCRIPTS="));
    assert!(deb.contains("OKP_CANDIDATE_TOOLCHAIN_REQUIRE_DOTNET_TOOLS=false"));
    assert!(velopack_package.contains("OKP_CANDIDATE_TOOLCHAIN_REQUIRE_DOTNET_TOOLS=true"));
    assert!(portable_builder.contains("linux-portable-builder.Dockerfile"));
    assert!(portable_builder.contains("debian-13-v1"));
    assert!(portable_builder.contains("git -C \"$ROOT\" rev-parse --verify 'HEAD^{commit}'"));
    assert!(portable_builder.contains("-e OKP_BUILD_SHA=\"$BUILD_SHA\""));
    assert!(portable_builder.contains("--target \"$LANE\""));
    assert!(portable_image.contains(
        "FROM debian@sha256:9bb8a3626890e084ab54e888fdd7c4b6d2f119071cd4c5dc5fecb4d73062aa5f"
    ));
    let media_image = portable_image
        .split("FROM media AS deb")
        .next()
        .expect("portable media stage");
    assert!(media_image.contains("--print-portable-debian-packages"));
    assert!(!media_image.contains("dotnet-install.sh"));
    let appimage = portable_image
        .split("FROM media AS appimage")
        .nth(1)
        .expect("portable AppImage stage");
    assert!(appimage.contains("dotnet-install.sh"));
    assert!(appimage.contains("vpk --version 1.2.0"));
    assert!(candidate.contains("run_gate bundled-mpv okp_use_linux_bundled_mpv"));
    assert!(candidate.contains("run_gate portability-package-smoke"));
}

#[test]
fn embedded_wayland_mpv_keeps_the_retained_egl_plane_visible_during_resize() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let patch = fs::read_to_string(root.join("rust/patches/mpv-v0.40.0-wayland-embed.patch"))
        .expect("Wayland embed patch should be readable");

    assert!(patch.contains("vo_wayland_set_opaque_region(wl, wl->embedded);"));
    assert!(patch.contains("uint32_t alpha = vo->wl->embedded ? 0 : UINT32_MAX;"));
    assert!(patch.contains("uint32_t format = vo->wl->embedded ?"));
    assert!(patch.contains("WL_SHM_FORMAT_ARGB8888 : WL_SHM_FORMAT_XRGB8888;"));
    assert!(patch.contains("wl_subsurface_place_below(wl->embed_subsurface, wl->embed_parent);"));
}

#[test]
fn bundled_runtime_manifest_rejects_target_desktop_libraries() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let policy = root.join("scripts/linux-bundled-mpv-runtime-policy.sh");
    let temp = unique_temp_dir("okp-bundled-runtime-policy");
    let manifest = temp.path().join("bundled-runtime.sha256");
    let digest = "0".repeat(64);

    fs::write(
        &manifest,
        format!("{digest}  libmpv.so.2\n{digest}  libokp-libjpeg.so.62\n"),
    )
    .expect("media-only runtime manifest should be written");
    let accepted = std::process::Command::new("bash")
        .arg(&policy)
        .arg(&manifest)
        .output()
        .expect("runtime policy should run");
    assert!(
        accepted.status.success(),
        "{}",
        String::from_utf8_lossy(&accepted.stderr)
    );

    for platform_library in [
        "libX11.so.6",
        "libwayland-client.so.0",
        "libcairo.so.2",
        "libpango-1.0.so.0",
        "libfontconfig.so.1",
        "libmount.so.1",
        "libasound.so.2",
        "libasound_module_pcm_pulse.so",
        "libjpeg.so.62",
        "libjpeg.so.8",
        "libturbojpeg.so.0",
        "libtiff.so.6",
        "libtiffxx.so.6",
        "libwebp.so.7",
        "libwebpdemux.so.2",
        "libwebpmux.so.3",
        "libpng16.so.16",
        "libpng.so.3",
        "libBrokenLocale.so.1",
        "libSegFault.so",
        "libc_malloc_debug.so.0",
        "libcidn.so.1",
        "libmemusage.so",
        "libmvec.so.1",
        "libnss_files.so.2",
        "libpcprofile.so",
        "libthread_db.so.1",
    ] {
        fs::write(&manifest, format!("{digest}  {platform_library}\n"))
            .expect("platform runtime manifest should be written");
        let rejected = std::process::Command::new("bash")
            .arg(&policy)
            .arg(&manifest)
            .output()
            .expect("runtime policy should run");
        assert!(
            !rejected.status.success(),
            "{platform_library} must be rejected"
        );
        assert!(
            String::from_utf8_lossy(&rejected.stderr).contains(platform_library),
            "rejection should name {platform_library}"
        );
    }

    let stray_glibc = temp.path().join("libmvec.so.1");
    fs::write(&stray_glibc, b"not an ELF object").expect("stray glibc fixture");
    let rejected = std::process::Command::new("bash")
        .args([
            "-c",
            "source \"$1\"; okp_verify_no_linux_glibc_runtime_files \"$2\"",
            "bash",
        ])
        .arg(&policy)
        .arg(temp.path())
        .output()
        .expect("runtime file policy should run");
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("libmvec.so.1"));
}

#[test]
fn real_velopack_pack_resolves_candidate_channel_artifact_identities() {
    if std::env::var_os("OKP_RUN_VELOPACK_PACK_TEST").is_none() {
        return;
    }

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let version = "0.11.0-beta.0.367";
    let channel = "linux-candidate";
    let package_id = okp_core::velopack_artifacts::LINUX_VELOPACK_PACKAGE_ID;
    let fixture = okp_test_fixtures::run_velopack_pack(
        package_id,
        version,
        channel,
        &root.join("rust/packaging/linux/com.befeast.okplayer.svg"),
    )
    .expect("real Velopack candidate pack should succeed");
    let versioned_name = format!("OK-Player-{version}-x86_64.AppImage");
    let identity = okp_core::velopack_artifacts::stage_versioned_appimage(
        &fixture.output_dir,
        channel,
        package_id,
        version,
        &versioned_name,
    )
    .expect("channel-qualified Velopack outputs should resolve and stage");

    assert_eq!(identity.feed_file_name, "releases.linux-candidate.json");
    assert_eq!(identity.package_id, package_id);
    assert_eq!(identity.version, version);
    assert_eq!(
        identity.full_package_file_name,
        format!("{package_id}-{version}-linux-candidate-full.nupkg")
    );
    assert_eq!(
        identity.appimage_file_name,
        format!("{package_id}-linux-candidate.AppImage")
    );
    assert!(identity.full_package_size > 0);
    assert!(identity.appimage_size > 0);
    let source = fs::read(fixture.output_dir.join(&identity.appimage_file_name))
        .expect("standalone AppImage should be readable");
    let staged = fs::read(fixture.output_dir.join(&versioned_name))
        .expect("versioned AppImage should be readable");
    assert_eq!(staged, source);

    fs::write(fixture.output_dir.join(&identity.appimage_file_name), [])
        .expect("standalone AppImage should be corruptible for the failure regression");
    let error = okp_core::velopack_artifacts::stage_versioned_appimage(
        &fixture.output_dir,
        channel,
        package_id,
        version,
        &versioned_name,
    )
    .expect_err("a corrupt standalone AppImage must fail staging");
    assert!(error.contains("standalone AppImage matching the Full package"));
    assert!(
        !fixture.output_dir.join(&versioned_name).exists(),
        "failure must remove the apparently versioned output"
    );

    let public_fixture = okp_test_fixtures::run_velopack_pack(
        package_id,
        version,
        "linux",
        &root.join("rust/packaging/linux/com.befeast.okplayer.svg"),
    )
    .expect("real Velopack public pack should succeed");
    let public_identity = okp_core::velopack_artifacts::stage_versioned_appimage(
        &public_fixture.output_dir,
        "linux",
        package_id,
        version,
        &versioned_name,
    )
    .expect("public-channel Velopack outputs should remain supported");
    assert_eq!(public_identity.feed_file_name, "releases.linux.json");
    assert_eq!(
        public_identity.full_package_file_name,
        format!("{package_id}-{version}-linux-full.nupkg")
    );
    assert_eq!(
        public_identity.appimage_file_name,
        format!("{package_id}.AppImage")
    );
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

    // The audio-track/output action reads as a distinct music-track semantic,
    // rendered in-process so a sparse packaged icon theme cannot erase it.
    assert!(
        controls.contains("audio_track_icon(AUDIO_TRACK_ICON_SIZE)"),
        "audio-track/output action must use the bundled music-track glyph"
    );
    assert!(
        !controls.contains("audio-speakers-symbolic"),
        "regression guard: no second speaker glyph on the audio action"
    );
    assert!(!controls.contains("audio-headphones-symbolic"));

    // Distinct accessible names and tooltips: Volume versus Audio tracks / output.
    assert!(controls.contains("gtk::accessible::Property::Label(\"Volume\")"));
    assert!(controls.contains("set_tooltip_text(Some(\"Audio tracks / output\"))"));
    assert!(controls.contains("gtk::accessible::Property::Label(\"Audio tracks / output\")"));
}

#[test]
fn audio_track_glyph_paints_real_pixels_without_an_icon_theme() {
    let mut surface =
        cairo::ImageSurface::create(cairo::Format::ARgb32, 19, 19).expect("audio glyph surface");
    let cr = cairo::Context::new(&surface).expect("audio glyph context");
    draw_audio_track_glyph(&cr, 19, 19, gdk::RGBA::WHITE);
    drop(cr);
    surface.flush();

    let painted_bytes = surface
        .data()
        .expect("audio glyph pixels")
        .iter()
        .filter(|&&channel| channel != 0)
        .count();
    assert!(
        painted_bytes > 80,
        "music-track glyph rendered no usable pixels"
    );
    assert!(
        painted_bytes < 900,
        "music-track glyph unexpectedly filled its allocation"
    );
}

#[test]
fn settings_stays_in_the_width_safe_bottom_more_menu() {
    let window = include_str!("window.rs");
    let controls = include_str!("controls.rs");
    let popovers = include_str!("track_popovers.rs");

    assert!(!window.contains("gtk::Button::from_icon_name(\"emblem-system-symbolic\")"));
    assert!(window.contains("persistent_widgets: Vec::new()"));
    // The overflow entry is the final in-flow OSC action inside the adaptive
    // OscBar, never a floating overlay that could paint over its neighbour.
    assert!(controls.contains("let bar = OscBar::new();"));
    assert!(controls.contains("bar.push(&controls.more_button, OscControlId::Overflow);"));
    assert!(!controls.contains("more_slot"));
    assert!(!controls.contains("chrome.add_overlay(&controls.more_button)"));
    assert!(popovers.contains("Id::OpenSettings =>"));
    assert!(controls.contains("PlayerCommandSurface::More"));
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
fn adaptive_osc_bar_collapses_into_overflow_without_overlap() {
    let osc = include_str!("osc_bar.rs");
    let controls = include_str!("controls.rs");
    let css = include_str!("css.rs");

    // The bar reports only the floor as its horizontal minimum, so a narrow
    // window hands it the real allocation instead of forcing the full width and
    // clipping the trailing overflow entry.
    assert!(osc.contains("osc_overflow::floor_min_width(&slots, SPACING)"));
    assert!(osc.contains("osc_overflow::plan(&slots, width, SPACING"));
    // Collapsed controls are unmapped: not painted, focusable, or hit-testable.
    assert!(osc.contains("child.set_child_visible(false)"));
    // The row mirrors for right-to-left locales, keeping the layout RTL-safe.
    assert!(osc.contains("gtk::TextDirection::Rtl"));
    // The overflow entry is the final in-flow action, not a floating overlay.
    assert!(controls.contains("bar.push(&controls.more_button, OscControlId::Overflow);"));
    assert!(controls.contains("bar.set_collapsed_sink("));
    // The custom container owns the pill inset, so the `.okp-controls` CSS
    // padding is zeroed to avoid double-insetting the controls.
    let controls_block = css
        .split_once(".okp-controls {")
        .expect("controls pill block")
        .1
        .split_once('}')
        .expect("controls pill block close")
        .0;
    assert!(controls_block.contains("padding: 0;"));
    assert!(!controls_block.contains("padding: 7px 14px;"));
    assert!(osc.contains("pub(crate) const PAD_HORIZONTAL: i32 = 14;"));
    assert!(osc.contains("pub(crate) const PAD_VERTICAL: i32 = 7;"));
}

#[test]
fn media_presence_is_the_single_owner_of_standard_osc_mapping() {
    let chrome = include_str!("main.rs");
    let compact = include_str!("compact_mode.rs");

    assert!(chrome.contains("okp_core::osc_visibility::project("));
    assert!(chrome.contains("revealer.set_visible(next.visible)"));
    assert!(chrome.contains("revealer.set_sensitive(next.focusable)"));
    assert!(chrome.contains("revealer.set_can_target(next.hit_testable)"));
    assert!(chrome.contains("if old.visible != next.visible"));
    assert!(chrome.contains("if old.focusable != next.focusable"));
    assert!(chrome.contains("if old.hit_testable != next.hit_testable"));
    assert!(chrome.contains("self.has_media.replace(has_media) != has_media"));
    assert!(chrome.contains("self.surface_suppressed.replace(suppressed) != suppressed"));

    assert!(!compact.contains("standard_osc"));
    assert!(!compact.contains("self.chrome.widget().set_visible"));
    assert!(compact.contains("self.chrome.set_surface_suppressed(compact)"));
}

#[test]
fn overflow_menu_surfaces_every_collapsed_control_action() {
    let popovers = include_str!("track_popovers.rs");
    let registry = player_commands::player_command_registry();

    for id in [
        PlayerCommandId::PlayPause,
        PlayerCommandId::PlaybackSpeed,
        PlayerCommandId::Subtitles,
        PlayerCommandId::AudioTrack,
        PlayerCommandId::ChaptersUpNext,
        PlayerCommandId::SaveFrame,
        PlayerCommandId::Fullscreen,
        PlayerCommandId::OpenSettings,
    ] {
        assert!(registry.iter().any(|command| command.id == id));
    }
    assert!(popovers.contains("populate_speed_popover(popover"));
    assert!(popovers.contains("populate_subtitle_popover("));
    assert!(popovers.contains("populate_audio_popover(popover"));
    assert!(popovers.contains("reach.chapters.emit_clicked();"));
    assert!(popovers.contains("reach.screenshot.emit_clicked();"));
    assert!(popovers.contains("reach.fullscreen.emit_clicked();"));
    assert!(popovers.contains("reach.play.emit_clicked();"));
}

#[test]
fn screenshot_surfaces_share_the_same_capture_implementation() {
    let controls = include_str!("controls.rs");
    let keyboard = include_str!("keyboard.rs");
    let playback = include_str!("playback.rs");
    let popovers = include_str!("track_popovers.rs");

    assert!(controls.contains(
        "connect_clicked(move |_| save_screenshot(&screenshot_state, &screenshot_toast, false))"
    ));
    assert!(keyboard.contains("Some(ShortcutAction::SaveScreenshot)"));
    assert!(keyboard.contains("save_screenshot(&state, &status_toast, false);"));
    assert!(keyboard.contains("copy_frame_to_clipboard(&state, &status_toast);"));
    assert!(popovers.contains("Id::SaveFrame =>"));
    assert!(popovers.contains("reach.screenshot.emit_clicked();"));
    assert!(popovers.contains("save_screenshot(state, status_toast, true);"));
    assert!(popovers.contains("copy_frame_to_clipboard(state, status_toast);"));
    assert!(playback.contains("mpv.screenshot_to_file_async(path, include_subtitles)"));
    assert!(playback.contains("render_loop.render_for_screenshot();"));
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
    assert_eq!(
        normalized_settings_page(" Shortcuts "),
        Some(SettingsPage::Shortcuts)
    );
    assert_eq!(
        normalized_settings_page("UPDATES"),
        Some(SettingsPage::Updates)
    );
    assert_eq!(normalized_settings_page("about"), Some(SettingsPage::About));
    assert_eq!(normalized_settings_page("native-caption"), None);
}

#[test]
fn settings_updates_has_one_dedicated_content_owner() {
    let updates = include_str!("updates.rs");
    let advanced_page = updates
        .split_once("pub(crate) fn settings_advanced_page(")
        .expect("Advanced page")
        .1
        .split_once("pub(crate) fn settings_updates_page(")
        .expect("Updates page follows Advanced")
        .0;
    assert!(!advanced_page.contains("settings_updates_section"));

    let updates_page = updates
        .split_once("pub(crate) fn settings_updates_page(")
        .expect("dedicated Updates page")
        .1
        .split_once("pub(crate) fn settings_raw_mpv_section(")
        .expect("raw mpv section follows page constructors")
        .0;
    assert_eq!(updates_page.matches("settings_updates_section").count(), 1);

    let window = include_str!("settings_window.rs");
    let updates_builder = window
        .split_once("SettingsPage::Updates =>")
        .expect("lazy Updates page builder")
        .1
        .split_once("SettingsPage::Playback =>")
        .expect("Playback builder follows Updates")
        .0;
    assert_eq!(updates_builder.matches("settings_updates_page(").count(), 1);
    assert!(window.contains("Some(page.id())"));
    assert!(window.contains("SettingsNavIcon::Updates"));
    assert!(window.contains("search_settings(entry.text().as_str())"));
}

#[test]
fn update_decision_surfaces_are_shared_persistent_and_accessible() {
    let updates = include_str!("updates.rs");
    let window = include_str!("window.rs");
    let dialogs = include_str!("dialogs.rs");

    assert!(window.contains("persistent_update_surface("));
    assert!(window.contains("overlay.add_overlay(&update_surface)"));
    assert!(updates.contains("LinuxUpdateViewKind::Persistent"));
    assert!(updates.contains("LinuxUpdateViewKind::Settings"));
    assert!(updates.contains("refresh_linux_update_views"));
    assert!(updates.contains("gtk::Button::with_label(\"Update\")"));
    assert!(updates.contains("gtk::Button::with_label(\"Skip this version\")"));
    assert!(updates.contains("\"Install anyway\""));
    assert!(updates.contains("gtk::AccessibleRole::Group"));
    assert!(updates.contains("gtk::accessible::Property::Label(\"Available update actions\")"));
    assert!(updates.contains("gtk::accessible::Property::Label(\"Update OK Player\")"));
    assert!(updates.contains("gtk::accessible::Property::Label(\"Skip this update version\")"));
    assert!(!dialogs.contains("Update available"));
    assert!(!updates.contains("status_toast.show(&format!(\"Update available:"));
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
    let video = stream_sections
        .iter()
        .find(|section| section.title == "Video")
        .expect("video stream section");
    let dynamic_range_index = video
        .rows
        .iter()
        .position(|row| row.label == "Dynamic Range")
        .expect("dynamic range row");
    assert_eq!(video.rows[dynamic_range_index + 1].label, "HDR Handling");
    assert_eq!(
        video.rows[dynamic_range_index + 1].value,
        "Automatic · engine-managed"
    );

    let stats_sections = media_info_stats_sections(&sample);
    let stats_titles: Vec<&str> = stats_sections
        .iter()
        .map(|section| section.title.as_str())
        .collect();
    assert_eq!(
        stats_titles,
        vec!["Decode · Render", "Live · Performance", "Display · Output"]
    );
    let decode = stats_sections
        .iter()
        .find(|section| section.title == "Decode · Render")
        .expect("decode diagnostics");
    assert!(
        decode
            .rows
            .iter()
            .any(|row| row.label == "Engine Tone Mapping")
    );
    assert!(!decode.rows.iter().any(|row| row.label == "Tone Mapping"));
}

#[test]
fn media_info_hdr_handling_is_reserved_only_for_hdr_sources() {
    let mut sample = media_info_preview_sample();
    let video = sample
        .sections
        .iter_mut()
        .find(|section| section.title == "Video")
        .expect("video section");
    let dynamic_range = video
        .rows
        .iter_mut()
        .find(|row| row.label == "Dynamic Range")
        .expect("dynamic range row");
    dynamic_range.value = "SDR".to_owned();

    let video = media_info_stream_sections(&sample)
        .into_iter()
        .find(|section| section.title == "Video")
        .expect("video stream section");
    assert!(!video.rows.iter().any(|row| row.label == "HDR Handling"));
}

#[test]
fn hdr_settings_reservation_has_no_toggle_or_action() {
    let source = include_str!("settings_pages.rs");
    let function = source
        .split_once("pub(crate) fn settings_hdr_handling_row()")
        .expect("HDR settings row")
        .1
        .split_once("pub(crate) fn settings_shortcuts_section")
        .expect("next settings function")
        .0;

    assert!(function.contains("HDR handling"));
    assert!(function.contains("settings_label"));
    assert!(!function.contains("gtk::Switch"));
    assert!(!function.contains("gtk::Button"));
    assert!(!function.contains("connect_"));
}

#[test]
fn companion_window_geometry_clamps_natural_and_restored_sizes() {
    let work_area = window_fit::WindowRect {
        x: 0,
        y: 0,
        width: 1280,
        height: 852,
    };
    assert_eq!(
        companion_window_core::companion_window_size(
            CompanionWindowKind::MediaInfo,
            None,
            work_area,
        ),
        window_fit::WindowSize {
            width: 720,
            height: 571,
        }
    );
    assert_eq!(
        companion_window_core::companion_window_size(
            CompanionWindowKind::Settings,
            Some(window_fit::WindowSize {
                width: 1600,
                height: 1000,
            }),
            work_area,
        ),
        window_fit::WindowSize {
            width: 1280,
            height: 852,
        }
    );
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
fn media_info_window_classes_have_scoped_css() {
    let stylesheet = include_str!("css.rs");
    for class in [
        "okp-companion-window",
        "okp-companion-resize-zone",
        "okp-media-info-window",
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
            "Media Information class {class} must have window-scoped CSS"
        );
    }
    assert!(!stylesheet.contains("okp-media-info-backdrop"));
}

#[test]
fn media_info_command_uses_the_companion_window_entry_point() {
    let source = include_str!("track_popovers.rs");
    assert_eq!(
        player_commands::player_command_registry()
            .iter()
            .filter(|command| command.id == PlayerCommandId::MediaInfo)
            .count(),
        1
    );
    assert_eq!(source.matches("open_media_info_window(").count(), 1);
    assert!(source.contains("dispatch_player_command_action("));
    assert!(!source.contains("show_media_info_modal"));
    assert!(!source.contains("show_media_info_window"));
}

#[test]
fn long_lived_surfaces_share_non_modal_single_instance_window_semantics() {
    let helper = include_str!("companion_window.rs");
    let settings = include_str!("settings_window.rs");
    let media_info = include_str!("media_info.rs");

    assert!(helper.contains(".modal(policy.modal)"));
    assert!(helper.contains(".resizable(policy.resizable)"));
    assert!(!helper.contains(".transient_for("));
    assert!(helper.contains("present_existing_companion_window"));
    assert!(helper.contains("add_companion_window_resize_zones"));
    assert!(helper.contains("policy.retain_on_close"));
    assert!(helper.contains("glib::Propagation::Stop"));
    assert!(helper.contains("shutting_down"));
    assert!(helper.contains("settings.window.take()"));
    let slot = helper
        .split_once("struct CompanionWindowSlot {")
        .expect("companion window slot")
        .1
        .split_once("struct CompanionMapTiming")
        .expect("map timing follows companion window slot")
        .0;
    assert!(slot.contains("window: Option<gtk::Window>"));
    assert!(settings.contains("CompanionWindowKind::Settings"));
    assert!(media_info.contains("CompanionWindowKind::MediaInfo"));
    assert!(settings.contains("existing_companion_window"));
    assert!(media_info.contains("present_existing_companion_window"));
}

#[test]
fn command_confirmations_and_file_pickers_remain_modal() {
    let dialogs = include_str!("dialogs.rs");
    let subtitle_search = include_str!("track_popovers.rs");

    assert!(dialogs.matches(".modal(true)").count() >= 5);
    assert!(dialogs.matches("dialog.set_modal(true)").count() >= 4);
    assert!(subtitle_search.contains(".modal(true)"));
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
    assert!(!media_info_row_is_highlight("Dynamic Range", "Unknown"));
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
    let media = root.path().join("Track 01.flac");
    let folder_cover = root.path().join("cover.jpg");
    let same_named = root.path().join("Track 01.png");
    fs::write(&media, []).expect("test media should be written");
    write_jpeg_header(&folder_cover);
    write_png_header(&same_named);

    assert_eq!(mpris_sidecar_art_path(&media), Some(same_named.clone()));
    assert_eq!(
        mpris_sidecar_art_url(&media),
        Some(local_file_uri(&same_named))
    );

    root.close().expect("test folder should be removed");
}

#[test]
fn mpris_sidecar_art_uses_folder_priority_and_skips_junk() {
    let root = unique_temp_dir("okp-mpris-art-folder");
    let media = root.path().join("Episode 1.mkv");
    let bad_cover = root.path().join("cover.jpg");
    let folder_cover = root.path().join("folder.jpg");
    let poster = root.path().join("poster.png");
    fs::write(&media, []).expect("test media should be written");
    fs::write(&bad_cover, []).expect("junk cover should be written");
    write_jpeg_header(&folder_cover);
    write_png_header(&poster);

    assert_eq!(mpris_sidecar_art_path(&media), Some(folder_cover));

    root.close().expect("test folder should be removed");
}

#[test]
fn mpris_local_art_prefers_sidecar_before_embedded_art() {
    let root = unique_temp_dir("okp-mpris-art-sidecar-first");
    let media = root.path().join("Song.flac");
    let sidecar = root.path().join("Song.jpg");
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

    root.close().expect("test folder should be removed");
}

#[test]
fn mpris_embedded_art_is_audio_only() {
    let root = unique_temp_dir("okp-mpris-art-video-skip");
    let media = root.path().join("Movie.mkv");
    fs::write(&media, b"not a real video").expect("test media should be written");

    assert_eq!(mpris_embedded_art_url(&media), None);

    root.close().expect("test folder should be removed");
}

#[test]
fn mpris_embedded_art_cache_path_changes_when_media_changes() {
    let root = unique_temp_dir("okp-mpris-art-cache-key");
    let media = root.path().join("Song.flac");
    let cache_dir = root.path().join("cache");
    fs::write(&media, [1_u8]).expect("test media should be written");
    let before = mpris_embedded_art_cache_key(&media).expect("cache key should resolve");

    fs::write(&media, [1_u8, 2_u8]).expect("test media should be updated");
    let after = mpris_embedded_art_cache_key(&media).expect("updated cache key should resolve");

    assert_ne!(before.len, after.len);
    assert_ne!(
        mpris_embedded_art_cache_path_in_dir(&before, &cache_dir),
        mpris_embedded_art_cache_path_in_dir(&after, &cache_dir)
    );

    root.close().expect("test folder should be removed");
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
    let media = root.path().join("Movie.mkv");
    fs::write(&media, b"media").expect("test media should be written");
    fs::write(
        root.path().join("Movie.nfo"),
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

    root.close().expect("test folder should be removed");
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
    let first = root.path().join("Episode 1.mkv");
    let second = root.path().join("Episode 2.mkv");
    let third = root.path().join("Episode 3.mkv");
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

    root.close().expect("test folder should be removed");
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
    let tool = dir.path().join("yt-dlp");
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
    assert_eq!(PlayerPopoverKind::More.width(), COMMAND_POPOVER_WIDTH);

    let quick_widths = [
        PlayerPopoverKind::Speed.width(),
        PlayerPopoverKind::Subtitles.width(),
        PlayerPopoverKind::Audio.width(),
    ];
    assert!(!quick_widths.contains(&PlayerPopoverKind::More.width()));
    assert_eq!(
        PlayerPopoverKind::More.width(),
        PlayerPopoverKind::AdvancedCommands.width()
    );
}

#[test]
fn more_and_context_menu_share_one_registry_renderer_and_dispatcher() {
    let source = include_str!("track_popovers.rs");
    let controls = include_str!("controls.rs");
    assert_eq!(
        source
            .matches("pub(crate) fn populate_command_popover(")
            .count(),
        1
    );
    assert_eq!(
        source
            .matches("pub(crate) fn dispatch_player_command_action(")
            .count(),
        1
    );
    assert!(source.contains("player_commands::resolve_player_commands(surface"));
    assert!(source.contains("player_commands::filter_player_commands(commands, query)"));
    assert!(controls.contains("PlayerCommandSurface::More"));

    let player_clicks = include_str!("mpv_bridge.rs");
    assert!(player_clicks.contains("context_click.set_button(gdk::BUTTON_SECONDARY)"));
    assert!(player_clicks.contains("context_root.pick(x, y, gtk::PickFlags::INSENSITIVE)"));
    assert!(player_clicks.contains("player_context_menu_target_is_interactive("));
    assert!(player_clicks.contains("show_player_context_menu("));
    assert!(player_clicks.contains("popover.set_parent(player_root)"));
    assert!(player_clicks.contains("connect_popover_chrome_pin(&popover, chrome)"));
    assert!(player_clicks.contains("PlayerCommandSurface::ContextMenu"));
    assert!(player_clicks.contains("populate_command_popover("));

    let window = include_str!("window.rs");
    assert!(window.contains("connect_player_context_menu("));
}

#[test]
fn shared_command_renderer_uses_the_curated_two_level_core_model() {
    let source = include_str!("track_popovers.rs");
    assert!(source.contains("player_commands::PLAYER_COMMAND_MENU_TOP_LEVEL"));
    assert!(source.contains("PlayerCommandMenuEntry::Command(id)"));
    assert!(source.contains("PlayerCommandMenuEntry::Submenu(page)"));
    assert!(source.contains("PlayerCommandMenuEntry::Separator"));
    assert!(source.contains("page.commands()"));
    assert!(source.contains("close-media-top-level=true"));
    assert!(source.contains("OKP_ASSERT_COMMAND_MENU_FIT"));
}

#[test]
fn command_menu_search_is_a_single_shared_compact_entry_with_icon() {
    let source = include_str!("track_popovers.rs");
    // The shared component is a plain GtkEntry with a deliberate primary icon so
    // CSS fully owns the icon/placeholder spacing and alignment.
    assert!(source.contains("fn player_command_search_entry()"));
    assert!(source.contains("gtk::Entry::new()"));
    assert!(source.contains("gtk::EntryIconPosition::Primary"));
    assert!(source.contains("\"system-search-symbolic\""));
    assert!(source.contains("set_icon_tooltip_text(gtk::EntryIconPosition::Primary"));
    assert!(source.contains("gtk::AccessibleRole::SearchBox"));
    assert!(!source.contains("gtk::SearchEntry::new()"));

    // The search is wired into the shared command surface via connect_changed
    // so keyboard/focus/activation behavior stays intact.
    assert!(
        source
            .contains("search.connect_changed(move |entry| render_changed(entry.text().as_str()))")
    );

    // Both entry points (three-dots menu and right-click) use the same renderer.
    assert!(source.contains("populate_command_popover("));
    let controls = include_str!("controls.rs");
    assert!(controls.contains("PlayerCommandSurface::More"));
    let bridge = include_str!("mpv_bridge.rs");
    assert!(bridge.contains("PlayerCommandSurface::ContextMenu"));

    // High-contrast state is propagated to the popover alongside dark mode.
    let window = include_str!("window.rs");
    assert!(window.contains("pub(crate) fn idle_theme_is_high_contrast()"));
    assert!(source.contains("idle_theme_is_high_contrast()"));
    assert!(source.contains("popover.add_css_class(\"is-high-contrast\")"));

    let css = include_str!("css.rs");
    // Compact height/radius proportional to command rows, deliberate left padding
    // for the icon, and shared design tokens for every state.
    assert!(css.contains("entry.okp-command-search {"));
    assert!(css.contains("min-height: 24px;"));
    assert!(css.contains("padding: 4px 10px 4px 0;"));
    assert!(css.contains("border-radius: 6px;"));
    assert!(css.contains("entry.okp-command-search > image {"));
    assert!(css.contains("entry.okp-command-search > text > placeholder {"));
    assert!(css.contains("entry.okp-command-search:hover {"));
    assert!(css.contains("entry.okp-command-search:disabled {"));
    assert!(css.contains("popover.okp-command-popover.is-dark entry.okp-command-search:hover {"));
    assert!(
        css.contains("popover.okp-command-popover.is-high-contrast entry.okp-command-search {")
    );
    assert!(css.contains(
        "popover.okp-command-popover.is-dark.is-high-contrast entry.okp-command-search,"
    ));

    // Deterministic visual-smoke seams cover the normal 340 px surface and the
    // work-area-bounded narrow surface without requiring loaded media.
    assert!(window.contains("OKP_OPEN_MORE_POPOVER_ON_STARTUP"));
    assert!(window.contains("OKP_NARROW_COMMAND_PREVIEW"));
    assert!(bridge.contains("has_media || seek_preview || command_preview"));
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
        "self.chrome.set_surface_suppressed(compact)",
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
fn player_window_move_drags_the_whole_non_interactive_surface() {
    // A left-drag that clears the shared threshold anywhere on a non-OSC surface
    // begins a compositor-native move; short clicks stay play/pause and a
    // stationary double-click stays fullscreen. The gesture policy is verified
    // in okp-core; here we lock the shell wiring that feeds it.
    let bridge = include_str!("mpv_bridge.rs");
    assert!(bridge.contains("pub(crate) fn connect_player_window_move("));
    // Left-button drag with a movement threshold, not an immediate press grab.
    assert!(bridge.contains("gtk::GestureDrag::new()"));
    assert!(bridge.contains("drag.set_button(gdk::BUTTON_PRIMARY)"));
    assert!(bridge.contains("video_click::window_drag_action("));
    assert!(bridge.contains("video_click::WindowDragAction::BeginMove"));
    // Snapshot transient motion metadata before changing GTK gesture ownership.
    assert!(bridge.contains("let Some(device) = gesture.current_event_device()"));
    assert!(bridge.contains("let Some((surface_x, surface_y)) = gesture.bounding_box_center()"));
    assert!(bridge.contains("let button = gesture.current_button() as i32"));
    assert!(bridge.contains("let timestamp = gesture.current_event_time()"));
    // Reuse the right-click interactive classifier at press time so OSC/sliders/
    // buttons/panels keep their input, and fail safe when the pick is missing.
    assert!(bridge.contains("player_context_menu_target_is_interactive("));
    assert!(bridge.contains(".unwrap_or(true)"));
    // Fullscreen/maximized guards and compact-mode handoff.
    assert!(bridge.contains("move_window.is_fullscreen()"));
    assert!(bridge.contains("move_window.is_maximized()"));
    assert!(bridge.contains("window_compact_mode_active(&move_window)"));
    // Wayland-native move: GDK must consume the live implicit grab before GTK
    // claims/cancels sibling gestures. X11 keeps the established inverse order
    // required by its WM handoff. A shared one-shot suppressor prevents the drag
    // release from becoming play/pause if the compositor cancels first.
    assert!(bridge.contains("toplevel.begin_move("));
    assert!(
        bridge.contains("click.connect_pressed(move |_, _, _, _| reset_suppression.set(false))")
    );
    assert!(bridge.contains("video-click-suppressed-by-window-drag"));
    // Both normal completion and cancellation release the per-drag state without
    // unwrap/expect paths that could abort inside a GTK callback.
    assert!(bridge.contains("drag.connect_drag_end"));
    assert!(bridge.contains("drag.connect_cancel"));

    let move_wiring = bridge
        .split("pub(crate) fn connect_player_window_move(")
        .nth(1)
        .and_then(|tail| {
            tail.split("pub(crate) fn player_context_menu_target_is_interactive(")
                .next()
        })
        .expect("player window move function");
    assert!(!move_wiring.contains(".unwrap()"));
    assert!(!move_wiring.contains(".expect("));
    assert!(move_wiring.contains("let wayland = is_wayland_display("));
    assert!(move_wiring.contains("if !wayland"));
    assert!(move_wiring.contains("if wayland"));
    let begin_move = move_wiring
        .find("toplevel.begin_move(")
        .expect("native move handoff");
    let wayland_branch = move_wiring
        .rfind("if wayland")
        .expect("Wayland claim branch");
    assert!(
        begin_move < wayland_branch,
        "Wayland must consume the grab before GTK claims it"
    );

    let window = include_str!("window.rs");
    assert!(
        window.contains("let suppress_video_click = connect_player_window_move(&overlay, &window)")
    );
    assert!(window.contains("suppress_video_click,"));
}

#[test]
fn player_shift_resize_owns_pointer_geometry_without_configure_feedback() {
    let window = include_str!("window.rs");
    for required in [
        "gtk::GestureDrag::new()",
        "connect_drag_begin",
        "connect_drag_update",
        "connect_drag_end",
        "connect_modifiers",
        "gdk::ModifierType::SHIFT_MASK",
        "aspect_resize::client_max_for_anchor(",
        "session.resolve(pointer)",
        "update_window.set_default_size(",
        "move_resize_player_window_on_x11(",
        "current_pointer_position_on_x11(",
        "current_drag_pointer(",
        "event.position()?",
        "move |gesture, _offset_x, _offset_y|",
    ] {
        assert!(window.contains(required), "missing resize seam: {required}");
    }
    assert!(!window.contains("size.set_size(resolved.width, resolved.height)"));
    assert!(!window.contains("x: offset_x"));
    assert!(!window.contains("y: offset_y"));
    for edge in [
        "gdk::SurfaceEdge::North",
        "gdk::SurfaceEdge::South",
        "gdk::SurfaceEdge::West",
        "gdk::SurfaceEdge::East",
        "gdk::SurfaceEdge::NorthWest",
        "gdk::SurfaceEdge::NorthEast",
        "gdk::SurfaceEdge::SouthWest",
        "gdk::SurfaceEdge::SouthEast",
    ] {
        assert!(window.contains(edge), "missing resize edge: {edge}");
    }
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
fn online_subtitle_reservation_is_visible_and_has_no_dispatch_path() {
    let popover = include_str!("track_popovers.rs");
    assert!(popover.contains("OnlineSubtitleSearchContext::reserved("));
    assert!(popover.contains("Find subtitles online…"));

    let button_start = popover
        .find("pub(crate) fn online_subtitle_button(")
        .expect("online subtitle button should remain in the shared subtitle popover");
    let button_end = popover[button_start..]
        .find("#[derive(Clone, Copy)]")
        .map(|offset| button_start + offset)
        .expect("online subtitle button should end before the action-icon declaration");
    let button = &popover[button_start..button_end];

    assert!(button.contains("button.set_sensitive(false)"));
    assert!(button.contains("state.message()"));
    assert!(!button.contains("connect_clicked"));
}

#[test]
fn scribe_subtitle_reservation_is_shared_by_quick_context_and_settings_paths() {
    let state = PlayerState::default();
    let presentation = scribe_subtitle_presentation(&state);

    assert_eq!(presentation.label, "Generate subtitles…");
    assert_eq!(presentation.badge, "SOON");
    assert!(!presentation.can_generate);
    assert!(!presentation.can_cancel);
    assert!(presentation.message.contains("No network request"));

    let popover = include_str!("track_popovers.rs");
    assert!(popover.contains("append_scribe_subtitle_rows("));
    assert!(popover.contains("Id::Subtitles =>"));
    assert!(popover.contains("populate_subtitle_popover(popover, parent"));

    let settings = include_str!("settings_pages.rs");
    assert!(settings.contains("scribe_subtitle_presentation(&state.borrow())"));
    assert!(settings.contains("Cancel generation"));
}

#[test]
fn scribe_subtitle_placeholder_can_queue_progress_and_cancel_without_transport() {
    let state = Rc::new(RefCell::new(PlayerState {
        current_file: Some(PathBuf::from("/media/Movie.mkv")),
        scribe_subtitles: scribe_subtitles::ScribeSubtitleState::new(
            scribe_subtitles::ScribeSubtitleConfig::supported("https://scribe.example.invalid"),
        ),
        ..PlayerState::default()
    }));

    assert!(scribe_subtitle_presentation(&state.borrow()).can_generate);
    assert_eq!(begin_scribe_subtitle_generation(&state), Ok(()));
    assert_eq!(
        begin_scribe_subtitle_generation(&state),
        Err(scribe_subtitles::ScribeSubtitleBeginError::AlreadyActive)
    );

    let queued = scribe_subtitle_presentation(&state.borrow());
    assert_eq!(queued.badge, "QUEUED");
    assert!(queued.show_progress);
    assert!(queued.can_cancel);
    assert!(!queued.can_generate);

    assert!(
        state
            .borrow_mut()
            .scribe_subtitles
            .mark_in_progress(Some(37))
    );
    let progress = scribe_subtitle_presentation(&state.borrow());
    assert_eq!(progress.badge, "37%");
    assert!(progress.message.contains("37% complete"));

    assert!(cancel_scribe_subtitle_generation(&state));
    let canceled = scribe_subtitle_presentation(&state.borrow());
    assert_eq!(canceled.badge, "CANCELED");
    assert!(!canceled.show_progress);
    assert!(!canceled.can_cancel);
    assert!(canceled.can_generate);
    assert_eq!(
        state.borrow().scribe_subtitles.network_endpoint(false),
        None
    );
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
    assert_eq!(PlayerPopoverKind::More.css_class(), "okp-command-popover");
    assert_eq!(
        PlayerPopoverKind::AdvancedCommands.css_class(),
        "okp-command-popover"
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
    let media = dir.path().join("Song.mp3");
    fs::write(&media, b"x").expect("media file");
    fs::write(
        dir.path().join("Song.lrc"),
        "[00:01.00]one\n[00:02.50]two\n",
    )
    .expect("sidecar");

    let document = okp_core::lyrics::read_sidecar(&media)
        .map(|text| lrc::parse(Some(&text)))
        .unwrap_or_default();

    assert!(document.has_timings);
    assert_eq!(document.lines.len(), 2);
    assert_eq!(document.lines[1].text, "two");
}

#[test]
fn missing_sidecar_resolves_to_an_empty_document_for_the_empty_state() {
    // A local audio file with no sidecar yields the calm empty state, never debug text: the
    // discover → parse expression produces the empty document `rebuild` renders as "No lyrics".
    let dir = unique_temp_dir("okp-gtk-lyrics-empty");
    let media = dir.path().join("Instrumental.flac");
    fs::write(&media, b"x").expect("media file");

    let document = okp_core::lyrics::read_sidecar(&media)
        .map(|text| lrc::parse(Some(&text)))
        .unwrap_or_default();

    assert!(document.is_empty());
    assert!(!document.has_timings);
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
fn clip_export_placeholder_explains_every_eligibility_state() {
    assert_eq!(
        clip_export_placeholder_reason(ClipExportEligibility::NoSelection),
        "Set both A and B to prepare export"
    );
    assert_eq!(
        clip_export_placeholder_reason(ClipExportEligibility::InvalidRange),
        "Set B after A"
    );
    assert_eq!(
        clip_export_placeholder_reason(ClipExportEligibility::SelectionTooShort {
            duration_seconds: 0.5,
            min_seconds: 1.0,
        }),
        "Select at least 1 second"
    );
    assert_eq!(
        clip_export_placeholder_reason(ClipExportEligibility::SelectionTooLong {
            duration_seconds: 301.0,
            max_seconds: 300.0,
        }),
        "Select 5 minutes or less"
    );
    assert_eq!(
        clip_export_placeholder_reason(ClipExportEligibility::MissingTooling),
        "Install FFmpeg to export"
    );
    assert_eq!(
        clip_export_placeholder_reason(ClipExportEligibility::Ready(
            okp_core::clip_export::ClipExportSelection {
                start_seconds: 12.0,
                end_seconds: 42.0,
            }
        )),
        "Selection ready (00:30); encoder not enabled"
    );
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
    let first = root.path().join("Episode 1.mp4");
    let second = root.path().join("Episode 2.mkv");
    let tenth = root.path().join("Episode 10.mkv");
    fs::write(&tenth, []).expect("test media should be created");
    fs::write(&first, []).expect("test media should be created");
    fs::write(root.path().join("Episode 2.srt"), []).expect("test subtitle should be created");
    fs::write(&second, []).expect("test media should be created");
    fs::write(root.path().join("cover.jpg"), []).expect("test ignored file should be created");

    assert_eq!(
        selected_media_paths(&[root.path().to_owned()]),
        vec![first, second, tenth]
    );

    root.close().expect("test folder should be removed");
}

#[test]
fn selected_media_paths_expands_multiple_folders_in_selection_order() {
    let root = unique_temp_dir("okp-folder-multi-selection");
    let season_one = root.path().join("Season 1");
    let season_two = root.path().join("Season 2");
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

    root.close().expect("test folders should be removed");
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
    let first = root.path().join("Episode 1.mkv");
    let second = root.path().join("Episode 2.mkv");
    let subtitle = root.path().join("Episode 2.srt");
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

    root.close().expect("test folder should be removed");
}

#[test]
fn load_selected_local_paths_opens_folder_as_playlist() {
    let root = unique_temp_dir("okp-folder-load");
    let first = root.path().join("Episode 1.mkv");
    let second = root.path().join("Episode 2.mkv");
    fs::write(&second, []).expect("test media should be created");
    fs::write(&first, []).expect("test media should be created");

    let state = Rc::new(RefCell::new(PlayerState::default()));

    assert!(load_selected_local_paths(
        &state,
        vec![root.path().to_owned()]
    ));

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

    root.close().expect("test folder should be removed");
}

#[test]
fn playlist_drop_target_index_maps_before_after_slots() {
    assert_eq!(Playlist::drop_target_index(0, 2, false), Some(1));
    assert_eq!(Playlist::drop_target_index(0, 2, true), Some(2));
    assert_eq!(Playlist::drop_target_index(3, 1, false), Some(1));
    assert_eq!(Playlist::drop_target_index(3, 1, true), Some(2));
}

#[test]
fn playlist_drop_target_index_rejects_self_or_existing_slot() {
    assert_eq!(Playlist::drop_target_index(2, 2, false), None);
    assert_eq!(Playlist::drop_target_index(2, 2, true), None);
    assert_eq!(Playlist::drop_target_index(1, 2, false), None);
    assert_eq!(Playlist::drop_target_index(2, 1, true), None);
}

#[test]
fn deb_checksum_download_refuses_release_without_manifest() {
    let update = DebUpdate {
        version: "0.1.0-linux-alpha.46".to_owned(),
        name: "ok-player_0.1.0-linux-alpha.46_amd64.deb".to_owned(),
        url: "https://example.invalid/update.deb".to_owned(),
        size: Some(42),
        sums_url: None,
        expected_sha256: None,
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
    let name = "ok-player_0.1.0-linux-alpha.46_amd64.deb";
    let payload = b"pretend this is a .deb archive".to_vec();
    let manifest = format!("{}  {name}\n", sha256sums::sha256_hex(&payload));
    let mut tampered = payload.clone();
    tampered[payload.len() / 2] ^= 0x01;

    let error = stage_verified_deb(&tampered, &manifest, name, cache_dir.path())
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
    assert!(!cache_dir.path().join(name).exists());
    assert!(!cache_dir.path().join(format!("{name}.part")).exists());
    cache_dir.close().expect("cache dir should be removed");
}

#[test]
fn staged_deb_tampered_on_disk_after_write_is_refused() {
    let cache_dir = unique_temp_dir("okp-deb-verify-disk");
    let name = "ok-player_0.1.0-linux-alpha.46_amd64.deb";
    let payload = b"pretend this is a .deb archive".to_vec();
    let manifest = format!("{}  {name}\n", sha256sums::sha256_hex(&payload));
    let staged = cache_dir.path().join(name);
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
    cache_dir.close().expect("cache dir should be removed");
}

#[test]
fn staged_deb_matching_manifest_is_finalized() {
    let cache_dir = unique_temp_dir("okp-deb-verify-ok");
    let name = "ok-player_0.1.0-linux-alpha.46_amd64.deb";
    let payload = b"pretend this is a .deb archive".to_vec();
    let manifest = format!(
        "{}  {name}\n{}  OK-Player-0.1.0-x86_64.AppImage\n",
        sha256sums::sha256_hex(&payload),
        sha256sums::sha256_hex(b"another asset")
    );

    let target = stage_verified_deb(&payload, &manifest, name, cache_dir.path())
        .expect("verified payload should be staged");

    assert_eq!(target, cache_dir.path().join(name));
    assert_eq!(
        fs::read(&target).expect("target should be readable"),
        payload
    );
    assert!(!cache_dir.path().join(format!("{name}.part")).exists());
    cache_dir.close().expect("cache dir should be removed");
}

#[test]
fn deb_update_exposes_the_target_version_to_the_shared_offer() {
    let update = PendingLinuxUpdate {
        manager: None,
        target: LinuxUpdateTarget::Deb(DebUpdate {
            version: "0.1.0-linux-alpha.46".to_owned(),
            name: "ok-player_0.1.0-linux-alpha.46_amd64.deb".to_owned(),
            url: "https://example.invalid/update.deb".to_owned(),
            size: Some(42),
            sums_url: None,
            expected_sha256: None,
        }),
    };

    assert_eq!(
        update.target_version().as_deref(),
        Some("0.1.0-linux-alpha.46")
    );
}

#[test]
fn linux_update_status_reflects_last_check_result() {
    let skipped = SkippedUpdateVersions::default();
    let up_to_date = LinuxUpdateStatus::from_check_result(
        &LinuxUpdateCheckResult::UpToDate,
        UpdateChannel::Public,
        &skipped,
    );
    assert_eq!(
        up_to_date.settings_status_text(true),
        "OK Player is up to date"
    );
    assert!(up_to_date.pending_offer().is_none());

    let managed = LinuxUpdateStatus::from_check_result(
        &LinuxUpdateCheckResult::ManagedExternally,
        UpdateChannel::Public,
        &skipped,
    );
    assert_eq!(
        managed.settings_status_text(true),
        "Updates are managed by DNF."
    );
    assert!(managed.pending_offer().is_none());

    let update = PendingLinuxUpdate {
        manager: None,
        target: LinuxUpdateTarget::Deb(DebUpdate {
            version: "0.1.0-linux-alpha.46".to_owned(),
            name: "ok-player_0.1.0-linux-alpha.46_amd64.deb".to_owned(),
            url: "https://example.invalid/update.deb".to_owned(),
            size: Some(42),
            sums_url: None,
            expected_sha256: None,
        }),
    };
    let available = LinuxUpdateStatus::from_check_result(
        &LinuxUpdateCheckResult::Available(update),
        UpdateChannel::Public,
        &skipped,
    );
    assert_eq!(
        available.settings_status_text(true),
        "Version 0.1.0-linux-alpha.46 is available."
    );
    let offer = available.pending_offer().expect("available update offer");
    assert_eq!(offer.state.primary_action_label(), Some("Update"));
    assert!(offer.state.can_skip());

    let failed = LinuxUpdateStatus::from_check_result(
        &LinuxUpdateCheckResult::Failed("no feed".into()),
        UpdateChannel::Public,
        &skipped,
    );
    assert_eq!(
        failed.settings_status_text(true),
        "Update check failed: no feed"
    );
}

#[test]
fn failed_manual_recheck_keeps_the_previous_update_offer() {
    let update = PendingLinuxUpdate {
        manager: None,
        target: LinuxUpdateTarget::Deb(DebUpdate {
            version: "0.11.0-beta.2".to_owned(),
            name: "ok-player_0.11.0-beta.2_amd64.deb".to_owned(),
            url: "https://example.invalid/update.deb".to_owned(),
            size: Some(42),
            sums_url: None,
            expected_sha256: None,
        }),
    };
    let available = LinuxUpdateStatus::from_check_result(
        &LinuxUpdateCheckResult::Available(update),
        UpdateChannel::Public,
        &SkippedUpdateVersions::default(),
    );
    let previous = available.pending_offer().expect("available offer");
    let state = Rc::new(RefCell::new(PlayerState {
        linux_update_status: LinuxUpdateStatus::Checking(Some(previous)),
        ..PlayerState::default()
    }));

    restore_after_failed_check(&state, "feed unavailable");

    let restored = state
        .borrow()
        .linux_update_status
        .pending_offer()
        .expect("offer should survive a failed refresh");
    assert_eq!(restored.state.version(), "0.11.0-beta.2");
    assert_eq!(restored.state.phase(), &UpdateOfferPhase::Available);
}

#[test]
fn external_installer_handoff_keeps_the_pending_update_retryable() {
    let update = PendingLinuxUpdate {
        manager: None,
        target: LinuxUpdateTarget::Deb(DebUpdate {
            version: "0.11.0-beta.2".to_owned(),
            name: "ok-player_0.11.0-beta.2_amd64.deb".to_owned(),
            url: "https://example.invalid/update.deb".to_owned(),
            size: Some(42),
            sums_url: None,
            expected_sha256: None,
        }),
    };
    let mut status = LinuxUpdateStatus::from_check_result(
        &LinuxUpdateCheckResult::Available(update),
        UpdateChannel::Public,
        &SkippedUpdateVersions::default(),
    );
    let LinuxUpdateStatus::Offer(offer) = &mut status else {
        panic!("available update should create an offer");
    };
    assert!(offer.state.start_install());
    let state = Rc::new(RefCell::new(PlayerState {
        linux_update_status: status,
        ..PlayerState::default()
    }));

    defer_update_install(
        &state,
        "Installer opened. Complete it to update.".to_owned(),
    );

    let restored = state
        .borrow()
        .linux_update_status
        .pending_offer()
        .expect("offer should survive external installer handoff");
    assert_eq!(restored.state.phase(), &UpdateOfferPhase::Available);
    assert!(restored.state.persistent_surface_visible());
    assert_eq!(restored.state.primary_action_label(), Some("Update"));
    assert_eq!(
        restored.status_text(),
        "Installer opened. Complete it to update."
    );
}

#[test]
fn candidate_22_check_bypasses_stale_shared_caches_and_selects_23() {
    let listener =
        std::net::TcpListener::bind("127.0.0.1:0").expect("candidate feed test server should bind");
    let address = listener
        .local_addr()
        .expect("candidate feed test address should resolve");
    let response_body = format!(
        r#"{{
  "channel": "candidate",
  "version": "0.11.0-beta.0.23",
  "build": 23,
  "commit_sha": "{}",
  "timestamp_utc": "2026-07-18T16:00:00Z",
  "acceptance": "accepted",
  "package": {{
    "name": "ok-player_0.11.0-beta.0.23_amd64.deb",
    "url": "https://example.invalid/ok-player_0.11.0-beta.0.23_amd64.deb",
    "size": 42,
    "sha256": "{}"
  }},
  "appimage": {{
    "package_id": "com.befeast.okplayer",
    "name": "com.befeast.okplayer-0.11.0-beta.0.23-linux-candidate-full.nupkg",
    "url": "https://example.invalid/com.befeast.okplayer-0.11.0-beta.0.23-linux-candidate-full.nupkg",
    "size": 84,
    "sha256": "{}",
    "sha1": "{}"
  }},
  "sha256sums_url": "https://example.invalid/SHA256SUMS-23.txt",
  "history": []
}}"#,
        "a".repeat(40),
        "b".repeat(64),
        "c".repeat(64),
        "d".repeat(40),
    );
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener
            .accept()
            .expect("candidate feed test request should connect");
        let mut request = [0_u8; 4096];
        let request_len = std::io::Read::read(&mut stream, &mut request)
            .expect("candidate feed test request should be readable");
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            response_body.len(),
            response_body
        );
        std::io::Write::write_all(&mut stream, response.as_bytes())
            .expect("candidate feed test response should be writable");
        String::from_utf8(request[..request_len].to_vec())
            .expect("candidate feed test request should be UTF-8")
    });

    let update = fetch_linux_candidate_update_from_url(
        &format!("http://{address}/candidate.linux.json?fixture=1"),
        "publish-23",
        "0.11.0-beta.0.22",
    )
    .expect("candidate feed check should succeed")
    .expect("newer accepted candidate should be selected");
    assert_eq!(update.build, 23);

    let request = server
        .join()
        .expect("candidate feed test server should stop");
    let request = request.to_ascii_lowercase();
    assert!(
        request.starts_with(
            "get /candidate.linux.json?fixture=1&okp-cache-bust=publish-23 http/1.1\r\n"
        )
    );
    assert!(request.contains("\r\naccept: application/json\r\n"));
    assert!(request.contains("\r\ncache-control: no-cache\r\n"));
    assert!(request.contains("\r\npragma: no-cache\r\n"));
    assert!(request.contains("\r\nuser-agent: ok player linux\r\n"));
}

#[test]
fn candidate_feed_cache_bust_changes_for_every_check() {
    assert_ne!(candidate_feed_cache_bust(), candidate_feed_cache_bust());
}

#[test]
fn candidate_deb_feed_sha_mismatch_is_refused_before_download() {
    let name = "ok-player_0.11.0-beta.1.42_amd64.deb";
    let update = DebUpdate {
        version: "0.11.0-beta.1.42".to_owned(),
        name: name.to_owned(),
        url: "https://example.invalid/update.deb".to_owned(),
        size: Some(42),
        sums_url: Some("https://example.invalid/SHA256SUMS-42.txt".to_owned()),
        expected_sha256: Some("a".repeat(64)),
    };
    let manifest = format!("{}  {name}\n", "b".repeat(64));

    let error = verify_deb_feed_identity(&update, &manifest)
        .expect_err("candidate feed and checksum manifest must agree");
    assert!(error.contains("Candidate identity check failed"));
    assert!(error.contains("SHA mismatch"));
}

#[test]
fn candidate_appimage_source_uses_the_manifest_bound_full_package() {
    let package = CandidateAppImage {
        package_id: "com.befeast.okplayer".to_owned(),
        name: "com.befeast.okplayer-0.11.0-beta.1.42-linux-candidate-full.nupkg".to_owned(),
        url: "https://example.invalid/linux-candidate/full.nupkg".to_owned(),
        size: 1234,
        sha256: "a".repeat(64),
        sha1: "b".repeat(40),
    };
    let asset = candidate_velopack_asset(&package, "0.11.0-beta.1.42");

    assert_eq!(asset.PackageId, package.package_id);
    assert_eq!(asset.Version, "0.11.0-beta.1.42");
    assert_eq!(asset.Type, "Full");
    assert_eq!(asset.FileName, package.name);
    assert_eq!(asset.SHA256, package.sha256);

    let builder = include_str!("../../../../scripts/build-linux-candidate.sh");
    assert!(builder.contains("OKP_LINUX_CHANNEL=linux-candidate"));
}

#[test]
fn linux_packages_stamp_their_update_install_lane() {
    let deb = include_str!("../../../../scripts/package-linux-deb.sh");
    let appimage = include_str!("../../../../scripts/package-linux-velopack.sh");
    let rpm = include_str!("../../../packaging/fedora/ok-player.spec");

    assert!(deb.contains("OKP_PACKAGE_KIND=deb"));
    assert!(appimage.contains("OKP_PACKAGE_KIND=appimage"));
    assert_eq!(rpm.matches("OKP_PACKAGE_KIND=rpm").count(), 2);
}

#[test]
fn candidate_builder_defaults_below_beta_one_until_the_base_is_overridden() {
    use okp_core::candidate_build::candidate_version;
    use okp_core::update_selection::compare_versions;
    use std::cmp::Ordering;

    let builder = include_str!("../../../../scripts/build-linux-candidate.sh");
    assert!(
        builder.contains("VERSION_BASE=\"${OKP_CANDIDATE_VERSION_BASE:-0.11.0-beta.0}\""),
        "a clean builder invocation must use the pre-beta base while preserving the override"
    );
    assert!(builder.contains("version --base \"$VERSION_BASE\" --build \"$BUILD_NUMBER\""));
    assert!(builder.contains("--version \"$VERSION\""));
    assert!(builder.contains(">\"$OUT_DIR/candidate-build.json\""));

    let first = candidate_version("0.11.0-beta.0", 108).unwrap();
    let second = candidate_version("0.11.0-beta.0", 109).unwrap();
    assert_eq!(compare_versions(&second, &first), Ordering::Greater);
    assert_eq!(
        compare_versions("0.11.0-beta.1", &second),
        Ordering::Greater,
        "sequential pre-beta candidates must remain below the first public beta"
    );

    let post_beta = candidate_version("0.11.0-beta.1", 110).unwrap();
    assert_eq!(
        compare_versions(&post_beta, "0.11.0-beta.1"),
        Ordering::Greater,
        "the explicit post-beta base must move candidates above the public beta"
    );
}

#[test]
fn candidate_workflow_holds_one_close_on_exec_build_publish_section() {
    let workflow = include_str!("../../../../.github/workflows/release-linux-candidate.yml");
    let runner = include_str!("../../../../scripts/run-linux-candidate-workflow.sh");
    let builder = include_str!("../../../../scripts/build-linux-candidate.sh");
    let publisher = include_str!("../../../../scripts/publish-linux-candidate.sh");

    assert!(workflow.contains("--phase build-and-publish"));
    assert!(workflow.contains("-- ./scripts/run-linux-candidate-workflow.sh"));
    assert!(runner.contains("\"$ROOT/scripts/build-linux-candidate.sh\""));
    assert!(runner.contains("\"$STATE_DIR/checkout/scripts/publish-linux-candidate.sh\""));
    assert!(runner.contains("BUNDLE=\"$(cat \"$STATE_DIR/last-bundle.path\")\""));
    assert!(builder.contains("OKP_CANDIDATE_LOCK_HELD"));
    assert!(publisher.contains("OKP_CANDIDATE_LOCK_HELD"));
    assert!(!builder.contains("exec 9>"));
    assert!(!publisher.contains("exec 9>"));
}

#[test]
fn candidate_publish_retry_reuses_exact_assets_and_keeps_pointer_last() {
    let publisher = include_str!("../../../../scripts/publish-linux-candidate.sh");
    let reuse = publisher
        .find("upload_exact_asset \"$BUNDLE/artifacts/deb/$deb_name\"")
        .expect("versioned asset reuse should be present");
    let pointer = publisher
        .find("gh release upload \"$TAG\" --repo \"$REPO\" \"$feed\" --clobber")
        .expect("candidate pointer upload should be present");

    assert!(
        reuse < pointer,
        "the rolling pointer must remain the commit point"
    );
    assert!(publisher.contains("cmp -s -- \"$source\" \"$existing_dir/$name\""));
    assert!(publisher.contains("pointer_committed=false"));
    assert!(publisher.contains("failed to restore the previous candidate pointer"));
    assert!(publisher.contains("gh release delete-asset \"$TAG\" \"$asset\""));
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
    assert!(state.last_load_diagnostic.is_none());
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
    assert!(state.last_load_diagnostic.is_none());
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
    assert_eq!(
        state
            .last_load_diagnostic
            .as_ref()
            .map(|diagnostic| diagnostic.detail.as_str()),
        Some("libmpv error 404")
    );
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
    assert_eq!(
        state
            .last_load_diagnostic
            .as_ref()
            .map(|diagnostic| diagnostic.detail.as_str()),
        Some("libmpv error 7")
    );
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
    assert_eq!(
        state
            .last_load_diagnostic
            .as_ref()
            .map(|diagnostic| diagnostic.detail.as_str()),
        Some("libmpv error 7")
    );
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

    apply_endfile_error(&state, 412, Some("https://example.com/live.m3u8"), &[]);

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
    assert_eq!(
        state
            .last_load_diagnostic
            .as_ref()
            .map(|diagnostic| diagnostic.detail.as_str()),
        Some("libmpv error 412")
    );
}

#[test]
fn codec_failure_reported_at_eof_stays_on_the_failed_source() {
    let state = Rc::new(RefCell::new(PlayerState {
        current_file: Some(PathBuf::from("/media/movie.mkv")),
        media_load_state: network_media::MediaLoadState::Playing,
        ..PlayerState::default()
    }));

    assert!(apply_endfile_eof_diagnostic(
        &state,
        Some("/media/movie.mkv"),
        &["[ffmpeg/video] Decoder not found for codec hevc".to_owned()],
    ));

    let state = state.borrow();
    assert_eq!(
        state.media_load_state,
        network_media::MediaLoadState::Failed
    );
    assert_eq!(
        state
            .last_load_diagnostic
            .as_ref()
            .map(|diagnostic| diagnostic.kind),
        Some(okp_core::playback_failure::PlaybackFailureKind::MissingCodec)
    );
}

#[test]
fn benign_eof_keeps_the_normal_playlist_path_available() {
    let state = Rc::new(RefCell::new(PlayerState {
        current_file: Some(PathBuf::from("/media/movie.mkv")),
        media_load_state: network_media::MediaLoadState::Playing,
        ..PlayerState::default()
    }));

    assert!(!apply_endfile_eof_diagnostic(
        &state,
        Some("/media/movie.mkv"),
        &["cplayer: finished playback".to_owned()],
    ));
    assert_eq!(
        state.borrow().media_load_state,
        network_media::MediaLoadState::Playing
    );
}

#[test]
fn eof_without_an_auto_advance_target_returns_to_idle() {
    let path = PathBuf::from("/media/movie.mkv");
    let item = PlaylistItem::Local(path.clone());
    let mut playlist = Playlist::from_items(vec![item.clone()], Some(&item), false);
    playlist.set_auto_advance(false);
    let state = Rc::new(RefCell::new(PlayerState {
        current_file: Some(path),
        playlist,
        media_load_state: network_media::MediaLoadState::Playing,
        ..PlayerState::default()
    }));

    assert!(!advance_playlist_on_eof(&state));

    let state = state.borrow();
    assert!(state.current_file.is_none());
    assert!(state.playlist.items().is_empty());
    assert_eq!(state.media_load_state, network_media::MediaLoadState::Idle);
}

#[test]
fn idle_return_smoke_waits_for_natural_eof_before_welcome_capture() {
    let smoke = include_str!("../../../../scripts/smoke-linux-idle-return.sh");
    let eof_flow = smoke
        .split("launch_fixture eof-app")
        .nth(1)
        .and_then(|source| source.split("stop_app").next())
        .expect("EOF smoke flow");

    let loaded_probe = eof_flow
        .find("idle-return-smoke: file-loaded")
        .expect("file-loaded lifecycle probe");
    let eof_probe = eof_flow
        .find("idle-return-smoke: eof-idle")
        .expect("natural-EOF idle probe");
    let welcome_probe = eof_flow
        .find("assert_idle_capture")
        .expect("Welcome identity probe");
    assert!(loaded_probe < eof_probe);
    assert!(eof_probe < welcome_probe);
    assert!(!eof_flow.contains("sleep 7"));

    assert!(smoke.contains("identity > 0.012"));
    assert!(smoke.contains("magenta < 0.35"));
    assert!(smoke.contains("idle-return-smoke: close-idle"));
    assert!(smoke.contains("export GSK_RENDERER=cairo"));
    assert!(smoke.contains("-crop 1120x638+0+42"));

    let lifecycle = include_str!("track_popovers.rs");
    assert!(lifecycle.contains("idle-return-smoke: file-loaded"));
    assert!(lifecycle.contains("idle-return-smoke: eof-idle"));
    let playback = include_str!("playback.rs");
    assert!(playback.contains("idle-return-smoke: close-idle"));
}

#[test]
fn native_video_background_is_transparent_only_while_media_is_active() {
    let css = include_str!("css.rs");
    let bridge = include_str!("mpv_bridge.rs");

    assert!(css.contains(".okp-root.okp-native-video.has-active-video-plane"));
    assert!(css.contains("window.okp-player-window.okp-native-video.has-active-video-plane"));
    assert!(!css.contains("window.okp-player-window.okp-native-video,\n"));
    assert!(bridge.contains("sync_native_video_background(&window, &root_surface, has_media)"));
    assert!(bridge.contains("remove_css_class(\"has-active-video-plane\")"));
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

    apply_endfile_error(&state, 7, Some("/media/movie.mkv"), &[]);

    let state = state.borrow();
    assert_eq!(
        state.media_load_state,
        network_media::MediaLoadState::Failed
    );
    assert_eq!(
        state.retry_load_source.as_ref(),
        Some(&network_media::LoadFailureSource::local(path))
    );
    assert_eq!(
        state
            .last_load_diagnostic
            .as_ref()
            .map(|diagnostic| diagnostic.detail.as_str()),
        Some("libmpv error 7")
    );
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

    apply_endfile_error(&state, 412, Some("https://example.com/a.m3u8"), &[]);

    let state = state.borrow();
    // The surface stays Loading for B and B's retry URL is untouched, so B's own
    // lifecycle (FileLoaded / its own EndFile) resolves it.
    assert_eq!(
        state.media_load_state,
        network_media::MediaLoadState::Loading
    );
    assert!(state.last_load_diagnostic.is_none());
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

    apply_endfile_error(&state, 7, Some("/media/movie.mkv"), &[]);

    let state = state.borrow();
    assert_eq!(state.media_load_state, network_media::MediaLoadState::Idle);
    assert!(state.retry_load_source.is_none());
    assert!(state.last_load_diagnostic.is_none());
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

    apply_endfile_error(&state, 7, None, &[]);

    let state = state.borrow();
    assert_eq!(state.media_load_state, network_media::MediaLoadState::Idle);
    assert!(state.retry_load_source.is_none());
    assert!(state.last_load_diagnostic.is_none());
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

    apply_endfile_error(&state, 412, None, &[]);

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
    assert_eq!(
        state
            .last_load_diagnostic
            .as_ref()
            .map(|diagnostic| diagnostic.detail.as_str()),
        Some("libmpv error 412")
    );
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
    assert!(state.last_load_diagnostic.is_none());
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

#[test]
fn settings_and_about_open_path_does_not_block_on_metadata_probes() {
    // The synchronous activation path must not call the expensive metadata probes
    // that previously stalled the first mapped frame of the Settings window.
    let settings_window = include_str!("settings_window.rs");
    let about = include_str!("about.rs");
    let updates = include_str!("updates.rs");

    for probe in [
        "pkg_config_version",
        "ffmpeg_version",
        "linux_os_label",
        "linux_update_install_status",
    ] {
        assert!(
            !settings_window.contains(probe),
            "settings_window.rs must not call {probe} synchronously"
        );
    }

    // Expensive metadata is captured in a dedicated async helper and dispatched from
    // a background thread so the UI maps before the probes resolve.
    assert!(
        about.contains("AboutDeferredFields"),
        "about.rs must have a deferred metadata struct"
    );
    assert!(
        about.contains("std::thread::spawn"),
        "about.rs must spawn the deferred capture"
    );
    assert!(
        about.contains("AboutDeferredFields::capture()"),
        "about.rs must capture deferred metadata via the helper"
    );
    assert!(
        about.contains("capture_cheap"),
        "about.rs must use the cheap synchronous snapshot for the first frame"
    );

    // The Updates page also resolves the install mode off the main thread.
    assert!(updates.contains("linux_update_install_status"));
    let updates_after_spawn = updates
        .split("std::thread::spawn")
        .nth(1)
        .expect("updates.rs spawns a thread for the install probe");
    assert!(
        updates_after_spawn.contains("linux_update_install_status"),
        "updates.rs must probe install status in a background thread"
    );
}

#[test]
fn settings_first_map_builds_only_the_requested_page() {
    let source = include_str!("settings_window.rs");
    let open_path = source
        .split_once("pub(crate) fn open_settings_window(")
        .expect("Settings entry point")
        .1
        .split_once("pub(crate) fn apply_settings_window_theme(")
        .expect("theme helper follows Settings entry point")
        .0;

    assert_eq!(
        open_path
            .matches("ensure_page(&stack, initial_page)")
            .count(),
        1
    );
    for eager_page_constructor in [
        "settings_about_section(",
        "settings_appearance_section(",
        "settings_advanced_page(",
        "settings_updates_page(",
        "settings_subtitles_page(",
        "settings_audio_page(",
        "settings_shortcuts_section(",
        "settings_integration_section(",
    ] {
        assert!(
            !open_path.contains(eager_page_constructor),
            "the synchronous window entry point must not eagerly call {eager_page_constructor}"
        );
    }

    let builder = source
        .split_once("impl SettingsPageBuilder {")
        .expect("lazy Settings page builder")
        .1
        .split_once("pub(crate) fn open_settings_window(")
        .expect("entry point follows lazy page builder")
        .0;
    assert!(builder.contains("stack.child_by_name(page.id()).is_some()"));
    assert!(builder.contains("Some(page.id())"));
}
