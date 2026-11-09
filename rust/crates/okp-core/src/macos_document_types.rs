//! macOS document types for the app bundle's `Info.plist`, generated from
//! [`crate::media_formats`] so Finder offers OK Player for every audio and video file the player
//! opens. The `okp-macos-document-types` binary writes them as a standalone property list
//! holding `CFBundleDocumentTypes` and `UTImportedTypeDeclarations`. The bundle build replaces
//! both keys of `Info.plist` with these values (for example `PlistBuddy -c "Delete
//! :CFBundleDocumentTypes"` before a `Merge`). A plain merge would keep the template's old
//! entries next to the generated ones, so the template should carry neither key.
//!
//! Every extension resolves to one content type:
//!
//! - **System types.** Extensions macOS declares itself, under identifiers Apple publishes as
//!   AVFoundation file types or `UTType` constants (`public.mpeg-4`, `public.mp3`, …), are
//!   claimed by that identifier.
//! - **Imported types.** Every other extension gets an imported declaration conforming to
//!   `public.movie` or `public.audio`. Without one, macOS gives such a file (`.mkv`, `.webm`, …)
//!   a dynamic type that conforms to nothing, so no document type can claim it. Where Mac media
//!   apps share an established identifier (`org.matroska.mkv`), the import uses it so the
//!   declarations agree; otherwise the identifier is scoped to the bundle id. Launch Services
//!   prefers an exported or system declaration of the same identifier over an imported one, so
//!   an import never redefines an existing type.
//!
//! The video document type also claims `public.movie`, and the audio type `public.audio`, so a
//! file whose extension the system or another app declares under a different identifier is
//! still offered through type conformance. Both are viewer roles at rank `Default`.
//!
//! Two more viewer types, at rank `Alternate`, let Finder hand over what the player opens as a
//! playlist without making OK Player the default app for it: `Folder` claims `public.folder`
//! (a folder plays its media) and `Playlist` the type macOS declares for M3U and M3U8 files.
//! Without them a folder or an `.m3u8` dropped on the Dock icon is refused and Open With does
//! not list the app.

use std::fmt::Write as _;

use crate::app::MACOS_BUNDLE_ID;
use crate::media_formats::{AUDIO_EXTENSIONS, VIDEO_EXTENSIONS};

/// Which document type an extension belongs to.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MediaKind {
    Video,
    Audio,
}

impl MediaKind {
    /// `CFBundleTypeName` of the document type.
    pub const fn document_type_name(self) -> &'static str {
        match self {
            Self::Video => "Video",
            Self::Audio => "Audio",
        }
    }

    /// The system supertype every type of this kind conforms to.
    pub const fn umbrella_type(self) -> &'static str {
        match self {
            Self::Video => "public.movie",
            Self::Audio => "public.audio",
        }
    }

    fn noun(self) -> &'static str {
        match self {
            Self::Video => "video",
            Self::Audio => "audio",
        }
    }
}

/// How the bundle makes an extension's content type known to macOS.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Declaration {
    /// macOS declares the type; the bundle only claims it.
    System,
    /// The bundle imports a declaration with this description.
    Imported { description: String },
}

/// The content type one media extension resolves to.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtensionType {
    /// Lowercase extension without the leading dot.
    pub extension: &'static str,
    pub kind: MediaKind,
    pub identifier: String,
    pub declaration: Declaration,
}

/// Extensions macOS declares itself, with the identifier it declares.
const SYSTEM_TYPES: &[(&str, &str)] = &[
    ("mp4", "public.mpeg-4"),
    ("m4v", "com.apple.m4v-video"),
    ("mov", "com.apple.quicktime-movie"),
    ("avi", "public.avi"),
    ("mpg", "public.mpeg"),
    ("mpeg", "public.mpeg"),
    ("3gp", "public.3gpp"),
    ("3g2", "public.3gpp2"),
    ("ts", "public.mpeg-2-transport-stream"),
    ("mp3", "public.mp3"),
    ("m4a", "com.apple.m4a-audio"),
    ("wav", "com.microsoft.waveform-audio"),
    ("aiff", "public.aiff-audio"),
    ("aif", "public.aiff-audio"),
    ("caf", "com.apple.coreaudio-format"),
    ("ac3", "public.ac3-audio"),
    ("amr", "org.3gpp.adaptive-multi-rate-audio"),
];

