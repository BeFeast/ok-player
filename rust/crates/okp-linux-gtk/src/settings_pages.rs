use super::*;

pub(crate) fn settings_subtitles_page(
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) -> gtk::Box {
    let page = gtk::Box::new(gtk::Orientation::Vertical, 12);
    page.add_css_class("okp-settings-page");

    let snapshot = settings_subtitle_snapshot(&state);
    page.append(&settings_subtitle_presentation_section(
        Rc::clone(&state),
        Rc::clone(&status_toast),
    ));

    let summary = settings_section("Current Media");
    summary.append(&settings_value_row("Primary", &snapshot.primary));
    summary.append(&settings_value_row("Secondary", &snapshot.secondary));

    let (delay_row, delay_label, delay_buttons) = settings_stepper_row(
        "Delay",
        &subtitle_delay::format_label(snapshot.delay_seconds),
        &[
            ("-50 ms", SubtitleAdjustment::Delay(-0.05)),
            ("+50 ms", SubtitleAdjustment::Delay(0.05)),
            ("Reset", SubtitleAdjustment::SetDelay(0.0)),
        ],
    );
    let projected_delay = Rc::new(Cell::new(snapshot.delay_seconds));

    for (button, adjustment) in delay_buttons {
        button.set_sensitive(snapshot.has_media);
        let button_state = Rc::clone(&state);
        let button_toast = Rc::clone(&status_toast);
        let button_delay = delay_label.clone();
        let projected_delay = Rc::clone(&projected_delay);
        button.connect_clicked(move |_| {
            if let Some(applied_delay) =
                apply_subtitle_adjustment(&button_state, adjustment, projected_delay.get())
            {
                projected_delay.set(applied_delay);
            }
            button_delay.set_text(&subtitle_delay::format_label(projected_delay.get()));
            button_toast.show("Subtitle settings updated");
        });
    }
    summary.append(&delay_row);

    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    actions.add_css_class("okp-settings-action-row");
    actions.set_halign(gtk::Align::End);
    let add_button = gtk::Button::with_label("Add subtitle...");
    add_button.add_css_class("okp-settings-button");
    add_button.set_sensitive(snapshot.has_media);
    let add_parent = parent.clone();
    let add_state = Rc::clone(&state);
    add_button.connect_clicked(move |_| open_subtitle_dialog(&add_parent, Rc::clone(&add_state)));
    actions.append(&add_button);
    summary.append(&actions);
    page.append(&summary);

    // Mirror the flyout: only surface the secondary picker once a dual-subtitle
    // choice exists (≥2 tracks) or a secondary is already active.
    let offer_secondary = okp_core::subtitle_tracks::can_offer_secondary(
        read_tracks(&state)
            .into_iter()
            .filter(|track| track.kind == TrackKind::Subtitle)
            .count(),
        read_secondary_subtitle_id(&state).is_some(),
    );

    page.append(&settings_subtitle_track_section(
        "Primary Track",
        false,
        Rc::clone(&state),
        Rc::clone(&status_toast),
    ));
    if offer_secondary {
        page.append(&settings_subtitle_track_section(
            "Secondary Track",
            true,
            state,
            status_toast,
        ));
    }

    page
}

pub(crate) fn settings_subtitle_presentation_section(
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) -> gtk::Box {
    let section = settings_section("Presentation");
    let (scale, position, style) = {
        let state = state.borrow();
        (
            state.settings.subtitle_scale(),
            state.settings.subtitle_position(),
            state.settings.subtitle_style(),
        )
    };
    let tracks = read_tracks(&state)
        .into_iter()
        .filter(|track| track.kind == TrackKind::Subtitle)
        .collect::<Vec<_>>();
    let applicability =
        primary_subtitle_preset_applicability(&tracks, read_secondary_subtitle_id(&state));

    let scale_values = [0.8, 1.0, 1.4];
    let (scale_row, scale_buttons) = settings_segmented_choice_row(
        "Size",
        &[
            ("Small", (scale - scale_values[0]).abs() < 0.001),
            ("Normal", (scale - scale_values[1]).abs() < 0.001),
            ("Large", (scale - scale_values[2]).abs() < 0.001),
        ],
    );
    let scale_buttons = Rc::new(RefCell::new(scale_buttons));
    for (button, value) in scale_buttons.borrow().iter().cloned().zip(scale_values) {
        let selected = button.clone();
        let buttons = Rc::clone(&scale_buttons);
        let button_state = Rc::clone(&state);
        let button_toast = Rc::clone(&status_toast);
        button.connect_clicked(move |_| {
            if save_subtitle_scale_default(&button_state, value, &button_toast) {
                mark_settings_track_selected(&buttons, &selected);
            }
        });
    }
    section.append(&scale_row);

    let position_values = [100_i64, 90_i64];
    let (position_row, position_buttons) = settings_segmented_choice_row(
        "Position",
        &[
            ("Standard", position == position_values[0]),
            ("Raised", position == position_values[1]),
        ],
    );
    let position_buttons = Rc::new(RefCell::new(position_buttons));
    for (button, value) in position_buttons
        .borrow()
        .iter()
        .cloned()
        .zip(position_values)
    {
        let selected = button.clone();
        let buttons = Rc::clone(&position_buttons);
        let button_state = Rc::clone(&state);
        let button_toast = Rc::clone(&status_toast);
        button.connect_clicked(move |_| {
            if save_subtitle_position_default(&button_state, value, &button_toast) {
                mark_settings_track_selected(&buttons, &selected);
            }
        });
    }
    section.append(&position_row);

    let style_values = ["Default", "Bold", "Classic", "Contrast"];
    let (style_row, style_buttons) = settings_segmented_choice_row(
        "Style",
        &[
            ("Default", style.key == style_values[0]),
            ("Bold", style.key == style_values[1]),
            ("Classic", style.key == style_values[2]),
            ("High contrast", style.key == style_values[3]),
        ],
    );
    for button in &style_buttons {
        button.add_css_class("okp-subtitle-style-choice");
    }
    let style_buttons = Rc::new(RefCell::new(style_buttons));
    for (button, key) in style_buttons.borrow().iter().cloned().zip(style_values) {
        let selected = button.clone();
        let buttons = Rc::clone(&style_buttons);
        let button_state = Rc::clone(&state);
        let button_toast = Rc::clone(&status_toast);
        button.connect_clicked(move |_| {
            if save_subtitle_style_default(&button_state, key, &button_toast) {
                mark_settings_track_selected(&buttons, &selected);
            }
        });
    }
    section.append(&style_row);

    let hint = gtk::Label::new(Some(&settings_subtitle_preset_hint(applicability)));
    hint.add_css_class("okp-settings-hint");
    hint.set_xalign(0.0);
    hint.set_wrap(true);
    hint.set_max_width_chars(52);
    section.append(&hint);

    section
}

pub(crate) fn settings_subtitle_preset_hint(
    applicability: okp_core::subtitle_tracks::SubtitlePresetApplicability,
) -> String {
    use okp_core::subtitle_tracks::{SubtitlePresetApplicability, SubtitlePresetFormat};

    match applicability {
        SubtitlePresetApplicability::Applies(format) => format!(
            "The current {} track uses the selected OK Player preset.",
            format.display_name()
        ),
        SubtitlePresetApplicability::NativeStyle(format) => format!(
            "The current {} track keeps its authored native styling. Preset changes are saved for text tracks; size and position still apply.",
            format.display_name()
        ),
        SubtitlePresetApplicability::Unsupported(SubtitlePresetFormat::Image) => {
            "The current image subtitle cannot use appearance presets. Preset changes are saved for supported text tracks.".to_owned()
        }
        SubtitlePresetApplicability::Unsupported(_) => {
            "Preset support is unavailable for the current subtitle format. Changes are saved for supported text tracks.".to_owned()
        }
        SubtitlePresetApplicability::NoActiveTrack => {
            "Presets apply to supported text subtitles. ASS/SSA keeps authored native styling, and image subtitles keep their bitmap appearance; size and position still apply.".to_owned()
        }
    }
}

