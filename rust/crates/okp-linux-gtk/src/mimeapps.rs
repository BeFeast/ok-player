//! The user's `mimeapps.list` (freedesktop MIME Applications Associations spec 1.0.1).
//!
//! OK Player edits it line by line: every group, key, comment and blank line it does not touch
//! is written back byte for byte, and a line it does rewrite keeps its own line ending. The only
//! writer today is the `ok-player://` link toggle in Settings → Integration (#850).

use std::fs;
use std::io::{self, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

pub(crate) const DEFAULT_APPLICATIONS: &str = "Default Applications";
pub(crate) const ADDED_ASSOCIATIONS: &str = "Added Associations";
pub(crate) const REMOVED_ASSOCIATIONS: &str = "Removed Associations";

const MIMEAPPS_FILE_NAME: &str = "mimeapps.list";
/// Mode of a `mimeapps.list` OK Player creates; an existing file keeps its own mode.
const NEW_FILE_MODE: u32 = 0o644;

/// Where a desktop ID goes when it is added to a key's list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ListPosition {
    /// The most preferred entry: a `[Default Applications]` value is tried in order.
    First,
    /// After the existing entries.
    Last,
}

/// The text of one `mimeapps.list`, kept as its original lines (terminators included).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct MimeAppsList {
    lines: Vec<String>,
}

impl MimeAppsList {
    pub(crate) fn parse(text: &str) -> Self {
        Self {
            lines: text.split_inclusive('\n').map(str::to_owned).collect(),
        }
    }

    pub(crate) fn render(&self) -> String {
        self.lines.concat()
    }

    /// The desktop IDs listed for `mime` in `group`, in file order.
    pub(crate) fn entries(&self, group: &str, mime: &str) -> Vec<String> {
        self.key_lines(group, mime)
            .into_iter()
            .flat_map(|index| desktop_ids(value_of(&self.lines[index])))
            .collect()
    }

    pub(crate) fn contains(&self, group: &str, mime: &str, desktop_id: &str) -> bool {
        self.entries(group, mime).iter().any(|id| id == desktop_id)
    }

    /// Drop `desktop_id` from every `mime=` line of `group`; a line left without entries is
    /// removed. Returns whether anything changed.
    pub(crate) fn remove(&mut self, group: &str, mime: &str, desktop_id: &str) -> bool {
        let mut changed = false;
        for index in self.key_lines(group, mime).into_iter().rev() {
            let ids = desktop_ids(value_of(&self.lines[index]));
            if !ids.iter().any(|id| id == desktop_id) {
                continue;
            }
            changed = true;
            let kept: Vec<String> = ids.into_iter().filter(|id| id != desktop_id).collect();
            if kept.is_empty() {
                self.lines.remove(index);
            } else {
                let ending = line_ending(&self.lines[index]).to_owned();
                self.lines[index] = key_line(mime, &kept, &ending);
            }
        }
        changed
    }

    /// Make sure `desktop_id` is listed for `mime` in `group`, creating the key or the group when
    /// they are missing. Returns whether anything changed.
    pub(crate) fn insert(
        &mut self,
        group: &str,
        mime: &str,
        desktop_id: &str,
        position: ListPosition,
    ) -> bool {
        if let Some(&index) = self.key_lines(group, mime).first() {
            let mut ids = desktop_ids(value_of(&self.lines[index]));
            let present = ids.iter().position(|id| id == desktop_id);
            match (present, position) {
                (Some(0), _) | (Some(_), ListPosition::Last) => return false,
                (Some(at), ListPosition::First) => {
                    let id = ids.remove(at);
                    ids.insert(0, id);
                }
                (None, ListPosition::First) => ids.insert(0, desktop_id.to_owned()),
                (None, ListPosition::Last) => ids.push(desktop_id.to_owned()),
            }
            let ending = line_ending(&self.lines[index]).to_owned();
            self.lines[index] = key_line(mime, &ids, &ending);
            return true;
        }

        let new_line = key_line(mime, &[desktop_id.to_owned()], "\n");
        match self.group_body(group) {
            Some((header, end)) => {
                // After the group's last entry, so trailing blank lines and a comment that
                // introduces the next group stay where they are.
                let after = (header + 1..end)
                    .rev()
                    .find(|&index| key_value(&self.lines[index]).is_some())
                    .unwrap_or(header);
                self.terminate_line(after);
                self.lines.insert(after + 1, new_line);
            }
            None => {
                if let Some(last) = self.lines.len().checked_sub(1) {
                    self.terminate_line(last);
                    if !self.lines[last].trim().is_empty() {
                        self.lines.push("\n".to_owned());
                    }
                }
                self.lines.push(format!("[{group}]\n"));
                self.lines.push(new_line);
            }
        }
        true
    }

