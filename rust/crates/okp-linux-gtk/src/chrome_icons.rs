use super::*;

use gtk::gio;

/// Where the shipped icons live inside the binary. `GtkApplication` derives this
/// same path from the application ID and registers it on the display's icon
/// theme by itself; we still add it explicitly, because the icons have to be
/// registered as a `GResource` first and adding the path afterwards is what
/// makes the theme rescan it.
pub(crate) const CHROME_ICON_RESOURCE_PATH: &str = "/com/befeast/okplayer/icons";

/// The prefix that makes an icon name ours.
///
/// The shell used to ask for the standard freedesktop names, which reads well on
/// paper and does not survive contact with a real desktop: a registered resource
/// path is consulted only *after* every theme in the selected theme's
/// inheritance chain, so a third-party theme that carries `system-search-symbolic`
/// keeps supplying its own magnifier no matter what the application ships. That
/// is what the operator photographed in #731 - `MacTahoe`'s ring-and-stub
/// magnifier in our Settings rail, resolved perfectly, simply not ours.
///
/// A private namespace is the mechanism that does hold: nothing shadows a name
/// no theme defines, so `okp-*` always resolves to the file below, on every
/// desktop, without overriding the user's icon theme for anything else (GTK's
/// own file chooser and portals keep the host's icons).
pub(crate) const CHROME_ICON_PREFIX: &str = "okp-";

/// Every icon the chrome renders. The inventory was assembled from the shell's
/// own call sites and then completed by observation - running the shell against
/// a stripped icon theme under `GTK_DEBUG=icontheme` and reading back what GTK
/// asked for - because widget internals request names that never appear in the
/// source. `edit-clear` (`GtkSearchEntry`'s clear button), `pan-down`
/// (`GtkMenuButton`/`GtkDropDown` arrows) and `process-working` (`GtkSpinner`)
/// were all found that way; grep alone would have missed them.
pub(crate) const CHROME_ICONS: &[&str] = &[
    "okp-audio-volume-high-symbolic",
    "okp-audio-volume-low-symbolic",
    "okp-audio-volume-muted-symbolic",
    "okp-audio-x-generic-symbolic",
    "okp-camera-photo-symbolic",
    "okp-dialog-error-symbolic",
    "okp-dialog-information-symbolic",
    "okp-dialog-warning-symbolic",
    "okp-document-open-recent-symbolic",
    "okp-document-open-symbolic",
    "okp-document-send-symbolic",
    "okp-edit-clear-all-symbolic",
    "okp-edit-clear-symbolic",
    "okp-edit-copy-symbolic",
    "okp-edit-find-symbolic",
    "okp-emblem-system-symbolic",
    "okp-go-down-symbolic",
    "okp-go-next-symbolic",
    "okp-go-previous-symbolic",
    "okp-go-up-symbolic",
    "okp-image-x-generic-symbolic",
    "okp-insert-link-symbolic",
    "okp-list-add-symbolic",
    "okp-list-drag-handle-symbolic",
    "okp-list-remove-symbolic",
    "okp-media-playback-pause-symbolic",
    "okp-media-playback-start-symbolic",
    "okp-media-seek-forward-symbolic",
    "okp-media-skip-backward-symbolic",
    "okp-media-skip-forward-symbolic",
    "okp-media-view-subtitles-symbolic",
    "okp-network-server-symbolic",
    "okp-object-select-symbolic",
    "okp-pan-down-symbolic",
    "okp-process-working-symbolic",
    "okp-system-search-symbolic",
    "okp-user-bookmarks-symbolic",
    "okp-video-x-generic-symbolic",
    "okp-view-fullscreen-symbolic",
    "okp-view-list-symbolic",
    "okp-view-more-symbolic",
    "okp-view-pin-symbolic",
    "okp-view-restore-symbolic",
    "okp-window-close-symbolic",
];

/// The shipped icons that `scripts/generate-chrome-icons.py` still draws, by
/// stem. Everything else in the set is Adwaita artwork under LGPL-3 (see
/// THIRD-PARTY-NOTICES.md), so the generator must not write it.
pub(crate) const GENERATED_CHROME_ICON_STEMS: &[&str] = &["process-working"];

/// Names the chrome still resolves through the host theme and GTK's own builtin
/// icons, each with the reason it is not ours to own.
pub(crate) const THEME_ONLY_ICONS: &[(&str, &str)] = &[
    (
        "image-missing",
        "GTK's last-resort marker for a name that resolved to nothing. Shipping a \
         copy would replace the evidence of a failure with a picture of one.",
    ),
    (
        "com.befeast.okplayer",
        "the application icon. It is desktop integration, installed into hicolor \
         by the packages, and #731 leaves it untouched.",
    ),
];