/// Imports under an identifier other Mac apps already use: extension, identifier, description.
const SHARED_IMPORTS: &[(&str, &str, &str)] = &[
    ("mkv", "org.matroska.mkv", "Matroska video"),
    ("mka", "org.matroska.mka", "Matroska audio"),
    ("webm", "org.webmproject.webm", "WebM video"),
    ("flac", "org.xiph.flac", "FLAC audio"),
    (
        "wmv",
        "com.microsoft.windows-media-wmv",
        "Windows Media video",
    ),
    (
        "wma",
        "com.microsoft.windows-media-wma",
        "Windows Media audio",
    ),
    (
        "asf",
        "com.microsoft.advanced-systems-format",
        "Advanced Systems Format media",
    ),
];

/// The document types beyond the media kinds: name and claimed content types. Both are viewer
/// roles at rank `Alternate`, after the video and audio types.
pub const ALTERNATE_DOCUMENT_TYPES: &[(&str, &[&str])] = &[
    ("Folder", &["public.folder"]),
    ("Playlist", &["public.m3u-playlist"]),
];

/// Every audio and video extension in [`crate::media_formats`], video first, each with its
/// content type.
pub fn extension_types() -> Vec<ExtensionType> {
    let video = VIDEO_EXTENSIONS
        .iter()
        .map(|extension| (MediaKind::Video, *extension));
    let audio = AUDIO_EXTENSIONS
        .iter()
        .map(|extension| (MediaKind::Audio, *extension));
    video
        .chain(audio)
        .map(|(kind, dotted)| extension_type(kind, dotted.trim_start_matches('.')))
        .collect()
}

fn extension_type(kind: MediaKind, extension: &'static str) -> ExtensionType {
    if let Some((_, identifier)) = SYSTEM_TYPES.iter().find(|(known, _)| *known == extension) {
        return ExtensionType {
            extension,
            kind,
            identifier: (*identifier).to_owned(),
            declaration: Declaration::System,
        };
    }
    let (identifier, description) = SHARED_IMPORTS
        .iter()
        .find(|(known, _, _)| *known == extension)
        .map(|(_, identifier, description)| ((*identifier).to_owned(), (*description).to_owned()))
        .unwrap_or_else(|| {
            (
                format!("{MACOS_BUNDLE_ID}.{extension}"),
                format!("{} {}", extension.to_ascii_uppercase(), kind.noun()),
            )
        });
    ExtensionType {
        extension,
        kind,
        identifier,
        declaration: Declaration::Imported { description },
    }
}

