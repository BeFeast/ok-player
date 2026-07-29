//! Portable projection from the Settings window's layout to the record a harness reads.
//!
//! The Settings surface makes two promises that are invisible to a unit test and easy to
//! lose to a stray margin: the page opens at the height it wants inside the available room,
//! and the bottom band - the rule over the rail's About entry, the rule over a page footer,
//! and the controls under them - sits on one grid. Both are rectangles, so the window
//! publishes them on the same `interaction:` stream the player geometry uses (#690) and a
//! screenshot harness checks numbers instead of eyeballing pixels.
//!
//! This module owns the record format. The GTK shell only gathers widget bounds.

use std::fmt::Write as _;

use crate::interaction_geometry::{Plane, Rect};

/// Prefix shared by every emitted Settings geometry line.
pub const SETTINGS_GEOMETRY_PREFIX: &str = "interaction: settings-geometry";

/// Canonical plane names. A harness keys on these, so they are part of the contract.
pub const RAIL_RULE_PLANE: &str = "rail-rule";
pub const CONTENT_RULE_PLANE: &str = "content-rule";
pub const FOOTER_ACTION_PLANE: &str = "footer-action";
pub const FOOTER_LINKS_PLANE: &str = "footer-links";
pub const CONTENT_COLUMN_PLANE: &str = "content-column";

/// One sample of the Settings window: what it is showing, how big it is, how much room it
/// had, and where the surfaces that must line up ended up.
#[derive(Clone, Debug, PartialEq)]
pub struct SettingsGeometry {
    /// Identifier of the visible page, as `SettingsPage::id` spells it.
    pub page: String,
    /// The window's client rectangle, window-local.
    pub client: Rect,
    /// The room the window was allowed to take, in logical pixels.
    pub work_area_height: f64,
    /// How much taller the visible page is than its viewport. Zero means the page opened
    /// whole; anything else means the reader has to scroll to see the rest of it.
    pub content_overflow: f64,
    /// Window-local rectangles of the surfaces under test.
    pub planes: Vec<Plane>,
}

impl SettingsGeometry {
    /// The record a harness reads: one window line, then one line per plane.
    pub fn record(&self, seq: u64) -> Vec<String> {
        let mut lines = Vec::with_capacity(self.planes.len() + 1);
        lines.push(self.window_line(seq));
        for plane in &self.planes {
            lines.push(self.plane_line(plane, seq));
        }
        lines
    }

    /// The named plane, if this sample carries it.
    pub fn plane(&self, name: &str) -> Option<&Plane> {
        self.planes.iter().find(|plane| plane.name == name)
    }

    fn window_line(&self, seq: u64) -> String {
        let mut line = String::new();
        let _ = write!(
            line,
            "{SETTINGS_GEOMETRY_PREFIX} part=window seq={seq} page={} w={} h={} work-area-h={} overflow={}",
            self.page,
            coordinate(self.client.width),
            coordinate(self.client.height),
            coordinate(self.work_area_height),
            coordinate(self.content_overflow),
        );
        line
    }

    fn plane_line(&self, plane: &Plane, seq: u64) -> String {
        let mut line = String::new();
        let center = plane.bounds.center();
        let _ = write!(
            line,
            "{SETTINGS_GEOMETRY_PREFIX} part={} seq={seq} page={} x={} y={} w={} h={} center-y={}",
            plane.name,
            self.page,
            coordinate(plane.bounds.x),
            coordinate(plane.bounds.y),
            coordinate(plane.bounds.width),
            coordinate(plane.bounds.height),
            coordinate(center.y),
        );
        line
    }
}

/// Whole logical pixels. Allocations land on halves under fractional scaling, and a
/// harness comparing two surfaces needs both sides rounded the same way.
fn coordinate(value: f64) -> String {
    format!("{:.1}", value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields(line: &str) -> std::collections::HashMap<String, String> {
        line.split_whitespace()
            .filter_map(|token| token.split_once('='))
            .map(|(key, value)| (key.to_owned(), value.to_owned()))
            .collect()
    }

    /// The About page as it lays out on a 1080p desktop once it opens at its own height.
    fn about() -> SettingsGeometry {
        SettingsGeometry {
            page: "about".to_owned(),
            client: Rect::new(0.0, 0.0, 760.0, 753.0),
            work_area_height: 1032.0,
            content_overflow: 0.0,
            planes: vec![
                Plane::new(RAIL_RULE_PLANE, Rect::new(19.0, 694.0, 153.0, 1.0), false),
                Plane::new(
                    CONTENT_RULE_PLANE,
                    Rect::new(216.0, 694.0, 500.0, 1.0),
                    false,
                ),
                Plane::new(
                    FOOTER_ACTION_PLANE,
                    Rect::new(216.0, 705.0, 168.0, 36.0),
                    true,
                ),
                Plane::new(
                    FOOTER_LINKS_PLANE,
                    Rect::new(554.0, 711.0, 162.0, 24.0),
                    true,
                ),
                Plane::new(
                    CONTENT_COLUMN_PLANE,
                    Rect::new(216.0, 224.0, 500.0, 462.0),
                    false,
                ),
            ],
        }
    }

    #[test]
    fn the_window_line_carries_the_room_the_page_had_and_whether_it_fitted() {
        let record = about().record(3);
        let window = fields(&record[0]);
        assert_eq!(window.get("part").map(String::as_str), Some("window"));
        assert_eq!(window.get("page").map(String::as_str), Some("about"));
        assert_eq!(window.get("h").map(String::as_str), Some("753.0"));
        assert_eq!(
            window.get("work-area-h").map(String::as_str),
            Some("1032.0")
        );
        assert_eq!(window.get("overflow").map(String::as_str), Some("0.0"));
    }

    #[test]
    fn every_plane_reports_its_rectangle_and_its_vertical_center() {
        let geometry = about();
        let record = geometry.record(3);
        let links = record
            .iter()
            .map(|line| fields(line))
            .find(|line| line.get("part").map(String::as_str) == Some(FOOTER_LINKS_PLANE))
            .expect("footer links line");
        assert_eq!(links.get("y").map(String::as_str), Some("711.0"));
        assert_eq!(links.get("h").map(String::as_str), Some("24.0"));
        assert_eq!(links.get("center-y").map(String::as_str), Some("723.0"));

        let action = record
            .iter()
            .map(|line| fields(line))
            .find(|line| line.get("part").map(String::as_str) == Some(FOOTER_ACTION_PLANE))
            .expect("footer action line");
        // Both footer children are centred in the same row slot, so a harness comparing
        // the two centre lines reads one number.
        assert_eq!(action.get("center-y").map(String::as_str), Some("723.0"));
    }

    #[test]
    fn the_two_rules_are_reported_as_separate_planes_a_harness_can_subtract() {
        let geometry = about();
        let rail = geometry.plane(RAIL_RULE_PLANE).expect("rail rule");
        let content = geometry.plane(CONTENT_RULE_PLANE).expect("content rule");
        assert_eq!(rail.bounds.y, content.bounds.y);
        // The rules are insets of different columns, so only their baseline is shared.
        assert_ne!(rail.bounds.x, content.bounds.x);
    }

    #[test]
    fn a_record_names_the_page_on_every_line_so_samples_cannot_be_mixed() {
        let mut geometry = about();
        geometry.page = "appearance".to_owned();
        for line in geometry.record(9) {
            assert!(line.starts_with(SETTINGS_GEOMETRY_PREFIX), "{line}");
            assert!(line.contains(" page=appearance "), "{line}");
            assert!(line.contains(" seq=9 "), "{line}");
        }
    }
}