    /// The header index and the end (exclusive) of the first occurrence of `group`.
    fn group_body(&self, group: &str) -> Option<(usize, usize)> {
        let header = self
            .lines
            .iter()
            .position(|line| group_name(line) == Some(group))?;
        let end = (header + 1..self.lines.len())
            .find(|&index| group_name(&self.lines[index]).is_some())
            .unwrap_or(self.lines.len());
        Some((header, end))
    }

    /// Indices of the `mime=` lines inside every occurrence of `group`.
    fn key_lines(&self, group: &str, mime: &str) -> Vec<usize> {
        let mut in_group = false;
        let mut found = Vec::new();
        for (index, line) in self.lines.iter().enumerate() {
            if let Some(name) = group_name(line) {
                in_group = name == group;
            } else if in_group && key_value(line).is_some_and(|(key, _)| key == mime) {
                found.push(index);
            }
        }
        found
    }

    fn terminate_line(&mut self, index: usize) {
        if !self.lines[index].ends_with('\n') {
            self.lines[index].push('\n');
        }
    }
}

fn group_name(line: &str) -> Option<&str> {
    line.trim().strip_prefix('[')?.strip_suffix(']')
}

/// `key=value` of an entry line; comments, blank lines and group headers are not entries.
/// Whitespace around the `=` is ignored, as the Desktop Entry format allows.
fn key_value(line: &str) -> Option<(&str, &str)> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('[') {
        return None;
    }
    let (key, value) = trimmed.split_once('=')?;
    Some((key.trim(), value.trim()))
}

fn value_of(line: &str) -> &str {
    key_value(line).map_or("", |(_, value)| value)
}

fn desktop_ids(value: &str) -> Vec<String> {
    value
        .split(';')
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
        .collect()
}

fn key_line(mime: &str, ids: &[String], ending: &str) -> String {
    format!("{mime}={};{ending}", ids.join(";"))
}

fn line_ending(line: &str) -> &str {
    if line.ends_with("\r\n") {
        "\r\n"
    } else if line.ends_with('\n') {
        "\n"
    } else {
        ""
    }
}

/// `$XDG_CONFIG_HOME`, or `~/.config` when it is unset, empty or not absolute (the XDG Base
/// Directory spec says a relative value is invalid and must be ignored).
pub(crate) fn user_config_home() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|home| !home.is_empty())
                .map(|home| PathBuf::from(home).join(".config"))
        })
}

/// The `mimeapps.list` a configuration UI writes: `$XDG_CONFIG_HOME/mimeapps.list`.
pub(crate) fn user_mimeapps_path(config_home: &Path) -> PathBuf {
    config_home.join(MIMEAPPS_FILE_NAME)
}

