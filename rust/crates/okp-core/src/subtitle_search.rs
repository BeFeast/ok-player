//! Search and cue navigation primitives for timed subtitle text. The shell supplies subtitle
//! file text and dispatches seeks; parsing, text matching, and adjacent-cue selection stay here.

use std::path::Path;

use crate::{lrc, srt};

#[derive(Debug, Clone, PartialEq)]
pub struct SubtitleCue {
    pub start_seconds: f64,
    pub end_seconds: Option<f64>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct SubtitleCueIndex {
    cues: Vec<SubtitleCue>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SubtitleSearchMatch {
    pub cue_index: usize,
    pub start_seconds: f64,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubtitleCueDirection {
    Previous,
    Next,
}

impl SubtitleCueIndex {
    pub fn from_srt_text(text: Option<&str>) -> Self {
        Self::new(
            srt::parse(text)
                .into_iter()
                .map(|cue| SubtitleCue {
                    start_seconds: cue.start_seconds,
                    end_seconds: Some(cue.end_seconds),
                    text: cue.text,
                })
                .collect(),
        )
    }

    pub fn from_lrc_text(text: Option<&str>) -> Self {
        let document = lrc::parse(text);
        if !document.has_timings {
            return Self::default();
        }

        Self::new(
            document
                .lines
                .into_iter()
                .filter(|line| !line.text.trim().is_empty())
                .map(|line| SubtitleCue {
                    start_seconds: line.time_seconds,
                    end_seconds: None,
                    text: line.text,
                })
                .collect(),
        )
    }

    pub fn from_path_text(path: &Path, text: Option<&str>) -> Option<Self> {
        match path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("srt") => Some(Self::from_srt_text(text)),
            Some("lrc") => Some(Self::from_lrc_text(text)),
            _ => None,
        }
    }

    pub fn new(mut cues: Vec<SubtitleCue>) -> Self {
        cues.retain(|cue| cue.start_seconds.is_finite() && cue.start_seconds >= 0.0);
        cues.sort_by(|left, right| left.start_seconds.total_cmp(&right.start_seconds));
        Self { cues }
    }

    pub fn is_empty(&self) -> bool {
        self.cues.is_empty()
    }

    pub fn len(&self) -> usize {
        self.cues.len()
    }

    pub fn search(&self, query: &str, limit: usize) -> Vec<SubtitleSearchMatch> {
        let query = normalize_query(query);
        if query.is_empty() || limit == 0 {
            return Vec::new();
        }

        self.cues
            .iter()
            .enumerate()
            .filter(|(_, cue)| normalize_query(&cue.text).contains(&query))
            .take(limit)
            .map(|(cue_index, cue)| SubtitleSearchMatch {
                cue_index,
                start_seconds: cue.start_seconds,
                text: cue.text.clone(),
            })
            .collect()
    }

    pub fn adjacent_start(
        &self,
        position_seconds: f64,
        direction: SubtitleCueDirection,
    ) -> Option<f64> {
        if self.cues.is_empty() || !position_seconds.is_finite() {
            return None;
        }

        match direction {
            SubtitleCueDirection::Previous => {
                let before = position_seconds - 0.05;
                self.cues
                    .iter()
                    .rev()
                    .find(|cue| cue.start_seconds < before)
                    .map(|cue| cue.start_seconds)
            }
            SubtitleCueDirection::Next => self
                .cues
                .iter()
                .find(|cue| cue.start_seconds > position_seconds + 0.05)
                .map(|cue| cue.start_seconds),
        }
    }
}

pub fn is_supported_subtitle_path(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("srt" | "lrc")
    )
}

fn normalize_query(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_seconds(actual: Option<f64>, expected: Option<f64>) {
        match (actual, expected) {
            (Some(actual), Some(expected)) => {
                okp_test_fixtures::assert_close(actual, expected, 1e-9)
            }
            _ => assert_eq!(actual, expected),
        }
    }

    #[test]
    fn srt_search_indexes_text_case_insensitively() {
        let index = SubtitleCueIndex::from_srt_text(Some(
            "1\n00:00:01,000 --> 00:00:02,000\nHello <i>world</i>\n\n\
             2\n00:00:04,500 --> 00:00:05,000\nAnother line",
        ));

        assert_eq!(index.len(), 2);
        assert_eq!(
            index.search("hello world", 8),
            vec![SubtitleSearchMatch {
                cue_index: 0,
                start_seconds: 1.0,
                text: "Hello world".to_owned(),
            }]
        );
        assert!(index.search("", 8).is_empty());
    }

    #[test]
    fn lrc_search_uses_only_timed_non_empty_lines() {
        let index = SubtitleCueIndex::from_lrc_text(Some(
            "[ti:Example]\n[00:01.00] First line\n[00:02.00]\n[00:03.50] Final chorus",
        ));

        assert_eq!(index.len(), 2);
        assert_eq!(index.search("CHORUS", 1)[0].start_seconds, 3.5);
    }

    #[test]
    fn adjacent_start_selects_previous_and_next_cue_starts() {
        let index = SubtitleCueIndex::new(vec![
            SubtitleCue {
                start_seconds: 1.0,
                end_seconds: Some(2.0),
                text: "one".to_owned(),
            },
            SubtitleCue {
                start_seconds: 4.0,
                end_seconds: Some(5.0),
                text: "two".to_owned(),
            },
            SubtitleCue {
                start_seconds: 9.0,
                end_seconds: Some(10.0),
                text: "three".to_owned(),
            },
        ]);

        assert_seconds(
            index.adjacent_start(4.02, SubtitleCueDirection::Previous),
            Some(1.0),
        );
        assert_seconds(
            index.adjacent_start(4.2, SubtitleCueDirection::Next),
            Some(9.0),
        );
        assert_seconds(
            index.adjacent_start(0.0, SubtitleCueDirection::Previous),
            None,
        );
        assert_seconds(index.adjacent_start(9.5, SubtitleCueDirection::Next), None);
    }

    #[test]
    fn empty_or_unsupported_tracks_degrade_cleanly() {
        let empty = SubtitleCueIndex::from_srt_text(None);

        assert!(empty.is_empty());
        assert!(empty.search("anything", 8).is_empty());
        assert_eq!(empty.adjacent_start(12.0, SubtitleCueDirection::Next), None);
        assert!(SubtitleCueIndex::from_path_text(Path::new("movie.ass"), Some("x")).is_none());
    }
}