pub(crate) fn settings_segmented_choice_row(
    label: &str,
    choices: &[(&str, bool)],
) -> (gtk::Box, Vec<gtk::Button>) {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    row.add_css_class("okp-settings-row");

    let label = gtk::Label::new(Some(label));
    label.add_css_class("okp-info-label");
    label.set_xalign(0.0);
    label.set_width_chars(10);
    label.set_hexpand(true);
    row.append(&label);

    let group = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    group.add_css_class("okp-settings-segmented");
    group.set_halign(gtk::Align::End);
    let mut buttons = Vec::with_capacity(choices.len());
    for (label, selected) in choices {
        let button = gtk::Button::with_label(label);
        button.add_css_class("okp-settings-segment-button");
        button.set_has_frame(false);
        if *selected {
            button.add_css_class("is-selected");
        }
        group.append(&button);
        buttons.push(button);
    }
    row.append(&group);
    (row, buttons)
}

pub(crate) fn save_subtitle_scale_default(
    state: &Rc<RefCell<PlayerState>>,
    scale: f64,
    status_toast: &StatusToast,
) -> bool {
    {
        let mut state = state.borrow_mut();
        state.settings.set_subtitle_scale(scale);
        if !save_settings_or_toast(&mut state, status_toast) {
            return false;
        }
    }
    if with_mpv(state, |mpv| mpv.set_subtitle_scale(scale)) {
        save_current_preferences_with_subtitle_scale(state, scale);
    }
    status_toast.show("Subtitle size updated");
    true
}

pub(crate) fn save_subtitle_position_default(
    state: &Rc<RefCell<PlayerState>>,
    position: i64,
    status_toast: &StatusToast,
) -> bool {
    let mut state = state.borrow_mut();
    state.settings.set_subtitle_position(position);
    if save_settings_or_toast(&mut state, status_toast) {
        status_toast.show("Subtitle position updated");
        true
    } else {
        false
    }
}

pub(crate) fn save_subtitle_style_default(
    state: &Rc<RefCell<PlayerState>>,
    key: &str,
    status_toast: &StatusToast,
) -> bool {
    if let Err(message) = set_subtitle_style_setting(state, key) {
        status_toast.show(message);
        false
    } else {
        status_toast.show("Subtitle style updated");
        true
    }
}

pub(crate) fn settings_audio_page(
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) -> gtk::Box {
    let page = gtk::Box::new(gtk::Orientation::Vertical, 12);
    page.add_css_class("okp-settings-page");

    let summary = settings_section("Audio");
    summary.append(&settings_value_row(
        "Current track",
        &selected_track_summary(&state, TrackKind::Audio),
    ));
    summary.append(&settings_volume_row(Rc::clone(&state)));
    let audio_delay = audio_delay_adjustment_row(read_audio_delay(&state), &state, &status_toast);
    audio_delay.add_css_class("okp-settings-audio-delay-row");
    summary.append(&audio_delay);
    summary.append(&settings_audio_normalization_row(
        Rc::clone(&state),
        Rc::clone(&status_toast),
    ));
    summary.append(&settings_surround_downmix_row(
        Rc::clone(&state),
        Rc::clone(&status_toast),
    ));
    page.append(&summary);
    page.append(&settings_audio_device_section(
        Rc::clone(&state),
        Rc::clone(&status_toast),
    ));
    page.append(&settings_audio_track_section(state, status_toast));

    page
}

pub(crate) fn settings_screenshot_section(
    parent: &gtk::Window,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) -> gtk::Box {
    let section = settings_section("Screenshots");

    let format_row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    format_row.add_css_class("okp-settings-row");
    let format_label = gtk::Label::new(Some("Format"));
    format_label.add_css_class("okp-info-label");
    format_label.set_xalign(0.0);
    format_label.set_width_chars(14);
    format_label.set_hexpand(true);
    format_row.append(&format_label);

    let selected_format = state.borrow().settings.screenshot_format();
    let format_buttons = Rc::new(RefCell::new(Vec::<(
        gtk::Button,
        okp_core::settings::ScreenshotFormat,
    )>::new()));
    let format_group = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    format_group.add_css_class("okp-settings-stepper-group");
    for format in okp_core::settings::ScreenshotFormat::ALL {
        let button = gtk::Button::with_label(format.label());
        button.add_css_class("okp-settings-button");
        button.add_css_class("okp-screenshot-format-button");
        if format == selected_format {
            button.add_css_class("is-selected");
        }
        let button_state = Rc::clone(&state);
        let button_toast = Rc::clone(&status_toast);
        let buttons = Rc::clone(&format_buttons);
        button.connect_clicked(move |_| {
            let saved = {
                let mut state = button_state.borrow_mut();
                state.settings.set_screenshot_format(format);
                save_settings_or_toast(&mut state, &button_toast)
            };
            for (button, button_format) in buttons.borrow().iter() {
                if *button_format == format {
                    button.add_css_class("is-selected");
                } else {
                    button.remove_css_class("is-selected");
                }
            }
            if saved {
                button_toast.show(&format!("Screenshot format: {}", format.label()));
            }
        });
        format_group.append(&button);
        format_buttons.borrow_mut().push((button, format));
    }
    format_row.append(&format_group);
    section.append(&format_row);

    let configured_directory = state.borrow().settings.screenshot_directory();
    let displayed_directory = configured_directory
        .clone()
        .unwrap_or_else(screenshots::default_screenshot_dir);
    let (directory_row, directory_label) =
        settings_value_row_with_label("Save folder", &displayed_directory.to_string_lossy());

    let reset = gtk::Button::with_label("Default");
    reset.add_css_class("okp-settings-button");
    reset.set_sensitive(configured_directory.is_some());

    let choose = gtk::Button::with_label("Choose...");
    choose.add_css_class("okp-settings-button");
    let choose_parent = parent.clone();
    let choose_state = Rc::clone(&state);
    let choose_toast = Rc::clone(&status_toast);
    let choose_label = directory_label.clone();
    let choose_reset = reset.clone();
    choose.connect_clicked(move |_| {
        open_screenshot_folder_dialog(
            &choose_parent,
            Rc::clone(&choose_state),
            Rc::clone(&choose_toast),
            choose_label.clone(),
            choose_reset.clone(),
        );
    });
    directory_row.append(&choose);

    let reset_state = state;
    let reset_toast = status_toast;
    let reset_label = directory_label;
    reset.connect_clicked(move |button| {
        let default_directory = screenshots::default_screenshot_dir();
        let saved = {
            let mut state = reset_state.borrow_mut();
            state.settings.set_screenshot_directory(None);
            save_settings_or_toast(&mut state, &reset_toast)
        };
        reset_label.set_text(&default_directory.to_string_lossy());
        button.set_sensitive(false);
        if saved {
            reset_toast.show("Screenshot folder reset");
        }
    });
    directory_row.append(&reset);
    section.append(&directory_row);

    section
}

#[allow(deprecated)]
fn open_screenshot_folder_dialog(
    parent: &gtk::Window,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
    directory_label: gtk::Label,
    reset_button: gtk::Button,
) {
    let dialog = gtk::FileChooserDialog::new(
        Some("Choose screenshot folder"),
        Some(parent),
        gtk::FileChooserAction::SelectFolder,
        &[
            ("Cancel", gtk::ResponseType::Cancel),
            ("Choose", gtk::ResponseType::Accept),
        ],
    );
    dialog.set_modal(true);
    dialog.set_decorated(false);
    let current = state
        .borrow()
        .settings
        .screenshot_directory()
        .unwrap_or_else(screenshots::default_screenshot_dir);
    let _ = dialog.set_current_folder(Some(&gtk::gio::File::for_path(current)));

    dialog.connect_response(move |dialog, response| {
        if response == gtk::ResponseType::Accept
            && let Some(path) = dialog.file().and_then(|file| file.path())
        {
            let (accepted, saved) = {
                let mut state = state.borrow_mut();
                let accepted = state.settings.set_screenshot_directory(Some(&path));
                let saved = accepted && save_settings_or_toast(&mut state, &status_toast);
                (accepted, saved)
            };
            if accepted {
                directory_label.set_text(&path.to_string_lossy());
                reset_button.set_sensitive(true);
                if saved {
                    status_toast.show("Screenshot folder updated");
                }
            } else {
                status_toast.show("Choose an absolute local folder");
            }
        }
        dialog.close();
    });
    dialog.present();
}

