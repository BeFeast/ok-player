use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn next_screenshot_path(media_path: Option<&Path>, position: Option<f64>) -> PathBuf {
    let base_name = media_path
        .and_then(Path::file_stem)
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map(sanitize_filename)
        .unwrap_or_else(|| "ok-player".to_owned());
    let position = position
        .filter(|value| value.is_finite() && *value >= 0.0)
        .map(|value| format!("-{}", time_slug(value)))
        .unwrap_or_default();
    let timestamp = unix_millis();
    let dir = screenshot_dir();
    let _ = fs::create_dir_all(&dir);

    for suffix in 0..100 {
        let unique = if suffix == 0 {
            String::new()
        } else {
            format!("-{suffix}")
        };
        let path = dir.join(format!("{base_name}{position}-{timestamp}{unique}.png"));
        if !path.exists() {
            return path;
        }
    }

    dir.join(format!("{base_name}{position}-{timestamp}-fallback.png"))
}

fn screenshot_dir() -> PathBuf {
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

fn sanitize_filename(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| match ch {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '-',
            ch if ch.is_control() => '-',
            ch => ch,
        })
        .collect::<String>();

    sanitized
        .trim_matches(|ch| matches!(ch, ' ' | '.' | '-'))
        .chars()
        .take(80)
        .collect::<String>()
        .if_empty("ok-player")
}

fn time_slug(seconds: f64) -> String {
    let total = seconds.round() as u64;
    let hours = total / 3600;
    let minutes = (total % 3600) / 60;
    let seconds = total % 60;

    if hours > 0 {
        format!("{hours:02}h{minutes:02}m{seconds:02}s")
    } else {
        format!("{minutes:02}m{seconds:02}s")
    }
}

fn unix_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

trait IfEmpty {
    fn if_empty(self, fallback: &str) -> String;
}

impl IfEmpty for String {
    fn if_empty(self, fallback: &str) -> String {
        if self.is_empty() {
            fallback.to_owned()
        } else {
            self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_filename_replaces_path_punctuation_and_trims() {
        assert_eq!(
            sanitize_filename("  Movie: Cut/Scene?.mkv  "),
            "Movie- Cut-Scene-.mkv"
        );
        assert_eq!(sanitize_filename("...---"), "ok-player");
    }

    #[test]
    fn time_slug_formats_media_positions() {
        assert_eq!(time_slug(53.2), "00m53s");
        assert_eq!(time_slug(3222.0), "53m42s");
        assert_eq!(time_slug(3906.0), "01h05m06s");
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
