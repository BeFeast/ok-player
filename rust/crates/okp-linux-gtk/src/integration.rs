use super::*;

pub(crate) fn settings_integration_section(
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) -> gtk::Box {
    let snapshot = LinuxIntegrationSnapshot::capture();
    let section = settings_section("Integration");

    let status = gtk::Label::new(Some("Ready"));
    status.add_css_class("okp-update-status");
    status.set_xalign(0.0);
    status.set_width_chars(1);
    status.set_max_width_chars(58);
    status.set_wrap(true);

    let desktop_detail = snapshot
        .desktop_entry_path
        .as_ref()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|| format!("{LINUX_DESKTOP_ID} was not found in XDG application dirs"));
    section.append(&integration_status_row(
        "Desktop entry",
        if snapshot.desktop_entry_path.is_some() {
            "Installed"
        } else {
            "Missing"
        },
        &desktop_detail,
        if snapshot.desktop_entry_path.is_some() {
            IntegrationStatus::Good
        } else {
            IntegrationStatus::Bad
        },
    ));

    let registered = snapshot.registered_key_mimes;
    section.append(&integration_status_row(
        "Media types",
        &format!(
            "{registered}/{} key types",
            LINUX_KEY_MEDIA_MIME_TYPES.len()
        ),
        "Key audio/video MIME types advertised through the desktop entry.",
        if registered == LINUX_KEY_MEDIA_MIME_TYPES.len() {
            IntegrationStatus::Good
        } else if registered > 0 {
            IntegrationStatus::Warning
        } else {
            IntegrationStatus::Bad
        },
    ));

    let (defaults_value, defaults_detail, defaults_status) = match snapshot.default_key_mimes {
        Some(count) => {
            let remaining = LINUX_KEY_MEDIA_MIME_TYPES.len().saturating_sub(count);
            (
                format!("{count}/{} key types", LINUX_KEY_MEDIA_MIME_TYPES.len()),
                if remaining == 0 {
                    "OK Player is the default handler for the checked key media types.".to_owned()
                } else {
                    format!("{remaining} checked key media types still point elsewhere.")
                },
                if remaining == 0 {
                    IntegrationStatus::Good
                } else if count > 0 {
                    IntegrationStatus::Warning
                } else {
                    IntegrationStatus::Bad
                },
            )
        }
        None => (
            "Unavailable".to_owned(),
            "xdg-mime is not available, so default handlers cannot be checked.".to_owned(),
            IntegrationStatus::Warning,
        ),
    };
    let (defaults_row, defaults_value_label) = integration_status_row_with_value(
        "Default app",
        &defaults_value,
        &defaults_detail,
        defaults_status,
    );
    section.append(&defaults_row);

    let scheme_row = UriSchemeRow::new(snapshot.uri_scheme);
    section.append(&scheme_row.row);
    section.append(&uri_links_toggle_row(
        snapshot.uri_scheme.handling,
        Rc::clone(&state),
        Rc::clone(&status_toast),
        scheme_row.clone(),
        status.clone(),
    ));

    section.append(&integration_status_row(
        "System tools",
        linux_integration_tools_value(&snapshot),
        linux_integration_tools_detail(&snapshot),
        if snapshot.xdg_mime_available && snapshot.update_desktop_database_available {
            IntegrationStatus::Good
        } else {
            IntegrationStatus::Warning
        },
    ));

    section.append(&status);

    let actions = gtk::Box::new(gtk::Orientation::Vertical, 8);
    actions.add_css_class("okp-settings-action-row");
    actions.set_halign(gtk::Align::End);

    let primary = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    primary.set_halign(gtk::Align::End);

    let make_default = gtk::Button::with_label("Make Default");
    make_default.add_css_class("okp-settings-button");
    make_default
        .set_sensitive(snapshot.xdg_mime_available && snapshot.desktop_entry_path.is_some());
    let make_default_status = status.clone();
    let make_default_value = defaults_value_label.clone();
    let make_default_toast = Rc::clone(&status_toast);
    let make_default_state = Rc::clone(&state);
    let make_default_scheme_row = scheme_row.clone();
    make_default.connect_clicked(move |button| {
        button.set_sensitive(false);
        let uri_scheme_handling = user_uri_scheme_handling_enabled();
        match set_linux_default_app(uri_scheme_handling) {
            Ok(count) => {
                set_integration_state_pill(
                    &make_default_value,
                    &format!("{count}/{} key types", LINUX_KEY_MEDIA_MIME_TYPES.len()),
                    if count == LINUX_KEY_MEDIA_MIME_TYPES.len() {
                        IntegrationStatus::Good
                    } else {
                        IntegrationStatus::Warning
                    },
                );
                if uri_scheme_handling {
                    make_default_status.set_text(&format!(
                        "OK Player set as default for {count} key media types and {LINUX_RESERVED_URI_SCHEME} links."
                    ));
                } else {
                    // A default for a removed association would contradict the toggle, so the
                    // choice waits until links are turned back on.
                    let mut state = make_default_state.borrow_mut();
                    state.settings.set_restore_uri_scheme_default(true);
                    save_settings_or_toast(&mut state, &make_default_toast);
                    make_default_status.set_text(&format!(
                        "OK Player set as default for {count} key media types. {LINUX_RESERVED_URI_SCHEME} links are off; OK Player becomes their default when you turn them on."
                    ));
                }
                make_default_toast.show("Default media app updated");
            }
            Err(error) => {
                make_default_status.set_text(&format!("Could not update defaults: {error}"));
                make_default_toast.show("Could not update defaults");
            }
        }
        make_default_scheme_row.show(UriSchemeState::current());
        button.set_sensitive(true);
    });
    primary.append(&make_default);

    let default_apps = gtk::Button::with_label("Default Apps");
    default_apps.add_css_class("okp-settings-button");
    let default_apps_status = status.clone();
    let default_apps_toast = Rc::clone(&status_toast);
    default_apps.connect_clicked(move |_| match open_linux_default_apps_settings() {
        Ok(()) => {
            default_apps_status.set_text("Opened system Default Apps settings.");
            default_apps_toast.show("Default Apps opened");
        }
        Err(error) => {
            default_apps_status.set_text(&format!("Could not open Default Apps: {error}"));
            default_apps_toast.show("Could not open Default Apps");
        }
    });
    primary.append(&default_apps);
    actions.append(&primary);

    let secondary = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    secondary.set_halign(gtk::Align::End);

    let refresh = gtk::Button::with_label("Refresh Database");
    refresh.add_css_class("okp-settings-button");
    refresh.set_sensitive(snapshot.update_desktop_database_available);
    let refresh_status = status.clone();
    let refresh_toast = Rc::clone(&status_toast);
    refresh.connect_clicked(move |_| match refresh_linux_desktop_database() {
        Ok(detail) => {
            refresh_status.set_text(&detail);
            refresh_toast.show("Desktop database refreshed");
        }
        Err(error) => {
            refresh_status.set_text(&format!("Desktop database refresh failed: {error}"));
            refresh_toast.show("Desktop database refresh failed");
        }
    });
    secondary.append(&refresh);

    let copy = gtk::Button::with_label("Copy Diagnostics");
    copy.add_css_class("okp-settings-button");
    let copy_status = status.clone();
    let copy_toast = Rc::clone(&status_toast);
    copy.connect_clicked(move |_| {
        if let Some(display) = gdk::Display::default() {
            let snapshot = LinuxIntegrationSnapshot::capture();
            display
                .clipboard()
                .set_text(&linux_integration_diagnostics(&snapshot));
            copy_status.set_text("Integration diagnostics copied.");
            copy_toast.show("Diagnostics copied");
        }
    });
    secondary.append(&copy);
    actions.append(&secondary);

    section.append(&actions);
    section
}