/// A standalone XML property list with `CFBundleDocumentTypes` (one video and one audio viewer
/// type at rank `Default`, then [`ALTERNATE_DOCUMENT_TYPES`] at rank `Alternate`) and
/// `UTImportedTypeDeclarations` for every imported type.
pub fn info_plist_fragment() -> String {
    let types = extension_types();
    let mut plist = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
         \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n<dict>\n",
    );

    plist.push_str("\t<key>CFBundleDocumentTypes</key>\n\t<array>\n");
    for kind in [MediaKind::Video, MediaKind::Audio] {
        let mut content_types = vec![kind.umbrella_type().to_owned()];
        for entry in types.iter().filter(|entry| entry.kind == kind) {
            if !content_types.contains(&entry.identifier) {
                content_types.push(entry.identifier.clone());
            }
        }
        push_document_type(
            &mut plist,
            kind.document_type_name(),
            "Default",
            &content_types,
        );
    }
    for (name, content_types) in ALTERNATE_DOCUMENT_TYPES {
        let content_types = content_types
            .iter()
            .map(|identifier| (*identifier).to_owned())
            .collect::<Vec<_>>();
        push_document_type(&mut plist, name, "Alternate", &content_types);
    }
    plist.push_str("\t</array>\n");

    plist.push_str("\t<key>UTImportedTypeDeclarations</key>\n\t<array>\n");
    for entry in &types {
        let Declaration::Imported { description } = &entry.declaration else {
            continue;
        };
        plist.push_str("\t\t<dict>\n");
        push_string(&mut plist, 3, "UTTypeIdentifier", &entry.identifier);
        push_string(&mut plist, 3, "UTTypeDescription", description);
        push_string_array(
            &mut plist,
            3,
            "UTTypeConformsTo",
            &[entry.kind.umbrella_type().to_owned()],
        );
        plist.push_str("\t\t\t<key>UTTypeTagSpecification</key>\n\t\t\t<dict>\n");
        push_string_array(
            &mut plist,
            4,
            "public.filename-extension",
            &[entry.extension.to_owned()],
        );
        plist.push_str("\t\t\t</dict>\n\t\t</dict>\n");
    }
    plist.push_str("\t</array>\n</dict>\n</plist>\n");
    plist
}

/// One viewer entry of `CFBundleDocumentTypes`.
fn push_document_type(plist: &mut String, name: &str, rank: &str, content_types: &[String]) {
    plist.push_str("\t\t<dict>\n");
    push_string(plist, 3, "CFBundleTypeName", name);
    push_string(plist, 3, "CFBundleTypeRole", "Viewer");
    push_string(plist, 3, "LSHandlerRank", rank);
    push_string_array(plist, 3, "LSItemContentTypes", content_types);
    plist.push_str("\t\t</dict>\n");
}

fn push_string(plist: &mut String, depth: usize, key: &str, value: &str) {
    let indent = "\t".repeat(depth);
    let _ = writeln!(
        plist,
        "{indent}<key>{}</key>\n{indent}<string>{}</string>",
        escape(key),
        escape(value)
    );
}

