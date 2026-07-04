use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use okp_core::screenshot::{self, ScreenshotFormat};

/// Resolve the collision-free target path for a screenshot in `dir`, named after the media
/// and position and carrying `format`'s extension. Returns `None` if the directory can't be
/// created or every candidate name is already taken — the caller surfaces that as an error
/// rather than overwriting an existing capture. The name itself is composed by
/// `okp_core::screenshot`; only the directory IO and the timestamp live here.
pub fn next_screenshot_path(
    dir: &Path,
    media_path: Option<&Path>,
    position: Option<f64>,
    format: ScreenshotFormat,
) -> Option<PathBuf> {
    fs::create_dir_all(dir).ok()?;
    let media_stem = media_path
        .and_then(Path::file_stem)
        .and_then(|name| name.to_str());
    let stem = screenshot::screenshot_stem(media_stem, position, unix_millis());
    let name =
        screenshot::resolve_unique_name(&stem, format.extension(), |name| dir.join(name).exists())?;
    Some(dir.join(name))
}

/// A transient temp path for a clipboard frame, resolved collision-free. The file is deleted
/// once the frame is on the clipboard; PNG keeps it lossless for the paste target. Returns
/// `None` if the temp directory can't be prepared or no free name is available.
pub fn next_clipboard_frame_path() -> Option<PathBuf> {
    let dir = env::temp_dir().join("ok-player");
    fs::create_dir_all(&dir).ok()?;
    let stem = format!("clipboard-frame-{}", unix_millis());
    let name = screenshot::resolve_unique_name(&stem, "png", |name| dir.join(name).exists())?;
    Some(dir.join(name))
}

/// The directory captures are written to: the user's configured folder when set, otherwise
/// the platform default (`$XDG_PICTURES_DIR/OK Player`, falling back to `~/Pictures` or temp).
pub fn screenshot_dir(configured: Option<PathBuf>) -> PathBuf {
    configured.unwrap_or_else(default_screenshot_dir)
}

fn default_screenshot_dir() -> PathBuf {
    xdg_pictures_dir()
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join("Pictures")))
        .unwrap_or_else(env::temp_dir)
        .join("OK Player")
}

fn xdg_pictures_dir() -> Option<PathBuf> {
    let home = env::var_os("HOME").map(PathBuf::from)?;
    let user_dirs = fs::read_to_string(home.join(".config/user-dirs.dirs")).ok()?;
    parse_xdg_pictures_dir(&home, &user_dirs)
}

fn parse_xdg_pictures_dir(home: &Path, user_dirs: &str) -> Option<PathBuf> {
    for line in user_dirs.lines() {
        let line = line.trim();
        if let Some(value) = line.strip_prefix("XDG_PICTURES_DIR=") {
            let value = value.trim_matches('"');
            let value = value.replace("$HOME", &home.to_string_lossy());
            if !value.is_empty() {
                return Some(PathBuf::from(value));
            }
        }
    }

    None
}

fn unix_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screenshot_dir_prefers_the_configured_directory() {
        let configured = PathBuf::from("/home/tester/Captures");
        assert_eq!(
            screenshot_dir(Some(configured.clone())),
            configured,
            "a configured directory should be used verbatim"
        );
    }

    #[test]
    fn parse_xdg_pictures_dir_reads_standard_user_dirs_file() {
        let home = Path::new("/home/tester");
        let user_dirs = r#"
XDG_DESKTOP_DIR="$HOME/Desktop"
XDG_PICTURES_DIR="$HOME/Pictures"
"#;

        assert_eq!(
            parse_xdg_pictures_dir(home, user_dirs),
            Some(PathBuf::from("/home/tester/Pictures"))
        );
    }
}