pub(crate) fn integration_status_row(
    label: &str,
    value: &str,
    detail: &str,
    status: IntegrationStatus,
) -> gtk::Box {
    integration_status_row_with_value(label, value, detail, status).0
}

pub(crate) fn integration_status_row_with_value(
    label: &str,
    value: &str,
    detail: &str,
    status: IntegrationStatus,
) -> (gtk::Box, gtk::Label) {
    let (row, value, _detail) = integration_status_row_parts(label, value, detail, status);
    (row, value)
}

/// A status row plus its value pill and detail label, for rows whose text changes in place.
pub(crate) fn integration_status_row_parts(
    label: &str,
    value: &str,
    detail: &str,
    status: IntegrationStatus,
) -> (gtk::Box, gtk::Label, gtk::Label) {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row.add_css_class("okp-settings-row");

    let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
    text.set_hexpand(true);
    let label = gtk::Label::new(Some(label));
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

    let value = gtk::Label::new(Some(value));
    set_integration_state_pill(&value, value.text().as_ref(), status);
    value.set_valign(gtk::Align::Center);
    row.append(&value);

    (row, value, detail)
}

pub(crate) fn set_integration_state_pill(
    label: &gtk::Label,
    value: &str,
    status: IntegrationStatus,
) {
    label.set_text(value);
    label.add_css_class("okp-integration-state-pill");
    for css_class in ["is-good", "is-warning", "is-bad"] {
        label.remove_css_class(css_class);
    }
    label.add_css_class(status.css_class());
}

