//! Same-directory subtitle discovery for local playback.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::media_formats;

#[derive(Debug)]
pub struct Sidecar {
    pub path: PathBuf,
    pub title: String,
    pub language: Option<Language>,
}

#[derive(Clone, Copy, Debug)]
pub struct Language {
    alpha3: &'static str,
    bibliographic: &'static str,
    alpha2: &'static str,
    name: &'static str,
}

impl Language {
    fn from_tag(tag: &str) -> Option<Self> {
        // ISO 639-2 Registration Authority (Library of Congress), with French
        // names and the reserved qaa–qtz range omitted.
        // https://www.loc.gov/standards/iso639-2/ISO-639-2_utf-8.txt
        include_str!("subtitle_languages.txt")
            .lines()
            .find_map(|row| {
                let mut fields = row.split('|');
                let [alpha3, bibliographic, alpha2, names] = std::array::from_fn(|_| {
                    fields.next().expect("four fields in each language row")
                });
                let language = Self {
                    alpha3,
                    bibliographic,
                    alpha2,
                    name: names.split("; ").next().expect("registered English name"),
                };
                (language.matches_code(tag)
                    || names.split("; ").any(|name| {
                        name.eq_ignore_ascii_case(tag)
                            || (!tag.is_ascii() && name.to_lowercase() == tag)
                    }))
                .then_some(language)
            })
    }

    fn matches_code(self, code: &str) -> bool {
        !code.is_empty()
            && [self.alpha2, self.alpha3, self.bibliographic]
                .iter()
                .any(|alias| alias.eq_ignore_ascii_case(code))
    }

    /// Match the spelling in mpv's preferred language list if possible. Older
    /// libmpv builds compare the literal codes (e.g. `en` and `eng` differ).
    pub fn code(self, preferred: &str) -> &str {
        preferred
            .split(',')
            .map(str::trim)
            .find(|code| self.matches_code(code))
            .unwrap_or(if self.alpha2.is_empty() {
                self.alpha3
            } else {
                self.alpha2
            })
    }
}

/// Scan the media's own directory. Only a complete stem followed by known tags
/// is eligible. If a more specific stem exists as a media file, it owns the
/// sidecar (e.g. `Film.en.mp4` owns `Film.en.srt` beside `Film.mp4`).
pub fn discover(media_path: &Path) -> Vec<Sidecar> {
    if media_path.to_string_lossy().contains("://") {
        return Vec::new();
    }
    let Some(stem) = media_path.file_stem().and_then(|value| value.to_str()) else {
        return Vec::new();
    };
    let Some(parent) = media_path.parent() else {
        return Vec::new();
    };
    let directory = if parent.as_os_str().is_empty() {
        Path::new(".")
    } else {
        parent
    };
    let Ok(entries) = fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut media_stems = HashSet::new();
    let mut folded_media_stems = HashSet::new();
    let mut index_stems = HashSet::new();
    let mut candidates = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let is_media = media_formats::is_media(&path);
        if !(is_media || is_subtitle(&path))
            || !entry
                .file_type()
                .is_ok_and(|kind| kind.is_file() || (kind.is_symlink() && path.is_file()))
        {
            continue;
        }
        let Some(candidate_stem) = path.file_stem().and_then(|value| value.to_str()) else {
            continue;
        };
        if is_media {
            media_stems.insert(candidate_stem.to_owned());
            folded_media_stems.insert(candidate_stem.to_ascii_lowercase());
        } else if let Some(tags) = stem_suffix(stem, candidate_stem)
            && let Some((title, language)) = metadata(tags)
        {
            if extension_is(&path, "idx") {
                index_stems.insert(candidate_stem.to_owned());
            }
            candidates.push((
                tags.is_empty(),
                candidate_stem.to_owned(),
                Sidecar {
                    path,
                    title,
                    language,
                },
            ));
        }
    }
    candidates.retain(|(_, candidate, sidecar)| {
        let folded = candidate.to_ascii_lowercase();
        let more_specific = folded
            .match_indices('.')
            .map(|(index, _)| &folded[..index])
            .chain([folded.as_str()])
            .any(|prefix| prefix.len() > stem.len() && folded_media_stems.contains(prefix));
        let case_matches_another_stem = !candidate.starts_with(stem)
            && candidate
                .get(..stem.len())
                .is_some_and(|prefix| media_stems.contains(prefix));
        let has_vobsub_index =
            extension_is(&sidecar.path, "sub") && index_stems.contains(candidate);
        !(more_specific || case_matches_another_stem || has_vobsub_index)
    });
    // Keep the untagged track first for libmpv's default tie-break.
    candidates.sort_unstable_by(|a, b| b.0.cmp(&a.0).then_with(|| a.2.path.cmp(&b.2.path)));
    candidates
        .into_iter()
        .map(|(_, _, sidecar)| sidecar)
        .collect()
}

fn is_subtitle(path: &Path) -> bool {
    media_formats::is_subtitle(path)
        || [
            "utf", "utf8", "utf-8", "rt", "mks", "scc", "smi", "lrc", "pgs",
        ]
        .iter()
        .any(|extension| extension_is(path, extension))
}

fn extension_is(path: &Path, expected: &str) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case(expected))
}