pub(crate) fn settings_resume_row(
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) -> gtk::Box {
    let active = state.borrow().settings.resume_enabled();
    settings_playback_switch_row(
        "Resume playback",
        "Reopen files at the saved position, skipping the first 5% and final stretch.",
        active,
        state,
        status_toast,
        |state, enabled| state.settings.set_resume_enabled(enabled),
        "Resume playback",
    )
}

pub(crate) fn settings_auto_advance_row(
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) -> gtk::Box {
    let active = state.borrow().playlist.auto_advance();
    settings_playback_switch_row(
        "Auto-advance",
        "Continue to the next item in the folder or playlist when a file ends.",
        active,
        state,
        status_toast,
        |state, enabled| {
            state.playlist.set_auto_advance(enabled);
            state.settings.set_auto_advance_enabled(enabled);
        },
        "Auto-advance",
    )
}

pub(crate) fn settings_shuffle_row(
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) -> gtk::Box {
    let active = state.borrow().playlist.shuffle();
    settings_playback_switch_row(
        "Shuffle default",
        "Start folders and playlists in shuffled order, without immediate repeats.",
        active,
        state,
        status_toast,
        |state, enabled| {
            state.playlist.set_shuffle(enabled);
            state.settings.set_shuffle_enabled(enabled);
        },
        "Shuffle",
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GaplessSettingState {
    pub enabled: bool,
    pub can_toggle: bool,
    pub state_label: &'static str,
    pub action_label: &'static str,
    pub detail: &'static str,
}

pub(crate) fn gapless_setting_state(
    enabled: bool,
    capability: GaplessPlaybackCapability,
) -> GaplessSettingState {
    match capability {
        GaplessPlaybackCapability::Available => GaplessSettingState {
            enabled,
            can_toggle: true,
            state_label: if enabled { "On" } else { "Off" },
            action_label: if enabled { "Turn off" } else { "Turn on" },
            detail: "Keep compatible playlist entries on one continuous audio output path.",
        },
        GaplessPlaybackCapability::Deferred => GaplessSettingState {
            enabled: false,
            can_toggle: false,
            state_label: "Deferred",
            action_label: "Unavailable",
            detail: "OK Player currently loads the next item after end-of-file, so continuous audio cannot be guaranteed.",
        },
    }
}

pub(crate) fn settings_gapless_row(
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) -> gtk::Box {
    let setting = gapless_setting_state(
        state
            .borrow()
            .settings
            .gapless_enabled(LINUX_GAPLESS_CAPABILITY),
        LINUX_GAPLESS_CAPABILITY,
    );
    eprintln!(
        "playback capability: gapless={}",
        if setting.can_toggle {
            "available"
        } else {
            "deferred"
        }
    );

    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row.add_css_class("okp-settings-switch-row");

    let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
    text.set_hexpand(true);
    let label = gtk::Label::new(Some("Gapless playback"));
    label.add_css_class("okp-info-label");
    label.set_xalign(0.0);
    text.append(&label);
    let detail = gtk::Label::new(Some(setting.detail));
    detail.add_css_class("okp-update-status");
    detail.set_xalign(0.0);
    detail.set_width_chars(1);
    detail.set_max_width_chars(50);
    detail.set_wrap(true);
    text.append(&detail);
    row.append(&text);

    let state_label = gtk::Label::new(Some(setting.state_label));
    state_label.add_css_class("okp-settings-state-pill");
    state_label.set_valign(gtk::Align::Center);
    row.append(&state_label);

    let button = gtk::Button::with_label(setting.action_label);
    button.add_css_class("okp-settings-button");
    button.set_valign(gtk::Align::Center);
    button.set_sensitive(setting.can_toggle);
    let button_state = Rc::clone(&state);
    let button_toast = Rc::clone(&status_toast);
    let button_state_label = state_label.clone();
    button.connect_clicked(move |button| {
        let current = gapless_setting_state(
            button_state
                .borrow()
                .settings
                .gapless_enabled(LINUX_GAPLESS_CAPABILITY),
            LINUX_GAPLESS_CAPABILITY,
        );
        if !current.can_toggle {
            return;
        }
        let enabled = !current.enabled;
        {
            let mut state = button_state.borrow_mut();
            if !state
                .settings
                .set_gapless_enabled(LINUX_GAPLESS_CAPABILITY, enabled)
            {
                return;
            }
            save_settings_or_toast(&mut state, &button_toast);
        }
        button_state_label.set_text(if enabled { "On" } else { "Off" });
        button.set_label(if enabled { "Turn off" } else { "Turn on" });
        button_toast.show(if enabled {
            "Gapless playback on"
        } else {
            "Gapless playback off"
        });
    });
    row.append(&button);

    row
}

pub(crate) fn settings_playback_switch_row<F>(
    title: &str,
    detail: &str,
    active: bool,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
    apply: F,
    toast_subject: &'static str,
) -> gtk::Box
where
    F: Fn(&mut PlayerState, bool) + 'static,
{
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row.add_css_class("okp-settings-switch-row");

    let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
    text.set_hexpand(true);
    let label = gtk::Label::new(Some(title));
    label.add_css_class("okp-info-label");
    label.set_xalign(0.0);
    text.append(&label);
    let detail = gtk::Label::new(Some(detail));
    detail.add_css_class("okp-update-status");
    detail.set_xalign(0.0);
    detail.set_width_chars(1);
    detail.set_max_width_chars(50);
    detail.set_wrap(true);
    text.append(&detail);
    row.append(&text);

    let state_label = gtk::Label::new(Some(if active { "On" } else { "Off" }));
    state_label.add_css_class("okp-settings-state-pill");
    state_label.set_valign(gtk::Align::Center);
    row.append(&state_label);

    let toggle = about_toggle_button(active);
    let toggle_state = Rc::clone(&state);
    let toggle_toast = Rc::clone(&status_toast);
    let toggle_state_label = state_label.clone();
    toggle.connect_clicked(move |button| {
        let enabled = !button.has_css_class("is-active");
        set_about_toggle_active(button, enabled);
        {
            let mut state = toggle_state.borrow_mut();
            apply(&mut state, enabled);
            save_settings_or_toast(&mut state, &toggle_toast);
        }
        toggle_state_label.set_text(if enabled { "On" } else { "Off" });
        toggle_toast.show(&format!(
            "{toast_subject} {}",
            if enabled { "on" } else { "off" }
        ));
    });
    row.append(&toggle);

    row
}

pub(crate) fn settings_repeat_row(
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row.add_css_class("okp-settings-switch-row");

    let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
    text.set_hexpand(true);
    let label = gtk::Label::new(Some("Repeat default"));
    label.add_css_class("okp-info-label");
    label.set_xalign(0.0);
    text.append(&label);
    let detail = gtk::Label::new(Some(
        "Choose how folders and playlists repeat when they reach the end.",
    ));
    detail.add_css_class("okp-update-status");
    detail.set_xalign(0.0);
    detail.set_width_chars(1);
    detail.set_max_width_chars(50);
    detail.set_wrap(true);
    text.append(&detail);
    row.append(&text);

    let current = state.borrow().playlist.repeat();
    let state_label = gtk::Label::new(Some(match current {
        RepeatMode::Off => "Off",
        RepeatMode::One => "One",
        RepeatMode::All => "All",
    }));
    state_label.add_css_class("okp-settings-state-pill");
    state_label.set_valign(gtk::Align::Center);
    row.append(&state_label);

    let button = gtk::Button::with_label(repeat_mode_label(current));
    button.add_css_class("okp-settings-button");
    button.set_valign(gtk::Align::Center);
    let repeat_state = Rc::clone(&state);
    let repeat_toast = Rc::clone(&status_toast);
    let repeat_state_label = state_label.clone();
    button.connect_clicked(move |button| {
        let mode = {
            let mut state = repeat_state.borrow_mut();
            let mode = state.playlist.repeat().cycle();
            state.playlist.set_repeat(mode);
            state.settings.set_repeat_mode(mode.settings_value());
            save_settings_or_toast(&mut state, &repeat_toast);
            mode
        };
        button.set_label(repeat_mode_label(mode));
        repeat_state_label.set_text(match mode {
            RepeatMode::Off => "Off",
            RepeatMode::One => "One",
            RepeatMode::All => "All",
        });
        repeat_toast.show(repeat_mode_label(mode));
    });
    row.append(&button);

    row
}

pub(crate) fn save_settings_or_toast(state: &mut PlayerState, status_toast: &StatusToast) -> bool {
    match state.settings.save() {
        Ok(()) => true,
        Err(error) => {
            eprintln!("Failed to save settings: {error}");
            status_toast.show("Could not save settings");
            false
        }
    }
}

pub(crate) fn settings_subtitle_snapshot(
    state: &Rc<RefCell<PlayerState>>,
) -> SettingsSubtitleSnapshot {
    let has_media = has_loaded_media(state);
    let tracks = read_tracks(state)
        .into_iter()
        .filter(|track| track.kind == TrackKind::Subtitle)
        .collect::<Vec<_>>();
    let secondary_id = read_secondary_subtitle_id(state);
    // Exclude the secondary track from the primary readout: mpv reports it as
    // selected too, so `find(selected)` could otherwise name the secondary as
    // the primary when no primary is set (shared core rule).
    let primary = tracks
        .iter()
        .find(|track| {
            okp_core::subtitle_tracks::is_primary_subtitle(track.id, track.selected, secondary_id)
        })
        .map(track_base_label)
        .unwrap_or_else(|| {
            if has_media {
                "Off".to_owned()
            } else {
                "No media loaded".to_owned()
            }
        });
    let secondary = secondary_id
        .and_then(|id| tracks.iter().find(|track| track.id == id))
        .map(track_base_label)
        .unwrap_or_else(|| {
            if has_media {
                "Off".to_owned()
            } else {
                "No media loaded".to_owned()
            }
        });
    let (delay_seconds, _) = read_subtitle_adjustments(state);

    SettingsSubtitleSnapshot {
        has_media,
        primary,
        secondary,
        delay_seconds,
    }
}

pub(crate) fn settings_subtitle_track_section(
    title: &str,
    secondary: bool,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) -> gtk::Box {
    let section = settings_section(title);
    if !has_loaded_media(&state) {
        section.append(&settings_empty_state("No media loaded"));
        return section;
    }

    let tracks = read_tracks(&state)
        .into_iter()
        .filter(|track| track.kind == TrackKind::Subtitle)
        .collect::<Vec<_>>();
    let selected_id = if secondary {
        read_secondary_subtitle_id(&state)
    } else {
        // The primary picker excludes the secondary track so its checkmark never
        // lands on the caption that belongs to the secondary slot.
        let secondary_id = read_secondary_subtitle_id(&state);
        tracks
            .iter()
            .find(|track| {
                okp_core::subtitle_tracks::is_primary_subtitle(
                    track.id,
                    track.selected,
                    secondary_id,
                )
            })
            .map(|track| track.id)
    };
    let buttons = Rc::new(RefCell::new(Vec::<gtk::Button>::new()));

    let off_button = settings_track_button("Off", selected_id.is_none());
    connect_settings_subtitle_track_button(
        &off_button,
        None,
        secondary,
        Rc::clone(&state),
        Rc::clone(&status_toast),
        &buttons,
    );
    buttons.borrow_mut().push(off_button.clone());
    section.append(&off_button);

    if tracks.is_empty() {
        section.append(&settings_empty_state("No subtitle tracks"));
    } else {
        for track in tracks {
            let button = settings_track_button(&track_label(&track), selected_id == Some(track.id));
            connect_settings_subtitle_track_button(
                &button,
                Some(track.id),
                secondary,
                Rc::clone(&state),
                Rc::clone(&status_toast),
                &buttons,
            );
            buttons.borrow_mut().push(button.clone());
            section.append(&button);
        }
    }

    section
}

pub(crate) fn settings_audio_track_section(
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) -> gtk::Box {
    let section = settings_section("Audio Tracks");
    if !has_loaded_media(&state) {
        section.append(&settings_empty_state("No media loaded"));
        return section;
    }

    let tracks = read_tracks(&state)
        .into_iter()
        .filter(|track| track.kind == TrackKind::Audio)
        .collect::<Vec<_>>();
    let selected_id = tracks
        .iter()
        .find(|track| track.selected)
        .map(|track| track.id);
    let buttons = Rc::new(RefCell::new(Vec::<gtk::Button>::new()));

    let off_button = settings_track_button("Off", selected_id.is_none());
    connect_settings_audio_track_button(
        &off_button,
        None,
        Rc::clone(&state),
        Rc::clone(&status_toast),
        &buttons,
    );
    buttons.borrow_mut().push(off_button.clone());
    section.append(&off_button);

    if tracks.is_empty() {
        section.append(&settings_empty_state("No audio tracks"));
    } else {
        for track in tracks {
            let button = settings_track_button(&track_label(&track), selected_id == Some(track.id));
            connect_settings_audio_track_button(
                &button,
                Some(track.id),
                Rc::clone(&state),
                Rc::clone(&status_toast),
                &buttons,
            );
            buttons.borrow_mut().push(button.clone());
            section.append(&button);
        }
    }

    section
}

pub(crate) fn connect_settings_subtitle_track_button(
    button: &gtk::Button,
    track_id: Option<i64>,
    secondary: bool,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
    buttons: &Rc<RefCell<Vec<gtk::Button>>>,
) {
    let selected_button = button.clone();
    let buttons = Rc::clone(buttons);
    button.connect_clicked(move |_| {
        let ok = with_mpv(&state, |mpv| {
            if secondary {
                mpv.select_secondary_subtitle(track_id)
            } else {
                mpv.select_subtitle(track_id)
            }
        });
        if ok {
            save_current_preferences(&state);
            mark_settings_track_selected(&buttons, &selected_button);
            status_toast.show("Subtitle track updated");
        }
    });
}

pub(crate) fn connect_settings_audio_track_button(
    button: &gtk::Button,
    track_id: Option<i64>,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
    buttons: &Rc<RefCell<Vec<gtk::Button>>>,
) {
    let selected_button = button.clone();
    let buttons = Rc::clone(buttons);
    button.connect_clicked(move |_| {
        if with_mpv(&state, |mpv| mpv.select_audio(track_id)) {
            save_current_preferences(&state);
            mark_settings_track_selected(&buttons, &selected_button);
            status_toast.show("Audio track updated");
        }
    });
}

pub(crate) fn connect_settings_audio_device_button(
    button: &gtk::Button,
    device_name: String,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
    buttons: &Rc<RefCell<Vec<gtk::Button>>>,
) {
    let selected_button = button.clone();
    let buttons = Rc::clone(buttons);
    button.connect_clicked(move |_| {
        if with_mpv(&state, |mpv| mpv.set_audio_device(&device_name)) {
            save_audio_device_setting(&state, &device_name, Some(status_toast.as_ref()));
            mark_settings_track_selected(&buttons, &selected_button);
            status_toast.show("Audio output updated");
        }
    });
}

pub(crate) fn mark_settings_track_selected(
    buttons: &Rc<RefCell<Vec<gtk::Button>>>,
    selected: &gtk::Button,
) {
    for button in buttons.borrow().iter() {
        button.remove_css_class("is-selected");
    }
    selected.add_css_class("is-selected");
}

pub(crate) fn settings_track_button(text: &str, selected: bool) -> gtk::Button {
    let button = gtk::Button::with_label(text);
    button.add_css_class("okp-settings-track-row");
    button.set_has_frame(false);
    if selected {
        button.add_css_class("is-selected");
    }
    button
}

pub(crate) fn settings_empty_state(text: &str) -> gtk::Box {
    let block = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    block.add_css_class("okp-empty-state");
    block.set_halign(gtk::Align::Fill);

    let inner = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    inner.set_halign(gtk::Align::Center);
    inner.set_hexpand(true);

    let icon = gtk::Image::from_icon_name("dialog-information-symbolic");
    icon.set_pixel_size(14);
    icon.add_css_class("okp-empty-state-icon");
    inner.append(&icon);

    let label = gtk::Label::new(Some(text));
    label.add_css_class("okp-empty-state-text");
    label.set_xalign(0.0);
    inner.append(&label);

    block.append(&inner);
    block
}

pub(crate) fn selected_track_summary(state: &Rc<RefCell<PlayerState>>, kind: TrackKind) -> String {
    if !has_loaded_media(state) {
        return "No media loaded".to_owned();
    }

    read_tracks(state)
        .into_iter()
        .find(|track| track.kind == kind && track.selected)
        .map(|track| track_label(&track))
        .unwrap_or_else(|| "Off".to_owned())
}

pub(crate) fn settings_volume_row(state: Rc<RefCell<PlayerState>>) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Vertical, 8);
    row.add_css_class("okp-settings-row");

    let header = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    let label = gtk::Label::new(Some("Volume"));
    label.add_css_class("okp-info-label");
    label.set_xalign(0.0);
    label.set_hexpand(true);

    let current_volume = state.borrow().settings.volume();
    let value = gtk::Label::new(Some(&format!("{current_volume:.0}%")));
    value.add_css_class("okp-info-value");
    value.set_xalign(1.0);
    header.append(&label);
    header.append(&value);
    row.append(&header);

    let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 130.0, 1.0);
    scale.set_draw_value(false);
    scale.set_value(current_volume);
    scale.add_css_class("okp-settings-scale");

    let value_label = value.clone();
    scale.connect_change_value(move |_, _, volume| {
        value_label.set_text(&format!("{volume:.0}%"));
        set_volume_from_ui(&state, volume);
        glib::Propagation::Proceed
    });
    row.append(&scale);

    row
}