/// Make the shipped icons resolvable, and make them win.
pub(crate) fn register_chrome_icons() {
    gio::resources_register_include!("okp-chrome-icons.gresource")
        .expect("the shipped chrome icons must register");

    let Some(display) = gdk::Display::default() else {
        return;
    };
    // Unconditionally, even though `GtkApplication` already added this path:
    // adding it is what invalidates the icon theme, and the theme has to be
    // rescanned now that the resource behind the path exists. A repeated path
    // costs one extra directory scan and nothing else.
    let theme = gtk::IconTheme::for_display(&display);
    theme.add_resource_path(CHROME_ICON_RESOURCE_PATH);
    report_chrome_icon_inventory(&theme);
}

/// Say, on the running application's own authority, where each chrome icon came
/// from.
///
/// "Ours wins" is a claim about resolution under whatever icon theme the desktop
/// happens to have selected, so the shell resolves the whole inventory and
/// reports the file behind every name. The GUI gate reads these lines back; a
/// host theme that had taken one of our names would show up as a `file://`
/// source instead of a `resource://` one.
fn report_chrome_icon_inventory(theme: &gtk::IconTheme) {
    if env::var_os("OKP_DEBUG_INTERACTIONS").is_none() {
        return;
    }
    println!(
        "interaction: chrome-icon-theme name={} count={}",
        theme.theme_name(),
        CHROME_ICONS.len()
    );
    for name in CHROME_ICONS {
        let paintable = theme.lookup_icon(
            name,
            &[],
            16,
            1,
            gtk::TextDirection::None,
            gtk::IconLookupFlags::empty(),
        );
        let source = paintable
            .file()
            .map(|file| file.uri().to_string())
            .unwrap_or_else(|| "none".to_owned());
        println!("interaction: chrome-icon name={name} source={source}");
    }
    for (name, _) in THEME_ONLY_ICONS {
        println!("interaction: chrome-icon-theme-only name={name}");
    }
}

/// The shipped name that stands in for a standard freedesktop name.
pub(crate) fn chrome_icon_for(standard: &str) -> Option<&'static str> {
    CHROME_ICONS
        .iter()
        .copied()
        .find(|name| name.strip_prefix(CHROME_ICON_PREFIX) == Some(standard))
}

/// Rewrite the standard icon names GTK's own widget internals set on themselves.
///
/// A widget that builds its own `GtkImage` asks for a standard name from inside
/// GTK, where neither the call site nor the resource path can reach it. The name
/// is reachable on the instance, though, so replace it there.
fn adopt_chrome_icons(widget: &impl IsA<gtk::Widget>) {
    let mut next = widget.as_ref().first_child();
    while let Some(child) = next {
        if let Some(image) = child.downcast_ref::<gtk::Image>()
            && let Some(name) = image.icon_name()
            && let Some(ours) = chrome_icon_for(name.as_str())
        {
            image.set_icon_name(Some(ours));
        }
        adopt_chrome_icons(&child);
        next = child.next_sibling();
    }
}