fn stem_suffix<'a>(stem: &str, candidate: &'a str) -> Option<&'a str> {
    let prefix = candidate.get(..stem.len())?;
    if !prefix.eq_ignore_ascii_case(stem) {
        return None;
    }
    let suffix = &candidate[stem.len()..];
    if suffix.is_empty() {
        Some(suffix)
    } else if let Some(tags) = suffix.strip_prefix('.') {
        (!tags.is_empty()).then_some(tags)
    } else {
        // Retain the existing single bracketed language suffix.
        suffix
            .strip_prefix('[')
            .and_then(|suffix| suffix.strip_suffix(']'))
            .or_else(|| suffix.strip_prefix('(')?.strip_suffix(')'))
            .filter(|tag| !tag.is_empty() && !tag.contains('.'))
    }
}

fn metadata(tags: &str) -> Option<(String, Option<Language>)> {
    let mut language = None;
    let mut flags = [false; 4];
    let mut hi_count = 0;
    if !tags.is_empty() {
        for tag in tags.split('.') {
            match tag.to_lowercase().as_str() {
                "hi" => hi_count += 1,
                "sdh" | "cc" => flags[0] = true,
                "forced" => flags[1] = true,
                "default" => flags[2] = true,
                "full" => flags[3] = true,
                tag if language.is_none() => language = Some(Language::from_tag(tag)?),
                _ => return None,
            }
        }
    }
    // "hi" is both the ISO code for Hindi and a hearing-impaired flag.
    // With no other language tag, the first "hi" supplies the language.
    if hi_count > 0 && language.is_none() {
        language = Language::from_tag("hi");
        hi_count -= 1;
    }
    flags[0] |= hi_count > 0;
    let mut title = language
        .map_or("Subtitles", |language| language.name)
        .to_owned();
    let labels: Vec<_> = flags
        .into_iter()
        .zip(["SDH", "Forced", "Default", "Full"])
        .filter_map(|(enabled, label)| enabled.then_some(label))
        .collect();
    if !labels.is_empty() {
        title = format!("{title} ({})", labels.join(", "));
    }
    Some((title, language))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MOVIE: &str = "A Serious Man (2009) {tmdb-12573}";

    #[test]
    fn discovers_all_three_subtitles_beside_a_serious_man() {
        let root = tempfile::tempdir().unwrap();
        let media = root.path().join(format!("{MOVIE}.mp4"));
        fs::write(&media, "").unwrap();
        fs::write(root.path().join("movie.nfo"), "").unwrap();
        for suffix in ["en.hi.srt", "he.srt", "ru.hi.srt", "extended.en.srt"] {
            fs::write(root.path().join(format!("{MOVIE}.{suffix}")), "").unwrap();
        }
        let found = discover(&media);
        let titles: Vec<_> = found.iter().map(|sidecar| sidecar.title.as_str()).collect();
        assert_eq!(titles, ["English (SDH)", "Hebrew", "Russian (SDH)"]);
    }

    #[test]
    fn recognizes_stems_tags_and_formats_without_borrowing_siblings() {
        let root = tempfile::tempdir().unwrap();
        let stem = "Film.v2 (2026) [edition] {id-42}";
        let media = root.path().join(format!("{stem}.mp4"));
        let cases = [
            (".srt", "Subtitles", None),
            (
                ".FORCED.ENG.sdh.DEFAULT.full.Ass",
                "English (SDH, Forced, Default, Full)",
                Some("en"),
            ),
            (".english.cc.cc.hi.vtt", "English (SDH)", Some("en")),
            (".hi.rus.srt", "Russian (SDH)", Some("ru")),
            (".hi.sdh.ssa", "Hindi (SDH)", Some("hi")),
            (".fre.vtt", "French", Some("fr")),
            (".ast.sub", "Asturian", Some("ast")),
            (".he.idx", "Hebrew", Some("he")),
            ("[jpn].srt", "Japanese", Some("ja")),
        ];
        for (suffix, _, _) in cases {
            fs::write(root.path().join(format!("{stem}{suffix}")), "").unwrap();
        }
        for ignored in [
            ".he.sub", // VobSub payload is opened through .idx.
            ".de.mp4",
            ".de.srt", // Same stem as the other movie.
            ".de.forced.srt",
            ".eng.rus.srt",
            ".en..srt",
            ".unrecognized.srt",
            "-outtakes.en.srt",
            ".en.srt.bak",
        ] {
            fs::write(root.path().join(format!("{stem}{ignored}")), "").unwrap();
        }
        fs::write(root.path().join("Other film.en.srt"), "").unwrap();
        fs::create_dir(root.path().join(format!("{stem}.it.srt"))).unwrap();
        let found = discover(&media);
        assert_eq!(found.len(), cases.len(), "{found:?}");
        for (suffix, title, code) in cases {
            let path = root.path().join(format!("{stem}{suffix}"));
            let sidecar = found.iter().find(|sidecar| sidecar.path == path).unwrap();
            assert_eq!(sidecar.title, title, "{suffix}");
            assert_eq!(
                sidecar.language.map(|language| language.code("")),
                code,
                "{suffix}"
            );
        }
        assert!(discover(Path::new("https://example.invalid/Film.mp4")).is_empty());
    }

    #[test]
    fn language_code_respects_the_preferred_alias_order() {
        let (_, french) = metadata("french").unwrap();
        assert_eq!(french.unwrap().code("eng, fre, fra"), "fre");
        assert_eq!(french.unwrap().code("fra, fr"), "fra");
    }
}
