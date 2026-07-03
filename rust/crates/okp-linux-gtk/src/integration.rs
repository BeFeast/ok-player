use super::*;

pub(crate) fn settings_integration_section(status_toast: Rc<StatusToast>) -> gtk::Box {
    let snapshot = LinuxIntegrationSnapshot::capture();
    let section = settings_section("Integration");

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

    let status = gtk::Label::new(Some("Ready"));
    status.add_css_class("okp-update-status");
    status.set_xalign(0.0);
    status.set_width_chars(1);
    status.set_max_width_chars(58);
    status.set_wrap(true);
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
    make_default.connect_clicked(move |button| {
        button.set_sensitive(false);
        match set_linux_default_app_for_key_mimes() {
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
                make_default_status.set_text(&format!(
                    "OK Player set as default for {count} key media types."
                ));
                make_default_toast.show("Default media app updated");
            }
            Err(error) => {
                make_default_status.set_text(&format!("Could not update defaults: {error}"));
                make_default_toast.show("Could not update defaults");
            }
        }
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

    (row, value)
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

pub(crate) fn set_linux_default_app_for_key_mimes() -> Result<usize, String> {
    if linux_desktop_entry_path().is_none() {
        return Err(format!("{LINUX_DESKTOP_ID} is not installed"));
    }
    let xdg_mime =
        find_executable("xdg-mime").ok_or_else(|| "xdg-mime is not installed".to_owned())?;
    let mut failures = Vec::new();
    for mime in LINUX_KEY_MEDIA_MIME_TYPES {
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
    format!(
        "OK Player Linux Integration\nVersion: {APP_BUILD_VERSION}\nBuild: {APP_BUILD_SHA}\nDesktop ID: {LINUX_DESKTOP_ID}\nDesktop entry: {desktop_entry}\nRegistered key MIME types: {}/{}\nDefault key MIME types: {defaults}\nxdg-mime: {}\nupdate-desktop-database: {}\n",
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
