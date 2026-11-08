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
        let result = make_default_for_user(&mut make_default_state.borrow_mut().settings);
        match result {
            Ok(outcome) => {
                let count = outcome.key_media_types;
                set_integration_state_pill(
                    &make_default_value,
                    &format!("{count}/{} key types", LINUX_KEY_MEDIA_MIME_TYPES.len()),
                    if count == LINUX_KEY_MEDIA_MIME_TYPES.len() {
                        IntegrationStatus::Good
                    } else {
                        IntegrationStatus::Warning
                    },
                );
                make_default_status.set_text(&outcome.status());
                make_default_toast.show(if outcome.settings_saved {
                    "Default media app updated"
                } else {
                    "Could not save settings"
                });
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

/// Runs `xdg-mime`. The app runs it with its own environment; a test runs the real tool with an
/// environment of its own, so that it reads and writes only an isolated XDG tree.
#[derive(Clone, Debug)]
pub(crate) struct XdgMime {
    program: PathBuf,
    /// When set, the whole environment the tool sees; nothing is inherited.
    environment: Option<Vec<(std::ffi::OsString, std::ffi::OsString)>>,
}

impl XdgMime {
    /// The `xdg-mime` on `PATH`, run with the app's environment.
    pub(crate) fn find() -> Option<Self> {
        find_executable("xdg-mime").map(|program| Self {
            program,
            environment: None,
        })
    }

    /// `program` run with exactly `environment`.
    #[cfg(test)]
    pub(crate) fn with_environment(
        program: PathBuf,
        environment: Vec<(std::ffi::OsString, std::ffi::OsString)>,
    ) -> Self {
        Self {
            program,
            environment: Some(environment),
        }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(&self.program);
        if let Some(environment) = &self.environment {
            command.env_clear().envs(environment.iter().cloned());
        }
        command
    }

    /// `xdg-mime query default <mime>`: the desktop ID it names, if any.
    pub(crate) fn query_default(&self, mime: &str) -> Option<String> {
        let output = self
            .command()
            .arg("query")
            .arg("default")
            .arg(mime)
            .stderr(Stdio::null())
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let value = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        (!value.is_empty()).then_some(value)
    }

    /// Whether `xdg-mime query default <mime>` names OK Player.
    pub(crate) fn names_ok_player(&self, mime: &str) -> bool {
        self.query_default(mime)
            .as_deref()
            .is_some_and(default_app_matches_ok_player)
    }

    /// `xdg-mime default <desktop_id> <mime>`.
    pub(crate) fn set_default(&self, desktop_id: &str, mime: &str) -> Result<(), String> {
        match self
            .command()
            .arg("default")
            .arg(desktop_id)
            .arg(mime)
            .status()
        {
            Ok(status) if status.success() => Ok(()),
            Ok(status) => Err(format!("{mime} ({status})")),
            Err(error) => Err(format!("{mime} ({error})")),
        }
    }
}

/// The `ok-player://` contract version this shell executes (epic #845).
pub(crate) const URI_SCHEME_CONTRACT: &str = "v1";

/// What the Settings row and status line add while handling is off but xdg-mime still names OK
/// Player (see [`UriSchemeState`]).
const XDG_MIME_STILL_REACHES: &str = "xdg-mime still names OK Player, so where xdg-open falls back to it (Hyprland, Sway and other desktops it does not recognise) a link can still open OK Player.";

/// Where `ok-player://` links stand on this desktop, as Settings and Copy Diagnostics report it.
///
/// Two lookups decide where a link goes, and only one of them honours the link switch. GIO
/// (Firefox, `gio open`, and `xdg-open` on GNOME and Cinnamon, which calls `gio open`) reads the
/// `[Removed Associations]` entry the switch writes. `xdg-mime query default` does not: with no
/// `[Default Applications]` entry it falls back to `mimeinfo.cache`, which the package's desktop
/// entry fills. `xdg-open` uses that lookup on every session it does not recognise as a known
/// desktop (Hyprland and Sway among them), so there a link still starts OK Player with the
/// switch off. The report says so instead of claiming the links are gone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct UriSchemeState {
    /// An installed desktop entry lists `x-scheme-handler/ok-player` in `MimeType=`.
    pub(crate) advertised: bool,
    /// The Settings switch: the user's `mimeapps.list` does not remove the association.
    pub(crate) handling: bool,
    /// `xdg-mime query default` names OK Player; `None` when xdg-mime is unavailable. It does
    /// not read `[Removed Associations]`, so it can stay true with handling off.
    pub(crate) default: Option<bool>,
}

impl UriSchemeState {
    /// Read the state from the installed desktop entry's text, the `mimeapps.list` under
    /// `config_home` (handling counts as on without one) and `xdg_mime`.
    pub(crate) fn read(
        desktop_entry: Option<&str>,
        config_home: Option<&Path>,
        xdg_mime: Option<&XdgMime>,
    ) -> Self {
        Self {
            advertised: desktop_entry.is_some_and(desktop_registers_uri_scheme),
            handling: config_home.is_none_or(uri_scheme_handling_enabled),
            default: xdg_mime.map(|xdg_mime| xdg_mime.names_ok_player(LINUX_URI_SCHEME_MIME)),
        }
    }

    /// A fresh reading of this user's desktop, for refreshing the Settings row after a change.
    pub(crate) fn current() -> Self {
        let desktop_entry =
            linux_desktop_entry_path().and_then(|path| fs::read_to_string(path).ok());
        Self::read(
            desktop_entry.as_deref(),
            mimeapps::user_config_home().as_deref(),
            XdgMime::find().as_ref(),
        )
    }

    /// Handling is off, yet `xdg-mime query default` still names OK Player, so `xdg-open` on a
    /// session it does not recognise still hands links to OK Player.
    pub(crate) fn reachable_via_xdg_mime_while_off(self) -> bool {
        self.advertised && !self.handling && self.default == Some(true)
    }

    /// The Copy Diagnostics value: `v1, handling on|off;` followed by `registered (default)`,
    /// `registered (not default)`, `registered (default unknown)` (no xdg-mime to ask),
    /// `still reachable via xdg-mime` (handling off, but xdg-mime still names OK Player) or
    /// `missing` (no installed desktop entry advertises the scheme, or handling is off and
    /// neither lookup names OK Player).
    pub(crate) fn diagnostics(self) -> String {
        let registration = match (self.advertised, self.handling, self.default) {
            (false, _, _) => "missing",
            (true, true, Some(true)) => "registered (default)",
            (true, true, Some(false)) => "registered (not default)",
            (true, true, None) => "registered (default unknown)",
            (true, false, Some(true)) => "still reachable via xdg-mime",
            (true, false, _) => "missing",
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
        if self.reachable_via_xdg_mime_while_off() {
            return (
                "Partly off",
                format!(
                    "{LINUX_RESERVED_URI_SCHEME} links are off for GIO, which Firefox and GNOME use. {XDG_MIME_STILL_REACHES}"
                ),
                IntegrationStatus::Warning,
            );
        }
        if !self.handling {
            return (
                "Off",
                format!(
                    "{LINUX_RESERVED_URI_SCHEME} links are turned off; OK Player is removed as their handler."
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
/// user's `mimeapps.list`, so GIO stops offering OK Player for the scheme; on removes it again.
/// It is not a player-side refusal. Where `xdg-open` falls back to `xdg-mime`, which ignores
/// that entry, a link can still start the player; the row and the status line say so (see
/// [`UriSchemeState`]).
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
        let result = apply_link_toggle(&config_home, &mut state.borrow_mut().settings, enabled);
        match result {
            Ok(outcome) => {
                set_settings_switch_active(button, enabled);
                state_label.set_text(if enabled { "On" } else { "Off" });
                let after = UriSchemeState::current();
                status.set_text(&link_toggle_status(enabled, outcome.change, after));
                if outcome.settings_saved {
                    status_toast.show(&format!(
                        "{LINUX_RESERVED_URI_SCHEME} links {}",
                        if enabled { "on" } else { "off" }
                    ));
                } else {
                    status_toast.show("Could not save settings");
                }
                scheme_row.show(after);
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

/// What the link switch did: the `mimeapps.list` change and whether the settings were saved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LinkToggleOutcome {
    pub(crate) change: UriSchemeHandlingChange,
    /// The pending default restore reached the settings file. A failed save is logged; the
    /// `mimeapps.list` change stands.
    pub(crate) settings_saved: bool,
}

/// The link switch: turn handling on or off in the `mimeapps.list` under `config_home`, then keep
/// the pending default restore in `settings` and save them.
pub(crate) fn apply_link_toggle(
    config_home: &Path,
    settings: &mut settings::SettingsStore,
    enabled: bool,
) -> std::io::Result<LinkToggleOutcome> {
    let change =
        set_uri_scheme_handling(config_home, enabled, settings.restore_uri_scheme_default())?;
    settings.set_restore_uri_scheme_default(change.restore_default_on_enable);
    Ok(LinkToggleOutcome {
        change,
        settings_saved: save_integration_settings(settings),
    })
}

/// The status line after the link switch changed, from the change and a fresh reading.
pub(crate) fn link_toggle_status(
    enabled: bool,
    change: UriSchemeHandlingChange,
    after: UriSchemeState,
) -> String {
    if enabled {
        if change.restored_default {
            format!(
                "{LINUX_RESERVED_URI_SCHEME} links turned on; OK Player is their default again."
            )
        } else {
            format!("{LINUX_RESERVED_URI_SCHEME} links turned on.")
        }
    } else if after.reachable_via_xdg_mime_while_off() {
        format!(
            "{LINUX_RESERVED_URI_SCHEME} links turned off for GIO, which Firefox and GNOME use. {XDG_MIME_STILL_REACHES}"
        )
    } else {
        format!(
            "{LINUX_RESERVED_URI_SCHEME} links turned off; OK Player is removed as their handler."
        )
    }
}

/// What Make Default did about `ok-player://` links.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UriSchemeClaim {
    /// OK Player is now the scheme's default handler.
    Claimed,
    /// Links are off. A default for a removed association would turn them back on for GIO and
    /// xdg-mime, so the claim waits in the settings until the user turns links on.
    Deferred,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MakeDefaultOutcome {
    /// The number of key media types claimed.
    pub(crate) key_media_types: usize,
    pub(crate) uri_scheme: UriSchemeClaim,
    /// A deferred claim reached the settings file (true when there was nothing to save).
    pub(crate) settings_saved: bool,
}

impl MakeDefaultOutcome {
    /// The Settings status line.
    pub(crate) fn status(self) -> String {
        let count = self.key_media_types;
        match self.uri_scheme {
            UriSchemeClaim::Claimed => format!(
                "OK Player set as default for {count} key media types and {LINUX_RESERVED_URI_SCHEME} links."
            ),
            UriSchemeClaim::Deferred => format!(
                "OK Player set as default for {count} key media types. {LINUX_RESERVED_URI_SCHEME} links are off; OK Player becomes their default when you turn them on."
            ),
        }
    }
}

/// Make Default: run `make_default` (an `xdg-mime default` call in the app) for the key media
/// types and, while link handling is on in the `mimeapps.list` under `config_home`, for
/// `x-scheme-handler/ok-player`. While links are off the scheme is left alone and the claim is
/// kept in `settings`, which are saved, for the next time links are turned on.
pub(crate) fn apply_make_default(
    config_home: Option<&Path>,
    settings: &mut settings::SettingsStore,
    mut make_default: impl FnMut(&str) -> Result<(), String>,
) -> Result<MakeDefaultOutcome, String> {
    let handling = config_home.is_none_or(uri_scheme_handling_enabled);
    let failures: Vec<String> = default_app_mime_types(handling)
        .into_iter()
        .filter_map(|mime| make_default(mime).err())
        .collect();
    if !failures.is_empty() {
        return Err(failures.join(", "));
    }
    let uri_scheme = if handling {
        UriSchemeClaim::Claimed
    } else {
        settings.set_restore_uri_scheme_default(true);
        UriSchemeClaim::Deferred
    };
    Ok(MakeDefaultOutcome {
        key_media_types: LINUX_KEY_MEDIA_MIME_TYPES.len(),
        uri_scheme,
        settings_saved: save_integration_settings(settings),
    })
}

/// Make Default as the Settings button runs it: this user's config directory and the
/// `xdg-mime` on `PATH`.
fn make_default_for_user(
    settings: &mut settings::SettingsStore,
) -> Result<MakeDefaultOutcome, String> {
    if linux_desktop_entry_path().is_none() {
        return Err(format!("{LINUX_DESKTOP_ID} is not installed"));
    }
    let xdg_mime = XdgMime::find().ok_or_else(|| "xdg-mime is not installed".to_owned())?;
    apply_make_default(mimeapps::user_config_home().as_deref(), settings, |mime| {
        xdg_mime.set_default(LINUX_DESKTOP_ID, mime)
    })
}

/// Save `settings` after an integration change. A failure is logged and reported to the caller;
/// it does not undo the change already made to the desktop's associations.
fn save_integration_settings(settings: &mut settings::SettingsStore) -> bool {
    match settings.save() {
        Ok(()) => true,
        Err(error) => {
            eprintln!("Failed to save settings: {error}");
            false
        }
    }
}

pub(crate) fn count_default_key_media_mimes(xdg_mime: &XdgMime) -> usize {
    LINUX_KEY_MEDIA_MIME_TYPES
        .iter()
        .filter(|mime| xdg_mime.names_ok_player(mime))
        .count()
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