fn push_string_array(plist: &mut String, depth: usize, key: &str, values: &[String]) {
    let indent = "\t".repeat(depth);
    let _ = writeln!(plist, "{indent}<key>{}</key>\n{indent}<array>", escape(key));
    for value in values {
        let _ = writeln!(plist, "{indent}\t<string>{}</string>", escape(value));
    }
    let _ = writeln!(plist, "{indent}</array>");
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use serde_json::{Map, Value};

    use super::*;

    /// Read an XML property list into JSON: dict, array, string and boolean nodes.
    fn read_plist(xml: &str) -> Value {
        // The Apple plist DOCTYPE names an external DTD, which roxmltree refuses by default.
        let options = roxmltree::ParsingOptions {
            allow_dtd: true,
            ..roxmltree::ParsingOptions::default()
        };
        let document = roxmltree::Document::parse_with_options(xml, options)
            .expect("the property list is well-formed");
        let root = document.root_element();
        assert_eq!(root.tag_name().name(), "plist");
        assert_eq!(root.attribute("version"), Some("1.0"));
        let mut children = root.children().filter(roxmltree::Node::is_element);
        let top = children.next().expect("the plist has a top-level value");
        assert!(children.next().is_none(), "one top-level value");
        plist_value(top)
    }

    fn plist_value(node: roxmltree::Node<'_, '_>) -> Value {
        let elements = || node.children().filter(roxmltree::Node::is_element);
        match node.tag_name().name() {
            "dict" => {
                let children = elements().collect::<Vec<_>>();
                assert_eq!(children.len() % 2, 0, "every dict key has a value");
                let mut map = Map::new();
                for pair in children.chunks(2) {
                    assert_eq!(pair[0].tag_name().name(), "key");
                    let key = pair[0].text().unwrap_or_default().to_owned();
                    assert!(
                        map.insert(key.clone(), plist_value(pair[1])).is_none(),
                        "duplicate key {key}"
                    );
                }
                Value::Object(map)
            }
            "array" => Value::Array(elements().map(plist_value).collect()),
            "string" => Value::String(node.text().unwrap_or_default().to_owned()),
            "true" => Value::Bool(true),
            "false" => Value::Bool(false),
            other => panic!("unexpected plist element <{other}>"),
        }
    }

    fn strings(value: &Value) -> Vec<String> {
        value
            .as_array()
            .expect("an array")
            .iter()
            .map(|item| item.as_str().expect("a string").to_owned())
            .collect()
    }

    fn document_types(plist: &Value) -> BTreeMap<String, &Value> {
        plist["CFBundleDocumentTypes"]
            .as_array()
            .expect("document types are an array")
            .iter()
            .map(|entry| {
                (
                    entry["CFBundleTypeName"]
                        .as_str()
                        .expect("a type name")
                        .to_owned(),
                    entry,
                )
            })
            .collect()
    }

    /// Imported declarations by identifier: (conforms-to, filename extensions).
    fn imports(plist: &Value) -> BTreeMap<String, (Vec<String>, Vec<String>)> {
        let mut imports = BTreeMap::new();
        for entry in plist["UTImportedTypeDeclarations"]
            .as_array()
            .expect("imports are an array")
        {
            let identifier = entry["UTTypeIdentifier"]
                .as_str()
                .expect("an identifier")
                .to_owned();
            assert!(
                !entry["UTTypeDescription"]
                    .as_str()
                    .unwrap_or_default()
                    .is_empty(),
                "{identifier} has a description"
            );
            let conforms = strings(&entry["UTTypeConformsTo"]);
            let tags = strings(&entry["UTTypeTagSpecification"]["public.filename-extension"]);
            assert!(
                imports
                    .insert(identifier.clone(), (conforms, tags))
                    .is_none(),
                "{identifier} is imported twice"
            );
        }
        imports
    }

    fn bare(extensions: &[&'static str]) -> Vec<&'static str> {
        extensions
            .iter()
            .map(|extension| extension.trim_start_matches('.'))
            .collect()
    }

    #[test]
    fn every_media_extension_maps_to_a_declared_document_type() {
        let plist = read_plist(&info_plist_fragment());
        let types = document_types(&plist);
        let imports = imports(&plist);
        let system = SYSTEM_TYPES.iter().copied().collect::<BTreeMap<_, _>>();

        for (kind, extensions) in [
            (MediaKind::Video, bare(VIDEO_EXTENSIONS)),
            (MediaKind::Audio, bare(AUDIO_EXTENSIONS)),
        ] {
            let claimed = strings(&types[kind.document_type_name()]["LSItemContentTypes"]);
            for extension in extensions {
                // The extension resolves either to a system type or to exactly one import.
                let identifier = match system.get(extension) {
                    Some(identifier) => {
                        assert!(
                            !imports
                                .values()
                                .any(|(_, tags)| tags.iter().any(|t| t == extension)),
                            ".{extension} is a system type and must not be imported"
                        );
                        (*identifier).to_owned()
                    }
                    None => {
                        let tagged = imports
                            .iter()
                            .filter(|(_, (_, tags))| tags.iter().any(|tag| tag == extension))
                            .collect::<Vec<_>>();
                        assert_eq!(tagged.len(), 1, ".{extension} has one imported declaration");
                        let (identifier, (conforms, _)) = tagged[0];
                        assert_eq!(
                            conforms,
                            &[kind.umbrella_type().to_owned()],
                            ".{extension} conforms to its kind"
                        );
                        identifier.clone()
                    }
                };
                assert!(
                    claimed.contains(&identifier),
                    ".{extension} ({identifier}) is not claimed by the {} document type",
                    kind.document_type_name()
                );
            }
        }
    }

    #[test]
    fn document_types_are_default_rank_viewers_for_video_and_audio() {
        let plist = read_plist(&info_plist_fragment());
        let types = document_types(&plist);

        assert_eq!(
            types.keys().cloned().collect::<Vec<_>>(),
            ["Audio", "Folder", "Playlist", "Video"]
        );
        for (name, umbrella) in [("Video", "public.movie"), ("Audio", "public.audio")] {
            let entry = types[name];
            assert_eq!(entry["CFBundleTypeRole"], "Viewer", "{name}");
            assert_eq!(entry["LSHandlerRank"], "Default", "{name}");
            let claimed = strings(&entry["LSItemContentTypes"]);
            assert_eq!(
                claimed.first().map(String::as_str),
                Some(umbrella),
                "{name}"
            );
            let unique = claimed.iter().collect::<BTreeSet<_>>();
            assert_eq!(unique.len(), claimed.len(), "{name} lists a type twice");
        }
    }

    #[test]
    fn folders_and_m3u_playlists_are_alternate_rank_viewers() {
        let plist = read_plist(&info_plist_fragment());
        let types = document_types(&plist);
        for (name, claimed) in [
            ("Folder", ["public.folder"]),
            ("Playlist", ["public.m3u-playlist"]),
        ] {
            let entry = types[name];
            assert_eq!(entry["CFBundleTypeRole"], "Viewer", "{name}");
            // Offered in Open With and on the Dock icon, never made the default app.
            assert_eq!(entry["LSHandlerRank"], "Alternate", "{name}");
            assert_eq!(strings(&entry["LSItemContentTypes"]), claimed, "{name}");
        }
        // Both are system types: nothing is imported for them.
        let imports = imports(&plist);
        assert!(!imports.contains_key("public.folder"));
        assert!(!imports.contains_key("public.m3u-playlist"));
    }

    #[test]
    fn extensions_without_a_system_type_are_imported() {
        let plist = read_plist(&info_plist_fragment());
        let imports = imports(&plist);
        let tagged = |extension: &str| {
            imports
                .iter()
                .find(|(_, (_, tags))| tags.iter().any(|tag| tag == extension))
                .map(|(identifier, (conforms, _))| (identifier.as_str(), conforms[0].as_str()))
        };

        assert_eq!(tagged("mkv"), Some(("org.matroska.mkv", "public.movie")));
        assert_eq!(
            tagged("webm"),
            Some(("org.webmproject.webm", "public.movie"))
        );
        assert_eq!(tagged("mka"), Some(("org.matroska.mka", "public.audio")));
        assert_eq!(
            tagged("dsf"),
            Some(("uk.oklabs.ok-player.dsf", "public.audio"))
        );
        assert_eq!(tagged("mp4"), None, "mp4 is a system type");
        assert_eq!(tagged("mp3"), None, "mp3 is a system type");
    }

    #[test]
    fn type_tables_name_only_extensions_media_formats_knows() {
        let known = bare(VIDEO_EXTENSIONS)
            .into_iter()
            .chain(bare(AUDIO_EXTENSIONS))
            .collect::<BTreeSet<_>>();
        let tabled = SYSTEM_TYPES
            .iter()
            .map(|(extension, _)| *extension)
            .chain(SHARED_IMPORTS.iter().map(|(extension, _, _)| *extension));
        for extension in tabled {
            assert!(
                known.contains(extension),
                ".{extension} is in a type table but not in media_formats"
            );
        }
    }

    #[test]
    fn extension_types_cover_media_formats_in_order() {
        let types = extension_types();
        let expected = bare(VIDEO_EXTENSIONS)
            .into_iter()
            .map(|extension| (MediaKind::Video, extension))
            .chain(
                bare(AUDIO_EXTENSIONS)
                    .into_iter()
                    .map(|extension| (MediaKind::Audio, extension)),
            )
            .collect::<Vec<_>>();

        assert_eq!(
            types
                .iter()
                .map(|entry| (entry.kind, entry.extension))
                .collect::<Vec<_>>(),
            expected
        );
        assert!(
            !types
                .iter()
                .any(|entry| entry.identifier.starts_with("public.")
                    && entry.declaration != Declaration::System),
            "public.* types belong to Apple and are never imported"
        );
    }
}