pub(crate) fn linux_integration_tools_value(snapshot: &LinuxIntegrationSnapshot) -> &'static str {
    match (
        snapshot.xdg_mime_available,
        snapshot.update_desktop_database_available,
    ) {
        (true, true) => "Available",
        (true, false) | (false, true) => "Partial",
        (false, false) => "Missing",
    }
}

pub(crate) fn linux_integration_tools_detail(snapshot: &LinuxIntegrationSnapshot) -> &'static str {
    match (
        snapshot.xdg_mime_available,
        snapshot.update_desktop_database_available,
    ) {
        (true, true) => "xdg-mime and update-desktop-database are available.",
        (true, false) => "xdg-mime is available; update-desktop-database is missing.",
        (false, true) => "update-desktop-database is available; xdg-mime is missing.",
        (false, false) => "xdg-mime and update-desktop-database are missing.",
    }
}

pub(crate) fn linux_desktop_entry_path() -> Option<PathBuf> {
    linux_desktop_entry_paths()
        .into_iter()
        .find(|path| path.is_file())
}

pub(crate) fn linux_desktop_entry_paths() -> Vec<PathBuf> {
    linux_application_dirs()
        .into_iter()
        .map(|dir| dir.join(LINUX_DESKTOP_ID))
        .collect()
}

pub(crate) fn linux_application_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(data_home) = env::var_os("XDG_DATA_HOME").filter(|value| !value.is_empty()) {
        dirs.push(PathBuf::from(data_home));
    } else if let Some(home) = env::var_os("HOME").filter(|value| !value.is_empty()) {
        dirs.push(PathBuf::from(home).join(".local/share"));
    }

    if let Some(data_dirs) = env::var_os("XDG_DATA_DIRS").filter(|value| !value.is_empty()) {
        dirs.extend(env::split_paths(&data_dirs));
    } else {
        dirs.push(PathBuf::from("/usr/local/share"));
        dirs.push(PathBuf::from("/usr/share"));
    }

    let mut application_dirs = Vec::new();
    for dir in dirs {
        let applications = dir.join("applications");
        if !application_dirs
            .iter()
            .any(|existing: &PathBuf| existing == &applications)
        {
            application_dirs.push(applications);
        }
    }
    application_dirs
}