pub(crate) fn settings_audio_normalization_row(
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) -> gtk::Box {
    let active = state.borrow().settings.audio_normalization_enabled();
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row.add_css_class("okp-settings-switch-row");

    let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
    text.set_hexpand(true);
    let label = gtk::Label::new(Some("Loudness normalization"));
    label.add_css_class("okp-info-label");
    label.set_xalign(0.0);
    text.append(&label);
    let detail = gtk::Label::new(Some(
        "Night mode: smooths quiet dialogue and loud effects using mpv dynaudnorm.",
    ));
    detail.add_css_class("okp-update-status");
    detail.set_xalign(0.0);
    detail.set_width_chars(1);
    detail.set_max_width_chars(50);
    detail.set_wrap(true);
    text.append(&detail);
    row.append(&text);

    let state_label = gtk::Label::new(Some(if active { "On" } else { "Off" }));
    state_label.add_css_class("okp-settings-state-pill");
    state_label.set_valign(gtk::Align::Center);
    row.append(&state_label);

    let toggle = about_toggle_button(active);
    let toggle_state = Rc::clone(&state);
    let toggle_toast = Rc::clone(&status_toast);
    let toggle_state_label = state_label.clone();
    toggle.connect_clicked(move |button| {
        let enabled = !button.has_css_class("is-active");
        set_about_toggle_active(button, enabled);

        let (save_result, live_result) = {
            let mut state = toggle_state.borrow_mut();
            state.settings.set_audio_normalization_enabled(enabled);
            let save_result = state.settings.save();
            let live_result = state
                .mpv
                .as_ref()
                .map(|mpv| mpv.set_audio_normalization(enabled));
            (save_result, live_result)
        };

        toggle_state_label.set_text(if enabled { "On" } else { "Off" });

        if let Err(error) = save_result {
            eprintln!("Failed to save audio normalization setting: {error}");
            toggle_toast.show("Could not save audio normalization");
        } else if let Some(Err(error)) = live_result {
            eprintln!("Failed to update audio normalization: {error}");
            toggle_toast.show("Could not update audio normalization");
        } else {
            toggle_toast.show(if enabled {
                "Loudness normalization on"
            } else {
                "Loudness normalization off"
            });
        }
    });
    row.append(&toggle);

    row
}

