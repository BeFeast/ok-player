use std::path::{Path, PathBuf};

pub fn write<'a>(paths: impl IntoIterator<Item = &'a str>) -> String {
    let mut text = String::from("#EXTM3U\n");
    for path in paths {
        text.push_str(path);
        text.push('\n');
    }
    text
}

pub fn parse(text: &str, base_dir: Option<&Path>) -> Vec<String> {
    text.split('\n')
        .filter_map(|raw| parse_line(raw, base_dir))
        .collect()
}

fn parse_line(raw: &str, base_dir: Option<&Path>) -> Option<String> {
    let line = raw.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }

    let absolute = Path::new(line).is_absolute() || line.contains("://");
    if absolute || base_dir.is_none() {
        return Some(line.to_owned());
    }

    let combined = base_dir.expect("checked above").join(line);
    Some(full_path(combined).to_string_lossy().into_owned())
}

fn full_path(path: PathBuf) -> PathBuf {
    std::path::absolute(&path).unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_emits_header_then_one_path_per_line() {
        let text = write([r"C:\v\a.mkv", r"C:\v\b.mkv"]);

        assert_eq!(text, "#EXTM3U\nC:\\v\\a.mkv\nC:\\v\\b.mkv\n");
    }

    #[test]
    fn parse_keeps_order_skips_directives_and_blanks() {
        let text = "#EXTM3U\n#EXTINF:123,Title\n\n  C:\\v\\b.mkv  \nC:\\v\\a.mkv\n";
        let entries = parse(text, None);

        assert_eq!(entries, [r"C:\v\b.mkv", r"C:\v\a.mkv"]);
    }

    #[test]
    fn parse_resolves_relative_passes_through_absolute_and_urls() {
        let base_dir = std::env::temp_dir();
        let absolute = if cfg!(windows) {
            r"C:\other\ep2.mkv"
        } else {
            "/other/ep2.mkv"
        };
        let text = format!("ep1.mkv\n{absolute}\nhttps://host/ep3.mp4\n");

        let entries = parse(&text, Some(&base_dir));

        assert_eq!(
            entries[0],
            full_path(base_dir.join("ep1.mkv")).to_string_lossy()
        );
        assert_eq!(entries[1], absolute);
        assert_eq!(entries[2], "https://host/ep3.mp4");
    }

    #[test]
    fn round_trips() {
        let paths = [r"C:\v\1.mkv", r"C:\v\2.mkv", r"C:\v\3.mkv"];
        let back = parse(&write(paths), None);

        assert_eq!(back, paths);
    }
}