/// A `GtkSearchEntry` that draws the shipped magnifier and clear button.
///
/// This is the widget from the #731 report. Its two icons are created by GTK
/// from `system-search-symbolic` and `edit-clear-symbolic`, so they appear
/// nowhere in this crate's source and are re-themed on the instance instead -
/// once at construction and again on every map, because a re-theme is cheap and
/// GTK is free to rebuild its own children.
pub(crate) fn chrome_search_entry() -> gtk::SearchEntry {
    let search = gtk::SearchEntry::new();
    adopt_chrome_icons(&search);
    search.connect_map(adopt_chrome_icons);
    search
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};

    fn crate_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    fn icon_dir() -> PathBuf {
        crate_dir().join("icons")
    }

    fn shipped_files() -> BTreeSet<String> {
        gio::resources_register_include!("okp-chrome-icons.gresource")
            .expect("the shipped chrome icons must register");
        gio::functions::resources_enumerate_children(
            "/com/befeast/okplayer/icons/scalable/actions/",
            gio::ResourceLookupFlags::NONE,
        )
        .expect("the shipped chrome icons must be enumerable")
        .into_iter()
        .map(|name| name.to_string())
        .collect()
    }

    /// The inventory check: every name the table claims is shipped is in the
    /// binary. Delete one SVG under `icons/` and this is the test that fails,
    /// naming it.
    #[test]
    fn every_named_chrome_icon_is_shipped_in_the_binary() {
        let shipped = shipped_files();
        let missing: Vec<&str> = CHROME_ICONS
            .iter()
            .copied()
            .filter(|name| !shipped.contains(&format!("{name}.svg")))
            .collect();
        assert!(
            missing.is_empty(),
            "chrome icons named but not shipped: {missing:?}"
        );
    }

    /// And nothing rides along unnamed, so the table stays the inventory rather
    /// than a subset of one.
    #[test]
    fn nothing_ships_that_the_inventory_does_not_name() {
        let named: BTreeSet<String> = CHROME_ICONS
            .iter()
            .map(|name| format!("{name}.svg"))
            .collect();
        let extra: Vec<String> = shipped_files().difference(&named).cloned().collect();
        assert!(
            extra.is_empty(),
            "icons shipped but not in the inventory: {extra:?}"
        );
    }

    #[test]
    fn the_inventory_is_sorted_and_unique() {
        let mut sorted = CHROME_ICONS.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.as_slice(), CHROME_ICONS);
    }

    #[test]
    fn every_shipped_name_carries_the_prefix_and_the_symbolic_suffix() {
        for name in CHROME_ICONS {
            assert!(
                name.starts_with(CHROME_ICON_PREFIX) && name.ends_with("-symbolic"),
                "{name} is not a shipped chrome icon name"
            );
        }
    }

    fn shell_sources() -> Vec<(String, String)> {
        // Read the directory rather than a hand-kept list, so a module added
        // later cannot escape the scan below.
        let mut sources = Vec::new();
        for entry in std::fs::read_dir(crate_dir().join("src"))
            .expect("the crate's own sources must be readable")
        {
            let path = entry.expect("reading the source directory failed").path();
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .to_owned();
            // `tests.rs` quotes icon names as assertion data, including names
            // the shell deliberately does not use.
            if !name.ends_with(".rs") || name == "tests.rs" {
                continue;
            }
            sources.push((
                name,
                std::fs::read_to_string(&path).expect("a source file must be readable"),
            ));
        }
        assert!(sources.len() > 20, "the source scan found almost nothing");
        sources
    }

    fn is_icon_name(name: &str) -> bool {
        name.len() > "-symbolic".len()
            && name.starts_with(|first: char| first.is_ascii_lowercase())
            && name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '.')
    }

    fn quoted_icon_names(source: &str) -> BTreeSet<String> {
        let mut names = BTreeSet::new();
        for quote in ['"', '\''] {
            let closing = format!("-symbolic{quote}");
            for (index, _) in source.match_indices(&closing) {
                let end = index + "-symbolic".len();
                let Some(start) = source[..index].rfind(quote) else {
                    continue;
                };
                let name = &source[start + 1..end];
                if is_icon_name(name) {
                    names.insert(name.to_owned());
                }
            }
        }
        names
    }

    /// The other half of the inventory check: an icon name written into the
    /// shell has to be one we ship, or be listed as the host's on purpose.
    #[test]
    fn the_shell_asks_only_for_icons_it_ships() {
        let allowlisted: BTreeSet<&str> = THEME_ONLY_ICONS.iter().map(|(name, _)| *name).collect();
        let shipped: BTreeSet<&str> = CHROME_ICONS.iter().copied().collect();
        let mut offenders = Vec::new();
        for (file, source) in shell_sources() {
            for name in quoted_icon_names(&source) {
                if shipped.contains(name.as_str()) || allowlisted.contains(name.as_str()) {
                    continue;
                }
                offenders.push(format!("{file}: {name}"));
            }
        }
        assert!(
            offenders.is_empty(),
            "icon names that are neither shipped nor allowlisted: {offenders:?}"
        );
    }

    /// The CSS-driven nodes are the third way an icon gets requested, and the
    /// only way to take one is by name in the stylesheet. Read the names out of
    /// the stylesheet the shell actually installs, and resolve every one of them
    /// against what the binary ships.
    #[test]
    fn css_driven_icon_nodes_name_the_shipped_icons() {
        let mut named = BTreeSet::new();
        for (index, _) in OKP_STYLESHEET.match_indices("-gtk-icontheme('") {
            let start = index + "-gtk-icontheme('".len();
            let rest = &OKP_STYLESHEET[start..];
            let end = rest.find('\'').expect("a CSS icon name must be closed");
            named.insert(rest[..end].to_owned());
        }
        let shipped = shipped_files();
        for name in &named {
            assert!(
                shipped.contains(&format!("{name}.svg")),
                "the stylesheet points a node at {name}, which the binary does not ship"
            );
        }
        assert_eq!(
            named.len(),
            2,
            "the stylesheet's CSS-driven icon nodes changed: {named:?}"
        );
    }

    fn icon_files() -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = std::fs::read_dir(icon_dir())
            .expect("the shipped icon directory must be readable")
            .map(|entry| entry.expect("reading the icon directory failed").path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "svg"))
            .collect();
        files.sort();
        files
    }

    /// #731 rejected text glyphs outright: a glyph renders only if the matched
    /// font carries the codepoint, which is the same class of failure by another
    /// route.
    #[test]
    fn no_shipped_icon_draws_text() {
        for path in icon_files() {
            let svg = std::fs::read_to_string(&path).expect("an icon must be readable");
            for forbidden in ["<text", "<tspan", "font-family"] {
                assert!(
                    !svg.contains(forbidden),
                    "{} draws {forbidden}",
                    path.display()
                );
            }
        }
    }

    /// GTK's symbolic recolour rewrites `fill` and leaves `stroke` alone, so a
    /// stroked icon would keep its authored colour and stop following the
    /// foreground.
    #[test]
    fn shipped_icons_are_fills_only() {
        for path in icon_files() {
            let svg = std::fs::read_to_string(&path).expect("an icon must be readable");
            assert!(
                !svg.contains("stroke"),
                "{} uses a stroke, which GTK will not recolour",
                path.display()
            );
        }
    }

    /// Symbolic recolour keys off the file name, and the 16px grid is what the
    /// whole set is drawn on.
    #[test]
    fn shipped_icons_declare_the_symbolic_grid() {
        for path in icon_files() {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            assert!(
                name.starts_with(CHROME_ICON_PREFIX) && name.ends_with("-symbolic.svg"),
                "{name} is not named as a symbolic icon"
            );
            let svg = std::fs::read_to_string(&path).expect("an icon must be readable");
            // The grid is the rule, not the literal string: upstream artwork can
            // carry a rounded extent (`0 0 16 15.980469`) from the tool that drew
            // it, which is the same 16px grid and renders identically.
            let box_attr = svg
                .split_once("viewBox=\"")
                .and_then(|(_, rest)| rest.split_once('"'))
                .map(|(value, _)| value.to_string())
                .unwrap_or_default();
            let extent: Vec<f64> = box_attr
                .split_whitespace()
                .filter_map(|value| value.parse::<f64>().ok())
                .collect();
            assert!(
                extent.len() == 4
                    && extent[0] == 0.0
                    && extent[1] == 0.0
                    && (extent[2] - 16.0).abs() < 0.05
                    && (extent[3] - 16.0).abs() < 0.05,
                "{name} is not drawn on the 16px grid: viewBox=\"{box_attr}\""
            );
        }
    }

    #[test]
    fn theme_only_icons_each_carry_a_reason() {
        for (name, reason) in THEME_ONLY_ICONS {
            assert!(!name.is_empty(), "a theme-only entry has no name");
            assert!(reason.len() > 40, "{name} is allowlisted without a reason");
        }
    }

    /// The generator used to own the whole chrome set. Now that the set is
    /// Adwaita artwork apart from one icon, running it must no longer write the
    /// rest: it would overwrite LGPL-3 files with GPL-3-headered script drawings
    /// and silently contradict THIRD-PARTY-NOTICES.md. The old assertion here
    /// required the opposite - that it draws every shipped name - so it is
    /// replaced rather than relaxed.
    #[test]
    fn the_generator_writes_only_the_still_first_party_icon() {
        let generator = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../scripts/generate-chrome-icons.py"),
        )
        .expect("the icon generator must be readable");

        for stem in GENERATED_CHROME_ICON_STEMS {
            assert!(
                generator.contains(&format!("\"{stem}\":")),
                "the generator does not draw {stem}"
            );
            assert!(
                generator.contains(&format!("GENERATED = (\"{stem}\",)")),
                "the generator does not declare {stem} as written"
            );
        }

        assert!(
            generator.contains("for name in sorted(GENERATED):"),
            "the write loop must iterate the written set"
        );
        assert!(
            !generator.contains("for name, body in sorted(ICONS.items()):"),
            "the write loop must not iterate the whole retired set"
        );
    }

    #[test]
    fn every_generated_icon_stem_is_actually_shipped() {
        for stem in GENERATED_CHROME_ICON_STEMS {
            let name = format!("{CHROME_ICON_PREFIX}{stem}-symbolic");
            assert!(
                CHROME_ICONS.contains(&name.as_str()),
                "{name} is generated but not shipped"
            );
        }
    }
}