pub(crate) fn settings_surround_downmix_row(
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) -> gtk::Box {
    let active = state.borrow().settings.downmix_surround_to_stereo_enabled();
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row.add_css_class("okp-settings-switch-row");

    let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
    text.set_hexpand(true);
    let label = gtk::Label::new(Some("Downmix surround to stereo"));
    label.add_css_class("okp-info-label");
    label.set_xalign(0.0);
    text.append(&label);
    let detail = gtk::Label::new(Some(
        "Mixes 5.1 and 7.1 sources to two-channel output; stereo sources stay stereo.",
    ));
    detail.add_css_class("okp-update-status");
    detail.set_xalign(0.0);
    detail.set_width_chars(1);
    detail.set_max_width_chars(50);
    detail.set_wrap(true);
    text.append(&detail);
    row.append(&text);

    let state_label = gtk::Label::new(Some(if active { "Stereo" } else { "Auto" }));
    state_label.add_css_class("okp-settings-state-pill");
    state_label.set_valign(gtk::Align::Center);
    row.append(&state_label);

    let toggle = about_toggle_button(active);
    let toggle_state = Rc::clone(&state);
    let toggle_toast = Rc::clone(&status_toast);
    let toggle_state_label = state_label.clone();
    toggle.connect_clicked(move |button| {
        let enabled = !button.has_css_class("is-active");
        set_about_toggle_active(button, enabled);

        let (save_result, live_result) = {
            let mut state = toggle_state.borrow_mut();
            state
                .settings
                .set_downmix_surround_to_stereo_enabled(enabled);
            let save_result = state.settings.save();
            let live_result = state
                .mpv
                .as_ref()
                .map(|mpv| mpv.set_downmix_surround_to_stereo(enabled));
            (save_result, live_result)
        };

        toggle_state_label.set_text(if enabled { "Stereo" } else { "Auto" });
        if let Err(error) = save_result {
            eprintln!("Failed to save surround downmix setting: {error}");
            toggle_toast.show("Could not save surround downmix");
        } else if let Some(Err(error)) = live_result {
            eprintln!("Failed to update surround downmix: {error}");
            toggle_toast.show("Could not update surround downmix");
        } else {
            toggle_toast.show(if enabled {
                "Surround downmix: stereo"
            } else {
                "Surround downmix: automatic"
            });
        }
    });
    row.append(&toggle);

    row
}

pub(crate) fn settings_audio_device_section(
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) -> gtk::Box {
    let section = settings_section("Output Device");
    let devices = read_audio_devices(&state);
    if devices.is_empty() {
        section.append(&settings_empty_state("Audio engine not ready"));
        return section;
    }

    let buttons = Rc::new(RefCell::new(Vec::<gtk::Button>::new()));
    for device in devices {
        let button = settings_track_button(&device.label, device.selected);
        connect_settings_audio_device_button(
            &button,
            device.name,
            Rc::clone(&state),
            Rc::clone(&status_toast),
            &buttons,
        );
        buttons.borrow_mut().push(button.clone());
        section.append(&button);
    }

    section
}