pub(crate) fn parse_desktop_mime_types(contents: &str) -> Vec<String> {
    contents
        .lines()
        .find_map(|line| line.trim().strip_prefix("MimeType="))
        .map(|types| {
            types
                .split(';')
                .map(str::trim)
                .filter(|mime| !mime.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn count_registered_key_media_mimes(desktop_entry: &str) -> usize {
    let registered = parse_desktop_mime_types(desktop_entry);
    LINUX_KEY_MEDIA_MIME_TYPES
        .iter()
        .filter(|mime| registered.iter().any(|registered| registered == *mime))
        .count()
}

pub(crate) fn desktop_registers_uri_scheme(desktop_entry: &str) -> bool {
    parse_desktop_mime_types(desktop_entry)
        .iter()
        .any(|mime| mime == LINUX_URI_SCHEME_MIME)
}

pub(crate) fn uri_scheme_default_is_ok_player() -> bool {
    query_default_app_for_mime(LINUX_URI_SCHEME_MIME)
        .as_deref()
        .is_some_and(default_app_matches_ok_player)
}

/// The `ok-player://` contract version this shell executes (epic #845).
pub(crate) const URI_SCHEME_CONTRACT: &str = "v1";

/// Where `ok-player://` links stand on this desktop, as Settings and Copy Diagnostics report it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct UriSchemeState {
    /// An installed desktop entry lists `x-scheme-handler/ok-player` in `MimeType=`.
    pub(crate) advertised: bool,
    /// The Settings toggle: the user's `mimeapps.list` does not remove the association.
    pub(crate) handling: bool,
    /// `xdg-mime query default` names OK Player; `None` when xdg-mime is unavailable.
    pub(crate) default: Option<bool>,
}

impl UriSchemeState {
    pub(crate) fn capture(desktop_entry: Option<&str>, xdg_mime_available: bool) -> Self {
        Self {
            advertised: desktop_entry.is_some_and(desktop_registers_uri_scheme),
            handling: user_uri_scheme_handling_enabled(),
            default: xdg_mime_available.then(uri_scheme_default_is_ok_player),
        }
    }

    /// A fresh reading, for refreshing the Settings row after a change.
    pub(crate) fn current() -> Self {
        let desktop_entry =
            linux_desktop_entry_path().and_then(|path| fs::read_to_string(path).ok());
        Self::capture(
            desktop_entry.as_deref(),
            find_executable("xdg-mime").is_some(),
        )
    }

    /// Whether the desktop routes links to OK Player at all. Turning handling off is what
    /// unregisters it, so an advertised scheme with handling off is not registered.
    pub(crate) fn registered(self) -> bool {
        self.advertised && self.handling
    }

    /// The Copy Diagnostics value:
    /// `v1, handling on|off; registered (default)|registered (not default)|missing`.
    /// Without xdg-mime the default handler cannot be checked, which reads
    /// `registered (default unknown)`.
    pub(crate) fn diagnostics(self) -> String {
        let registration = if !self.registered() {
            "missing"
        } else {
            match self.default {
                Some(true) => "registered (default)",
                Some(false) => "registered (not default)",
                None => "registered (default unknown)",
            }
        };
        format!(
            "{URI_SCHEME_CONTRACT}, handling {}; {registration}",
            if self.handling { "on" } else { "off" }
        )
    }

    /// The Settings row's pill value, detail line and status.
    pub(crate) fn row_content(self) -> (&'static str, String, IntegrationStatus) {
        if !self.advertised {
            return (
                "Missing",
                format!(
                    "{LINUX_RESERVED_URI_SCHEME} is not advertised by an installed desktop entry."
                ),
                IntegrationStatus::Bad,
            );
        }
        if !self.handling {
            return (
                "Off",
                format!(
                    "{LINUX_RESERVED_URI_SCHEME} links are turned off; browsers no longer offer OK Player for them."
                ),
                IntegrationStatus::Warning,
            );
        }
        match self.default {
            Some(true) => (
                "Default",
                format!("OK Player opens {LINUX_RESERVED_URI_SCHEME} links."),
                IntegrationStatus::Good,
            ),
            Some(false) => (
                "Registered",
                format!(
                    "Another application is the default for {LINUX_RESERVED_URI_SCHEME} links; Make Default claims them."
                ),
                IntegrationStatus::Warning,
            ),
            None => (
                "Registered",
                format!(
                    "{LINUX_RESERVED_URI_SCHEME} is advertised; xdg-mime is unavailable to check the default handler."
                ),
                IntegrationStatus::Good,
            ),
        }
    }
}

/// The Settings → Integration "URI scheme" row, kept so a change can update it in place.
#[derive(Clone)]
pub(crate) struct UriSchemeRow {
    row: gtk::Box,
    value: gtk::Label,
    detail: gtk::Label,
}

impl UriSchemeRow {
    fn new(state: UriSchemeState) -> Self {
        let (value, detail, status) = state.row_content();
        let (row, value, detail) =
            integration_status_row_parts("URI scheme", value, &detail, status);
        Self { row, value, detail }
    }

    fn show(&self, state: UriSchemeState) {
        let (value, detail, status) = state.row_content();
        set_integration_state_pill(&self.value, value, status);
        self.detail.set_text(&detail);
    }
}

/// The "Open ok-player:// links" switch. Off writes a `[Removed Associations]` entry to the
/// user's `mimeapps.list`, so the desktop stops offering OK Player for the scheme; on removes it
/// again. It is not a player-side refusal: with handling off a link never reaches the player.
fn uri_links_toggle_row(
    active: bool,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
    scheme_row: UriSchemeRow,
    status: gtk::Label,
) -> gtk::Box {
    let flatpak = flatpak_install_detected(
        env::var_os("FLATPAK_ID").as_deref(),
        Path::new("/.flatpak-info").is_file(),
    );
    let title = format!("Open {LINUX_RESERVED_URI_SCHEME} links");

    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row.add_css_class("okp-settings-switch-row");

    let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
    text.set_hexpand(true);
    let label = gtk::Label::new(Some(&title));
    label.add_css_class("okp-info-label");
    label.set_xalign(0.0);
    text.append(&label);
    let detail = gtk::Label::new(Some(if flatpak {
        "The Flatpak build does not handle these links."
    } else {
        "Lets web pages and local HTML files open media in OK Player. Off removes OK Player as a handler for these links."
    }));
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

    let toggle = settings_switch_button(active, &title);
    // Inside the sandbox `$XDG_CONFIG_HOME` is the app's own directory, which the desktop never
    // reads, so the switch could not change anything.
    toggle.set_sensitive(!flatpak);
    toggle.connect_clicked(move |button| {
        let enabled = !button.has_css_class("is-active");
        let Some(config_home) = mimeapps::user_config_home() else {
            status.set_text("Could not change links: no configuration directory (HOME is unset).");
            status_toast.show("Could not change links");
            return;
        };
        let restore_default = state.borrow().settings.restore_uri_scheme_default();
        match set_uri_scheme_handling(&config_home, enabled, restore_default) {
            Ok(change) => {
                {
                    let mut state = state.borrow_mut();
                    state
                        .settings
                        .set_restore_uri_scheme_default(change.restore_default_on_enable);
                    save_settings_or_toast(&mut state, &status_toast);
                }
                set_settings_switch_active(button, enabled);
                state_label.set_text(if enabled { "On" } else { "Off" });
                status.set_text(&if !enabled {
                    format!(
                        "{LINUX_RESERVED_URI_SCHEME} links turned off; browsers no longer offer OK Player for them."
                    )
                } else if change.restored_default {
                    format!(
                        "{LINUX_RESERVED_URI_SCHEME} links turned on; OK Player is their default again."
                    )
                } else {
                    format!("{LINUX_RESERVED_URI_SCHEME} links turned on.")
                });
                status_toast.show(&format!(
                    "{LINUX_RESERVED_URI_SCHEME} links {}",
                    if enabled { "on" } else { "off" }
                ));
                scheme_row.show(UriSchemeState::current());
            }
            Err(error) => {
                eprintln!("ok-player:// handling: could not update mimeapps.list: {error}");
                status.set_text(&format!("Could not update mimeapps.list: {error}"));
                status_toast.show("Could not change links");
            }
        }
    });
    row.append(&toggle);

    row
}

/// Whether the `mimeapps.list` under `config_home` leaves the `ok-player://` association in
/// place. A file that cannot be read counts as on, the default, and is logged.
pub(crate) fn uri_scheme_handling_enabled(config_home: &Path) -> bool {
    match mimeapps::read_mimeapps(&mimeapps::user_mimeapps_path(config_home)) {
        Ok(list) => !list.contains(
            mimeapps::REMOVED_ASSOCIATIONS,
            LINUX_URI_SCHEME_MIME,
            LINUX_DESKTOP_ID,
        ),
        Err(error) => {
            eprintln!("ok-player:// handling: could not read mimeapps.list: {error}");
            true
        }
    }
}

pub(crate) fn user_uri_scheme_handling_enabled() -> bool {
    mimeapps::user_config_home().is_none_or(|home| uri_scheme_handling_enabled(&home))
}

/// What a change of the link toggle did, for the caller to persist and report.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct UriSchemeHandlingChange {
    /// Remember for the next time handling is turned on (`IntegrationSettings`).
    pub(crate) restore_default_on_enable: bool,
    /// Turning handling on made OK Player the scheme's default handler again.
    pub(crate) restored_default: bool,
}

/// Turn `ok-player://` handling on or off in the `mimeapps.list` under `config_home`.
///
/// Off adds OK Player to `[Removed Associations]` for the scheme, drops it from `[Added
/// Associations]` (listing an app in both is invalid per the spec), and drops it from `[Default
/// Applications]`: GIO and xdg-mime both still answer with a `[Default Applications]` entry
/// whose association is removed. When OK Player was the default, the change asks the caller to
/// remember that. On removes the `[Removed Associations]` entry and, when
/// `restore_default_on_enable` is set, makes OK Player the scheme's default again.
pub(crate) fn set_uri_scheme_handling(
    config_home: &Path,
    enabled: bool,
    restore_default_on_enable: bool,
) -> std::io::Result<UriSchemeHandlingChange> {
    use mimeapps::{ADDED_ASSOCIATIONS, DEFAULT_APPLICATIONS, ListPosition, REMOVED_ASSOCIATIONS};

    let path = mimeapps::user_mimeapps_path(config_home);
    mimeapps::update_mimeapps(&path, |list| {
        if enabled {
            list.remove(
                REMOVED_ASSOCIATIONS,
                LINUX_URI_SCHEME_MIME,
                LINUX_DESKTOP_ID,
            );
            if restore_default_on_enable {
                list.insert(
                    DEFAULT_APPLICATIONS,
                    LINUX_URI_SCHEME_MIME,
                    LINUX_DESKTOP_ID,
                    ListPosition::First,
                );
            }
            UriSchemeHandlingChange {
                restore_default_on_enable: false,
                restored_default: restore_default_on_enable,
            }
        } else {
            let was_default = list
                .entries(DEFAULT_APPLICATIONS, LINUX_URI_SCHEME_MIME)
                .first()
                .is_some_and(|id| id == LINUX_DESKTOP_ID);
            list.remove(
                DEFAULT_APPLICATIONS,
                LINUX_URI_SCHEME_MIME,
                LINUX_DESKTOP_ID,
            );
            list.remove(ADDED_ASSOCIATIONS, LINUX_URI_SCHEME_MIME, LINUX_DESKTOP_ID);
            list.insert(
                REMOVED_ASSOCIATIONS,
                LINUX_URI_SCHEME_MIME,
                LINUX_DESKTOP_ID,
                ListPosition::Last,
            );
            UriSchemeHandlingChange {
                restore_default_on_enable: restore_default_on_enable || was_default,
                restored_default: false,
            }
        }
    })
}

pub(crate) fn count_default_key_media_mimes() -> usize {
    LINUX_KEY_MEDIA_MIME_TYPES
        .iter()
        .filter(|mime| {
            query_default_app_for_mime(mime)
                .as_deref()
                .is_some_and(default_app_matches_ok_player)
        })
        .count()
}

pub(crate) fn query_default_app_for_mime(mime: &str) -> Option<String> {
    let output = Command::new("xdg-mime")
        .arg("query")
        .arg("default")
        .arg(mime)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (!value.is_empty()).then_some(value)
}

pub(crate) fn default_app_matches_ok_player(desktop_id: &str) -> bool {
    desktop_id.trim() == LINUX_DESKTOP_ID
}

/// The MIME types Make Default claims: the key media types and, unless link handling is off,
/// `x-scheme-handler/ok-player`.
pub(crate) fn default_app_mime_types(claim_uri_scheme: bool) -> Vec<&'static str> {
    let mut mimes = LINUX_KEY_MEDIA_MIME_TYPES.to_vec();
    if claim_uri_scheme {
        mimes.push(LINUX_URI_SCHEME_MIME);
    }
    mimes
}

/// Make OK Player the default for the key media types and, with `claim_uri_scheme`, for
/// `ok-player://` links. Returns the number of key media types claimed.
pub(crate) fn set_linux_default_app(claim_uri_scheme: bool) -> Result<usize, String> {
    if linux_desktop_entry_path().is_none() {
        return Err(format!("{LINUX_DESKTOP_ID} is not installed"));
    }
    let xdg_mime =
        find_executable("xdg-mime").ok_or_else(|| "xdg-mime is not installed".to_owned())?;
    let mut failures = Vec::new();
    for mime in default_app_mime_types(claim_uri_scheme) {
        match Command::new(&xdg_mime)
            .arg("default")
            .arg(LINUX_DESKTOP_ID)
            .arg(mime)
            .status()
        {
            Ok(status) if status.success() => {}
            Ok(status) => failures.push(format!("{mime} ({status})")),
            Err(error) => failures.push(format!("{mime} ({error})")),
        }
    }
    if failures.is_empty() {
        Ok(LINUX_KEY_MEDIA_MIME_TYPES.len())
    } else {
        Err(failures.join(", "))
    }
}

pub(crate) fn refresh_linux_desktop_database() -> Result<String, String> {
    let updater = find_executable("update-desktop-database")
        .ok_or_else(|| "update-desktop-database is not installed".to_owned())?;
    let mut attempted = Vec::new();
    for dir in linux_application_dirs()
        .into_iter()
        .filter(|dir| dir.is_dir())
        .filter(|dir| dir.starts_with(user_data_home()))
    {
        attempted.push(dir.clone());
        match Command::new(&updater).arg(&dir).status() {
            Ok(status) if status.success() => {
                return Ok(format!("Refreshed {}.", dir.to_string_lossy()));
            }
            Ok(status) => eprintln!(
                "update-desktop-database failed for {}: {status}",
                dir.display()
            ),
            Err(error) => eprintln!(
                "update-desktop-database failed for {}: {error}",
                dir.display()
            ),
        }
    }

    if linux_desktop_entry_path().is_some() {
        Ok(
            "System desktop entry is installed; package manager owns the system database."
                .to_owned(),
        )
    } else if attempted.is_empty() {
        Err("no user application directory found".to_owned())
    } else {
        Err("no application database could be refreshed".to_owned())
    }
}

pub(crate) fn user_data_home() -> PathBuf {
    if let Some(data_home) = env::var_os("XDG_DATA_HOME").filter(|value| !value.is_empty()) {
        PathBuf::from(data_home)
    } else if let Some(home) = env::var_os("HOME").filter(|value| !value.is_empty()) {
        PathBuf::from(home).join(".local/share")
    } else {
        env::temp_dir()
    }
}

pub(crate) fn open_linux_default_apps_settings() -> Result<(), String> {
    for (program, args) in [
        ("gnome-control-center", &["default-apps"][..]),
        ("kcmshell6", &["componentchooser"][..]),
        ("kcmshell5", &["componentchooser"][..]),
        ("systemsettings", &["kcm_componentchooser"][..]),
    ] {
        if find_executable(program).is_some() && Command::new(program).args(args).spawn().is_ok() {
            return Ok(());
        }
    }

    Command::new("xdg-open")
        .arg("settings://default-apps")
        .spawn()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

pub(crate) fn linux_integration_diagnostics(snapshot: &LinuxIntegrationSnapshot) -> String {
    let desktop_entry = snapshot
        .desktop_entry_path
        .as_ref()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|| "missing".to_owned());
    let defaults = snapshot
        .default_key_mimes
        .map(|count| format!("{count}/{}", LINUX_KEY_MEDIA_MIME_TYPES.len()))
        .unwrap_or_else(|| "unavailable".to_owned());
    let uri_scheme = snapshot.uri_scheme.diagnostics();
    format!(
        "OK Player Linux Integration\nVersion: {APP_BUILD_VERSION}\nBuild: {APP_BUILD_SHA}\nDesktop ID: {LINUX_DESKTOP_ID}\nDesktop entry: {desktop_entry}\nRegistered key MIME types: {}/{}\nDefault key MIME types: {defaults}\nURI scheme ({LINUX_RESERVED_URI_SCHEME}): {uri_scheme}\nxdg-mime: {}\nupdate-desktop-database: {}\n",
        snapshot.registered_key_mimes,
        LINUX_KEY_MEDIA_MIME_TYPES.len(),
        if snapshot.xdg_mime_available {
            "available"
        } else {
            "missing"
        },
        if snapshot.update_desktop_database_available {
            "available"
        } else {
            "missing"
        },
    )
}