/// Read a `mimeapps.list`; a missing file reads as an empty one, as the spec says.
pub(crate) fn read_mimeapps(path: &Path) -> io::Result<MimeAppsList> {
    match fs::read(path) {
        Ok(bytes) => Ok(MimeAppsList::parse(&utf8(bytes)?)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(MimeAppsList::default()),
        Err(error) => Err(error),
    }
}

/// Apply `edit` to the `mimeapps.list` at `path` and write the result back only when the text
/// changed. The new text replaces the file atomically from a temporary file in the same
/// directory and keeps the file's mode; a symlinked `mimeapps.list` (a dotfiles checkout) has
/// its target replaced and stays a symlink.
pub(crate) fn update_mimeapps<T>(
    path: &Path,
    edit: impl FnOnce(&mut MimeAppsList) -> T,
) -> io::Result<T> {
    let target = if fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink()) {
        fs::canonicalize(path)?
    } else {
        path.to_path_buf()
    };
    let (original, mode) = match fs::read(&target) {
        Ok(bytes) => {
            let mode = fs::metadata(&target)?.permissions().mode() & 0o7777;
            (utf8(bytes)?, mode)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => (String::new(), NEW_FILE_MODE),
        Err(error) => return Err(error),
    };

    let mut list = MimeAppsList::parse(&original);
    let outcome = edit(&mut list);
    let updated = list.render();
    if updated == original {
        return Ok(outcome);
    }

    let directory = target
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(directory)?;
    let mut temporary = tempfile::Builder::new()
        .prefix(".mimeapps.list.")
        .suffix(".okp-tmp")
        .tempfile_in(directory)?;
    temporary.write_all(updated.as_bytes())?;
    temporary
        .as_file()
        .set_permissions(fs::Permissions::from_mode(mode))?;
    temporary.as_file().sync_all()?;
    temporary.persist(&target).map_err(|error| error.error)?;
    // Make the rename itself durable; a failure here leaves the new file in place.
    if let Ok(directory) = fs::File::open(directory) {
        let _ = directory.sync_all();
    }
    Ok(outcome)
}

fn utf8(bytes: Vec<u8>) -> io::Result<String> {
    String::from_utf8(bytes).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "mimeapps.list is not valid UTF-8",
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use okp_test_fixtures::unique_temp_dir;
    use std::os::unix::fs::symlink;

    const SCHEME: &str = "x-scheme-handler/ok-player";
    const ID: &str = "com.befeast.okplayer.desktop";

    #[test]
    fn untouched_list_renders_byte_for_byte() {
        let text = "# comment\r\n[Default Applications]\r\ntext/html = firefox.desktop\r\n\r\n[Added Associations]\nimage/png=eog.desktop;\nno-trailing-newline";
        assert_eq!(MimeAppsList::parse(text).render(), text);
    }

    #[test]
    fn insert_appends_to_an_existing_key_and_keeps_its_line_ending() {
        let mut list = MimeAppsList::parse(
            "[Removed Associations]\r\nx-scheme-handler/ok-player=other.desktop\r\n",
        );
        assert!(list.insert(REMOVED_ASSOCIATIONS, SCHEME, ID, ListPosition::Last));
        assert_eq!(
            list.render(),
            "[Removed Associations]\r\nx-scheme-handler/ok-player=other.desktop;com.befeast.okplayer.desktop;\r\n"
        );
        assert!(!list.insert(REMOVED_ASSOCIATIONS, SCHEME, ID, ListPosition::Last));
    }

    #[test]
    fn insert_first_moves_an_existing_entry_to_the_front() {
        let mut list = MimeAppsList::parse(
            "[Default Applications]\nx-scheme-handler/ok-player=other.desktop;com.befeast.okplayer.desktop;\n",
        );
        assert!(list.insert(DEFAULT_APPLICATIONS, SCHEME, ID, ListPosition::First));
        assert_eq!(
            list.entries(DEFAULT_APPLICATIONS, SCHEME),
            vec![ID.to_owned(), "other.desktop".to_owned()]
        );
        assert!(!list.insert(DEFAULT_APPLICATIONS, SCHEME, ID, ListPosition::First));
    }

    #[test]
    fn insert_adds_a_key_after_the_last_entry_of_its_group() {
        let mut list = MimeAppsList::parse(
            "[Removed Associations]\nimage/png=eog.desktop;\n\n# browsers\n[Default Applications]\ntext/html=firefox.desktop;\n",
        );
        assert!(list.insert(REMOVED_ASSOCIATIONS, SCHEME, ID, ListPosition::Last));
        assert_eq!(
            list.render(),
            "[Removed Associations]\nimage/png=eog.desktop;\nx-scheme-handler/ok-player=com.befeast.okplayer.desktop;\n\n# browsers\n[Default Applications]\ntext/html=firefox.desktop;\n"
        );
    }

    #[test]
    fn insert_creates_a_missing_group_at_the_end_after_a_blank_line() {
        let mut list = MimeAppsList::parse("[Default Applications]\ntext/html=firefox.desktop;");
        assert!(list.insert(REMOVED_ASSOCIATIONS, SCHEME, ID, ListPosition::Last));
        assert_eq!(
            list.render(),
            "[Default Applications]\ntext/html=firefox.desktop;\n\n[Removed Associations]\nx-scheme-handler/ok-player=com.befeast.okplayer.desktop;\n"
        );

        let mut empty = MimeAppsList::default();
        assert!(empty.insert(REMOVED_ASSOCIATIONS, SCHEME, ID, ListPosition::Last));
        assert_eq!(
            empty.render(),
            "[Removed Associations]\nx-scheme-handler/ok-player=com.befeast.okplayer.desktop;\n"
        );
    }

    #[test]
    fn remove_drops_only_the_named_id_and_an_emptied_line() {
        let mut list = MimeAppsList::parse(
            "[Default Applications]\nx-scheme-handler/ok-player=com.befeast.okplayer.desktop;\nvideo/mp4=com.befeast.okplayer.desktop;\n[Added Associations]\nx-scheme-handler/ok-player = other.desktop ; com.befeast.okplayer.desktop\n",
        );
        assert!(list.remove(DEFAULT_APPLICATIONS, SCHEME, ID));
        assert!(list.remove(ADDED_ASSOCIATIONS, SCHEME, ID));
        assert_eq!(
            list.render(),
            "[Default Applications]\nvideo/mp4=com.befeast.okplayer.desktop;\n[Added Associations]\nx-scheme-handler/ok-player=other.desktop;\n"
        );
        assert!(!list.remove(DEFAULT_APPLICATIONS, SCHEME, ID));
    }

    #[test]
    fn a_key_with_the_same_name_in_another_group_is_left_alone() {
        let list = MimeAppsList::parse(
            "[Added Associations]\nx-scheme-handler/ok-player=com.befeast.okplayer.desktop;\n",
        );
        assert!(!list.contains(REMOVED_ASSOCIATIONS, SCHEME, ID));
        assert!(list.contains(ADDED_ASSOCIATIONS, SCHEME, ID));
    }

    #[test]
    fn update_writes_only_on_change_and_keeps_the_file_mode() {
        let config = unique_temp_dir("okp-mimeapps-mode");
        let path = user_mimeapps_path(config.path());
        fs::write(
            &path,
            "[Default Applications]\ntext/html=firefox.desktop;\n",
        )
        .unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        let before = fs::metadata(&path).unwrap().modified().unwrap();

        update_mimeapps(&path, |list| list.remove(REMOVED_ASSOCIATIONS, SCHEME, ID)).unwrap();
        assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), before);

        update_mimeapps(&path, |list| {
            list.insert(REMOVED_ASSOCIATIONS, SCHEME, ID, ListPosition::Last)
        })
        .unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "[Default Applications]\ntext/html=firefox.desktop;\n\n[Removed Associations]\nx-scheme-handler/ok-player=com.befeast.okplayer.desktop;\n"
        );
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o7777,
            0o600
        );
        let leftovers: Vec<_> = fs::read_dir(config.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .filter(|name| name != MIMEAPPS_FILE_NAME)
            .collect();
        assert!(leftovers.is_empty(), "temporary files left: {leftovers:?}");
    }

    #[test]
    fn update_creates_the_config_directory_and_file() {
        let root = unique_temp_dir("okp-mimeapps-create");
        let config = root.path().join("config");
        let path = user_mimeapps_path(&config);
        update_mimeapps(&path, |list| {
            list.insert(REMOVED_ASSOCIATIONS, SCHEME, ID, ListPosition::Last)
        })
        .unwrap();
        assert!(
            read_mimeapps(&path)
                .unwrap()
                .contains(REMOVED_ASSOCIATIONS, SCHEME, ID)
        );
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o7777,
            NEW_FILE_MODE
        );
    }

    #[test]
    fn update_replaces_a_symlink_target_and_keeps_the_link() {
        let root = unique_temp_dir("okp-mimeapps-symlink");
        let dotfiles = root.path().join("dotfiles");
        let config = root.path().join("config");
        fs::create_dir_all(&dotfiles).unwrap();
        fs::create_dir_all(&config).unwrap();
        let real = dotfiles.join("mimeapps.list");
        fs::write(&real, "[Default Applications]\n").unwrap();
        let link = user_mimeapps_path(&config);
        symlink(&real, &link).unwrap();

        update_mimeapps(&link, |list| {
            list.insert(REMOVED_ASSOCIATIONS, SCHEME, ID, ListPosition::Last)
        })
        .unwrap();
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert!(
            read_mimeapps(&real)
                .unwrap()
                .contains(REMOVED_ASSOCIATIONS, SCHEME, ID)
        );
    }

    #[test]
    fn a_file_that_is_not_utf8_is_refused_and_left_unchanged() {
        let config = unique_temp_dir("okp-mimeapps-binary");
        let path = user_mimeapps_path(config.path());
        let bytes = b"[Default Applications]\ntext/html=\xff.desktop;\n".to_vec();
        fs::write(&path, &bytes).unwrap();
        let error = update_mimeapps(&path, |list| {
            list.insert(REMOVED_ASSOCIATIONS, SCHEME, ID, ListPosition::Last)
        })
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }
}