pub(crate) fn settings_video_adjustment_row(
    adjustment: VideoAdjustment,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Vertical, 8);
    row.add_css_class("okp-settings-row");

    let header = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    let label = gtk::Label::new(Some(adjustment.label()));
    label.add_css_class("okp-info-label");
    label.set_xalign(0.0);
    label.set_hexpand(true);

    let current = adjustment.read(&state.borrow().settings);
    let value = gtk::Label::new(Some(&format_video_adjustment(current)));
    value.add_css_class("okp-info-value");
    value.set_xalign(1.0);

    let reset = gtk::Button::with_label("Reset");
    reset.add_css_class("okp-settings-button");

    header.append(&label);
    header.append(&value);
    header.append(&reset);
    row.append(&header);

    let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, -100.0, 100.0, 1.0);
    scale.set_draw_value(false);
    scale.set_value(current);
    scale.add_css_class("okp-settings-scale");

    let value_label = value.clone();
    let slider_state = Rc::clone(&state);
    let slider_toast = Rc::clone(&status_toast);
    scale.connect_change_value(move |_, _, raw_value| {
        let value = raw_value.round().clamp(-100.0, 100.0);
        value_label.set_text(&format_video_adjustment(value));
        set_video_adjustment_from_ui(&slider_state, adjustment, value, &slider_toast);
        glib::Propagation::Proceed
    });

    let reset_scale = scale.clone();
    let reset_state = Rc::clone(&state);
    let reset_toast = Rc::clone(&status_toast);
    let reset_value = value.clone();
    reset.connect_clicked(move |_| {
        reset_scale.set_value(0.0);
        reset_value.set_text(&format_video_adjustment(0.0));
        set_video_adjustment_from_ui(&reset_state, adjustment, 0.0, &reset_toast);
    });

    row.append(&scale);
    row
}

pub(crate) fn set_video_adjustment_from_ui(
    state: &Rc<RefCell<PlayerState>>,
    adjustment: VideoAdjustment,
    value: f64,
    status_toast: &StatusToast,
) {
    let (stored_value, save_ok) = {
        let mut state = state.borrow_mut();
        adjustment.write(&mut state.settings, value);
        let save_ok = if let Err(error) = state.settings.save() {
            eprintln!("Failed to save video adjustment: {error}");
            false
        } else {
            true
        };
        (adjustment.read(&state.settings), save_ok)
    };

    let live_result = {
        let state = state.borrow();
        state
            .mpv
            .as_ref()
            .map(|mpv| adjustment.apply(mpv, stored_value))
    };

    match live_result {
        Some(Err(error)) => {
            eprintln!("Failed to update video adjustment: {error}");
            status_toast.show("Could not update video adjustment");
        }
        _ if !save_ok => status_toast.show("Could not save video adjustment"),
        _ => {}
    }
}

pub(crate) fn format_video_adjustment(value: f64) -> String {
    if value > 0.0 {
        format!("+{value:.0}")
    } else {
        format!("{value:.0}")
    }
}

pub(crate) fn settings_hwdec_row(
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row.add_css_class("okp-settings-switch-row");

    let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
    text.set_hexpand(true);

    let label = gtk::Label::new(Some("Hardware decode"));
    label.add_css_class("okp-info-label");
    label.set_xalign(0.0);
    text.append(&label);

    let detail = gtk::Label::new(Some(
        "Use mpv auto-safe decoding when the driver stack supports it.",
    ));
    detail.add_css_class("okp-update-status");
    detail.set_xalign(0.0);
    detail.set_wrap(true);
    text.append(&detail);
    row.append(&text);

    let enabled = state.borrow().settings.hardware_decode_enabled();
    let state_label = gtk::Label::new(Some(if enabled { "Auto-safe" } else { "Off" }));
    state_label.add_css_class("okp-settings-state-pill");
    row.append(&state_label);

    let toggle = gtk::Switch::new();
    toggle.add_css_class("okp-settings-switch");
    toggle.set_active(enabled);
    let switch_state = Rc::clone(&state);
    let switch_toast = Rc::clone(&status_toast);
    let switch_label = state_label.clone();
    toggle.connect_state_set(move |_, enabled| {
        let (hwdec_option, save_ok) = {
            let mut state = switch_state.borrow_mut();
            state.settings.set_hardware_decode_enabled(enabled);
            let save_ok = if let Err(error) = state.settings.save() {
                eprintln!("Failed to save hardware decode setting: {error}");
                false
            } else {
                true
            };
            (state.settings.hardware_decode_mpv_option(), save_ok)
        };

        switch_label.set_text(if enabled { "Auto-safe" } else { "Off" });

        let live_result = {
            let state = switch_state.borrow();
            state.mpv.as_ref().map(|mpv| mpv.set_hwdec(hwdec_option))
        };

        match live_result {
            Some(Err(error)) => {
                eprintln!("Failed to update hardware decode: {error}");
                switch_toast.show("Could not update hardware decode");
            }
            _ if !save_ok => switch_toast.show("Could not save hardware decode setting"),
            _ => switch_toast.show(if enabled {
                "Hardware decode auto-safe"
            } else {
                "Hardware decode off"
            }),
        }

        glib::Propagation::Proceed
    });
    row.append(&toggle);

    row
}

pub(crate) fn settings_hdr_handling_row() -> gtk::Box {
    let handling = LINUX_HDR_HANDLING;
    eprintln!(
        "video capability: hdr={} controls={}",
        handling.key(),
        if handling.controls_available() {
            "available"
        } else {
            "unavailable"
        }
    );

    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row.add_css_class("okp-settings-switch-row");

    let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
    text.set_hexpand(true);

    let label = gtk::Label::new(Some("HDR handling"));
    label.add_css_class("okp-info-label");
    label.set_xalign(0.0);
    text.append(&label);

    let detail = gtk::Label::new(Some(handling.detail()));
    detail.add_css_class("okp-update-status");
    detail.set_xalign(0.0);
    detail.set_width_chars(1);
    detail.set_max_width_chars(50);
    detail.set_wrap(true);
    text.append(&detail);
    row.append(&text);

    let state_label = gtk::Label::new(Some(handling.settings_label()));
    state_label.add_css_class("okp-settings-state-pill");
    state_label.set_valign(gtk::Align::Center);
    row.append(&state_label);

    row
}

pub(crate) fn settings_shortcuts_section(
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) -> gtk::Box {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.append(&settings_shortcut_editor_section(state, status_toast));
    content
}

pub(crate) fn settings_shortcut_editor_section(
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) -> gtk::Box {
    let section = settings_section("Keyboard Shortcuts");
    let rows = Rc::new(RefCell::new(Vec::<Rc<ShortcutEditorRow>>::new()));
    let bindings = shortcut_editor_initial_bindings(&state.borrow().settings);

    let search = gtk::Entry::new();
    search.add_css_class("okp-shortcuts-search");
    search.set_placeholder_text(Some("Search"));
    section.append(&search);

    let status = gtk::Label::new(Some("Ready"));
    status.add_css_class("okp-update-status");
    status.set_xalign(0.0);
    status.set_width_chars(1);
    status.set_max_width_chars(58);
    status.set_wrap(true);

    let list = gtk::Box::new(gtk::Orientation::Vertical, 0);
    list.add_css_class("okp-shortcuts-list");

    for action in shortcuts::SHORTCUT_ACTIONS {
        let mut current_chords = shortcuts::chords_for_action(&bindings, *action).into_iter();
        let primary_chord = current_chords
            .next()
            .unwrap_or_else(|| shortcuts::default_chord_for_action(*action));
        let secondary_chord = current_chords.next();
        let row = shortcut_editor_row(
            *action,
            primary_chord,
            secondary_chord,
            Rc::clone(&rows),
            Rc::clone(&state),
            Rc::clone(&status_toast),
            status.clone(),
        );
        list.append(&row.container);
        rows.borrow_mut().push(row);
    }
    section.append(&list);

    section.append(&status);

    let search_rows = Rc::clone(&rows);
    search.connect_changed(move |entry| {
        let query = entry.text().trim().to_ascii_lowercase();
        for row in search_rows.borrow().iter() {
            let visible = query.is_empty()
                || row.action.label().to_ascii_lowercase().contains(&query)
                || row.action.id().contains(&query)
                || row
                    .primary_chord
                    .borrow()
                    .label()
                    .to_ascii_lowercase()
                    .contains(&query)
                || row
                    .secondary_chord
                    .borrow()
                    .as_ref()
                    .map(ShortcutChord::label)
                    .is_some_and(|label| label.to_ascii_lowercase().contains(&query));
            row.container.set_visible(visible);
        }
    });

    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    actions.add_css_class("okp-settings-action-row");
    actions.set_halign(gtk::Align::End);

    let reset = gtk::Button::with_label("Reset All");
    reset.add_css_class("okp-settings-button");
    let reset_rows = Rc::clone(&rows);
    let reset_state = state;
    let reset_toast = status_toast;
    let reset_status = status;
    reset.connect_clicked(move |_| {
        shortcut_editor_clear_capture(&reset_rows.borrow());
        shortcut_editor_clear_conflicts(&reset_rows.borrow());
        for row in reset_rows.borrow().iter() {
            *row.primary_chord.borrow_mut() = row.default_chord.clone();
            *row.secondary_chord.borrow_mut() = None;
            shortcut_editor_refresh_row(row);
        }
        save_shortcut_editor_rows(
            &reset_rows,
            &reset_state,
            &reset_status,
            &reset_toast,
            "All shortcuts reset",
        );
    });
    actions.append(&reset);

    section.append(&actions);
    section
}

pub(crate) fn shortcut_editor_row(
    action: ShortcutAction,
    primary_chord: ShortcutChord,
    secondary_chord: Option<ShortcutChord>,
    rows: Rc<RefCell<Vec<Rc<ShortcutEditorRow>>>>,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
    status: gtk::Label,
) -> Rc<ShortcutEditorRow> {
    let default_chord = shortcuts::default_chord_for_action(action);
    let container = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    container.add_css_class("okp-shortcut-row");

    let text = gtk::Box::new(gtk::Orientation::Vertical, 1);
    text.set_hexpand(true);

    let title = gtk::Label::new(Some(action.label()));
    title.add_css_class("okp-shortcut-action-title");
    title.set_xalign(0.0);
    text.append(&title);

    let subtitle = gtk::Label::new(Some(action.id()));
    subtitle.add_css_class("okp-shortcut-action-id");
    subtitle.set_xalign(0.0);
    text.append(&subtitle);
    container.append(&text);

    let badge = gtk::Label::new(Some("CUSTOM"));
    badge.add_css_class("okp-shortcut-badge");
    badge.set_valign(gtk::Align::Center);
    container.append(&badge);

    let primary_chip = gtk::Button::new();
    primary_chip.add_css_class("okp-shortcut-chip");
    primary_chip.set_has_frame(false);
    primary_chip.set_focus_on_click(true);
    primary_chip.set_tooltip_text(Some("Change primary shortcut"));
    let primary_chip_label = gtk::Label::new(None);
    primary_chip_label.add_css_class("okp-shortcut-chip-label");
    primary_chip.set_child(Some(&primary_chip_label));
    container.append(&primary_chip);

    let secondary_chip = gtk::Button::new();
    secondary_chip.add_css_class("okp-shortcut-chip");
    secondary_chip.add_css_class("is-secondary");
    secondary_chip.set_has_frame(false);
    secondary_chip.set_focus_on_click(true);
    secondary_chip.set_tooltip_text(Some("Add secondary shortcut"));
    let secondary_chip_label = gtk::Label::new(None);
    secondary_chip_label.add_css_class("okp-shortcut-chip-label");
    secondary_chip.set_child(Some(&secondary_chip_label));
    container.append(&secondary_chip);

    let reset = gtk::Button::with_label("Reset");
    reset.add_css_class("okp-shortcut-reset");
    reset.set_has_frame(false);
    reset.set_valign(gtk::Align::Center);
    container.append(&reset);

    let row = Rc::new(ShortcutEditorRow {
        action,
        default_chord,
        primary_chord: RefCell::new(primary_chord),
        secondary_chord: RefCell::new(secondary_chord),
        container,
        primary_chip,
        primary_chip_label,
        secondary_chip,
        secondary_chip_label,
        badge,
        reset,
    });
    shortcut_editor_refresh_row(&row);

    connect_shortcut_editor_chip(
        &row,
        ShortcutSlot::Primary,
        Rc::clone(&rows),
        Rc::clone(&state),
        Rc::clone(&status_toast),
        status.clone(),
    );
    connect_shortcut_editor_chip(
        &row,
        ShortcutSlot::Secondary,
        Rc::clone(&rows),
        Rc::clone(&state),
        Rc::clone(&status_toast),
        status.clone(),
    );

    let reset_row = Rc::clone(&row);
    let reset_rows = rows;
    let reset_state = state;
    let reset_toast = status_toast;
    let reset_status = status;
    row.reset.connect_clicked(move |_| {
        shortcut_editor_clear_capture(&reset_rows.borrow());
        shortcut_editor_clear_conflicts(&reset_rows.borrow());
        *reset_row.primary_chord.borrow_mut() = reset_row.default_chord.clone();
        *reset_row.secondary_chord.borrow_mut() = None;
        shortcut_editor_refresh_row(&reset_row);
        save_shortcut_editor_rows(
            &reset_rows,
            &reset_state,
            &reset_status,
            &reset_toast,
            "Shortcut reset",
        );
    });

    row
}

pub(crate) fn connect_shortcut_editor_chip(
    row: &Rc<ShortcutEditorRow>,
    slot: ShortcutSlot,
    rows: Rc<RefCell<Vec<Rc<ShortcutEditorRow>>>>,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
    status: gtk::Label,
) {
    let chip = shortcut_editor_chip_for(row, slot);
    let chip_row = Rc::clone(row);
    let chip_rows = Rc::clone(&rows);
    let chip_status = status.clone();
    chip.connect_clicked(move |button| {
        shortcut_editor_clear_capture(&chip_rows.borrow());
        shortcut_editor_clear_conflicts(&chip_rows.borrow());
        button.add_css_class("is-capturing");
        shortcut_editor_chip_label_for(&chip_row, slot).set_text("Press keys");
        chip_status.set_text(&format!("Recording {}", chip_row.action.label()));
        button.grab_focus();
    });

    let key_row = Rc::clone(row);
    let key_rows = rows;
    let key_state = state;
    let key_toast = status_toast;
    let key_status = status;
    let key_chip = shortcut_editor_chip_for(row, slot);
    let key_controller = gtk::EventControllerKey::new();
    key_controller.connect_key_pressed(move |_, key, _, modifiers| {
        if !key_chip.has_css_class("is-capturing") {
            return glib::Propagation::Proceed;
        }

        let chord = match shortcut_chord_from_event(key, modifiers) {
            Ok(chord) => chord,
            Err(message) => {
                key_status.set_text(message);
                return glib::Propagation::Stop;
            }
        };

        if let Some(conflict) =
            shortcut_editor_conflict(&key_rows.borrow(), key_row.action, slot, &chord)
        {
            shortcut_editor_mark_conflict(&key_rows.borrow(), key_row.action, conflict);
            key_status.set_text(&format!(
                "{} already uses {}",
                conflict.label(),
                chord.label()
            ));
            key_toast.show("Shortcut conflict");
            return glib::Propagation::Stop;
        }

        shortcut_editor_clear_conflicts(&key_rows.borrow());
        key_chip.remove_css_class("is-capturing");
        shortcut_editor_set_chord(&key_row, slot, chord);
        shortcut_editor_refresh_row(&key_row);
        save_shortcut_editor_rows(
            &key_rows,
            &key_state,
            &key_status,
            &key_toast,
            "Shortcut saved",
        );
        glib::Propagation::Stop
    });
    shortcut_editor_chip_for(row, slot).add_controller(key_controller);
}

pub(crate) fn shortcut_editor_initial_bindings(
    settings: &settings::SettingsStore,
) -> Vec<ShortcutBinding> {
    resolved_shortcut_bindings(settings).unwrap_or_else(|error| {
        eprintln!(
            "Ignoring custom keybindings at line {} while building Settings UI: {}",
            error.line, error.message
        );
        shortcuts::default_bindings()
    })
}

pub(crate) fn shortcut_editor_refresh_row(row: &ShortcutEditorRow) {
    let secondary = row.secondary_chord.borrow().clone();
    let is_custom = *row.primary_chord.borrow() != row.default_chord || secondary.is_some();
    row.primary_chip_label
        .set_text(&row.primary_chord.borrow().label());
    if let Some(chord) = secondary {
        row.secondary_chip_label.set_text(&chord.label());
        row.secondary_chip.remove_css_class("is-empty");
        row.secondary_chip
            .set_tooltip_text(Some("Change secondary shortcut"));
    } else {
        row.secondary_chip_label.set_text("Add");
        row.secondary_chip.add_css_class("is-empty");
        row.secondary_chip
            .set_tooltip_text(Some("Add secondary shortcut"));
    }
    row.badge.set_visible(is_custom);
    row.reset.set_sensitive(is_custom);
}

pub(crate) fn shortcut_editor_clear_capture(rows: &[Rc<ShortcutEditorRow>]) {
    for row in rows {
        let was_capturing = row.primary_chip.has_css_class("is-capturing")
            || row.secondary_chip.has_css_class("is-capturing");
        row.primary_chip.remove_css_class("is-capturing");
        row.secondary_chip.remove_css_class("is-capturing");
        if was_capturing {
            shortcut_editor_refresh_row(row);
        }
    }
}

pub(crate) fn shortcut_editor_clear_conflicts(rows: &[Rc<ShortcutEditorRow>]) {
    for row in rows {
        row.container.remove_css_class("is-conflict");
        row.primary_chip.remove_css_class("is-conflict");
        row.secondary_chip.remove_css_class("is-conflict");
    }
}

pub(crate) fn shortcut_editor_mark_conflict(
    rows: &[Rc<ShortcutEditorRow>],
    left: ShortcutAction,
    right: ShortcutAction,
) {
    shortcut_editor_clear_conflicts(rows);
    for row in rows {
        if row.action == left || row.action == right {
            row.container.add_css_class("is-conflict");
            row.primary_chip.add_css_class("is-conflict");
            row.secondary_chip.add_css_class("is-conflict");
        }
    }
}

pub(crate) fn shortcut_editor_action_chords(
    rows: &[Rc<ShortcutEditorRow>],
) -> Vec<shortcuts::ActionChords> {
    rows.iter()
        .map(|row| shortcuts::ActionChords {
            action: row.action,
            primary: row.primary_chord.borrow().clone(),
            secondary: row.secondary_chord.borrow().clone(),
        })
        .collect()
}

pub(crate) fn shortcut_editor_conflict(
    rows: &[Rc<ShortcutEditorRow>],
    action: ShortcutAction,
    slot: ShortcutSlot,
    chord: &ShortcutChord,
) -> Option<ShortcutAction> {
    shortcuts::slot_conflict(&shortcut_editor_action_chords(rows), action, slot, chord)
}

pub(crate) fn save_shortcut_editor_rows(
    rows: &Rc<RefCell<Vec<Rc<ShortcutEditorRow>>>>,
    state: &Rc<RefCell<PlayerState>>,
    status: &gtk::Label,
    status_toast: &StatusToast,
    success_message: &str,
) {
    let bindings =
        shortcuts::bindings_from_action_chords(&shortcut_editor_action_chords(&rows.borrow()));
    if let Err(error) = shortcuts::validate_conflicts(&bindings) {
        status.set_text(&error.message);
        status_toast.show("Shortcut conflict");
        return;
    }

    let text = shortcuts::config_text_from_bindings(&bindings);
    let save_result = {
        let mut state = state.borrow_mut();
        state.settings.set_raw_keybindings_config(&text);
        state.settings.save()
    };
    if let Err(error) = save_result {
        eprintln!("Failed to save keybinding remap setting: {error}");
        status.set_text("Could not save keybindings.");
        status_toast.show("Could not save keybindings");
        return;
    }

    status.set_text(success_message);
    status_toast.show(success_message);
}

pub(crate) fn shortcut_editor_chip_for(row: &ShortcutEditorRow, slot: ShortcutSlot) -> gtk::Button {
    match slot {
        ShortcutSlot::Primary => row.primary_chip.clone(),
        ShortcutSlot::Secondary => row.secondary_chip.clone(),
    }
}

pub(crate) fn shortcut_editor_chip_label_for(
    row: &ShortcutEditorRow,
    slot: ShortcutSlot,
) -> gtk::Label {
    match slot {
        ShortcutSlot::Primary => row.primary_chip_label.clone(),
        ShortcutSlot::Secondary => row.secondary_chip_label.clone(),
    }
}

pub(crate) fn shortcut_editor_set_chord(
    row: &ShortcutEditorRow,
    slot: ShortcutSlot,
    chord: ShortcutChord,
) {
    match slot {
        ShortcutSlot::Primary => *row.primary_chord.borrow_mut() = chord,
        ShortcutSlot::Secondary => *row.secondary_chord.borrow_mut() = Some(chord),
    }
}

pub(crate) fn settings_private_session_row(
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row.add_css_class("okp-settings-row");

    let label = gtk::Label::new(Some("Private Session"));
    label.add_css_class("okp-info-label");
    label.set_xalign(0.0);
    label.set_hexpand(true);
    row.append(&label);

    let private_session = state.borrow().private_session;
    let button = gtk::Button::with_label(if private_session { "On" } else { "Off" });
    button.add_css_class("okp-settings-button");
    button.connect_clicked(move |button| {
        toggle_private_session(&state, &status_toast);
        let private_session = state.borrow().private_session;
        button.set_label(if private_session { "On" } else { "Off" });
    });
    row.append(&button);

    row
}

pub(crate) fn settings_clear_history_row(
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row.add_css_class("okp-settings-row");

    let label = gtk::Label::new(Some("History"));
    label.add_css_class("okp-info-label");
    label.set_xalign(0.0);
    label.set_hexpand(true);
    row.append(&label);

    let button = gtk::Button::with_label("Clear...");
    button.add_css_class("okp-settings-button");
    let parent = parent.clone();
    button.connect_clicked(move |_| {
        open_clear_history_dialog(&parent, Rc::clone(&state), Rc::clone(&status_toast));
    });
    row.append(&button);

    row
}

pub(crate) fn settings_value_row(label: &str, value: &str) -> gtk::Box {
    settings_value_row_with_label(label, value).0
}

/// A value row that carries a trailing group of compact stepper buttons, so the
/// control that changes a value sits next to the value it reports. Returns the
/// row, its value label (for live refresh), and the created buttons paired with
/// their adjustment so callers can wire behaviour without re-reading the group.
pub(crate) fn settings_stepper_row(
    label: &str,
    value: &str,
    buttons: &[(&str, SubtitleAdjustment)],
) -> (gtk::Box, gtk::Label, Vec<(gtk::Button, SubtitleAdjustment)>) {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row.add_css_class("okp-settings-row");

    let label_widget = gtk::Label::new(Some(label));
    label_widget.add_css_class("okp-info-label");
    label_widget.set_xalign(0.0);
    label_widget.set_width_chars(14);
    row.append(&label_widget);

    let value_widget = gtk::Label::new(Some(value));
    value_widget.add_css_class("okp-info-value");
    value_widget.set_xalign(0.0);
    value_widget.set_hexpand(true);
    value_widget.set_width_chars(1);
    value_widget.set_ellipsize(pango::EllipsizeMode::End);
    row.append(&value_widget);

    let group = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    group.add_css_class("okp-settings-stepper-group");
    group.set_halign(gtk::Align::End);
    group.set_valign(gtk::Align::Center);

    let mut created = Vec::with_capacity(buttons.len());
    for (text, adjustment) in buttons {
        let button = gtk::Button::with_label(text);
        button.add_css_class("okp-settings-button");
        button.add_css_class("okp-settings-stepper-button");
        group.append(&button);
        created.push((button, *adjustment));
    }
    row.append(&group);

    (row, value_widget, created)
}

pub(crate) fn settings_value_row_with_label(label: &str, value: &str) -> (gtk::Box, gtk::Label) {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row.add_css_class("okp-settings-row");

    let label = gtk::Label::new(Some(label));
    label.add_css_class("okp-info-label");
    label.set_xalign(0.0);
    label.set_width_chars(14);
    row.append(&label);

    let value = gtk::Label::new(Some(value));
    value.add_css_class("okp-info-value");
    value.set_xalign(0.0);
    value.set_hexpand(true);
    value.set_width_chars(1);
    value.set_max_width_chars(44);
    value.set_ellipsize(pango::EllipsizeMode::Middle);
    value.set_selectable(true);
    row.append(&value);

    (row, value)
}
