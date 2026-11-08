use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// A platform whose shell runs on okp-core. Per-platform defaults (the shortcut table, the
/// hardware-decoding choice, the app identity) are selected by this value.
///
/// The Windows shell runs its own C# core; its shortcut projection mirrors the
/// [`Platform::Linux`] table, so Linux is also the default for every other target.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    #[default]
    Linux,
    MacOs,
}

impl Platform {
    /// The platform this binary was compiled for: [`Platform::MacOs`] on Apple macOS targets,
    /// [`Platform::Linux`] everywhere else.
    pub const fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::MacOs
        } else {
            Self::Linux
        }
    }

    /// Stable lowercase identifier, the same string serde writes.
    pub const fn id(self) -> &'static str {
        match self {
            Self::Linux => "linux",
            Self::MacOs => "macos",
        }
    }
}

/// Bundle identifier of the macOS app. macOS keys privacy grants (folder access, local
/// network), Launch Services registration and user defaults on it, so it must never change.
pub const MACOS_BUNDLE_ID: &str = "uk.oklabs.ok-player";

/// Name of the app's own directory under the Linux XDG base directories, as the GTK shell
/// already uses it (`$XDG_STATE_HOME/ok-player`, `$XDG_CACHE_HOME/ok-player`).
pub const LINUX_SUPPORT_DIR_NAME: &str = "ok-player";

/// Who the app is on a platform. The bundle id and the support directory name are derived from
/// the platform rather than stored, so a shell that passes `{"name":…,"platform":"macos"}`
/// gets the same directory as core code that calls [`AppIdentity::macos`], and no document can
/// name a second one.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppIdentity {
    pub name: String,
    pub platform: Platform,
}

impl AppIdentity {
    pub fn linux() -> Self {
        Self::for_platform(Platform::Linux)
    }

    pub fn macos() -> Self {
        Self::for_platform(Platform::MacOs)
    }

    pub fn for_platform(platform: Platform) -> Self {
        Self {
            name: "OK Player".to_owned(),
            platform,
        }
    }

    /// The app bundle identifier: [`MACOS_BUNDLE_ID`] on macOS, `None` on Linux, where the GTK
    /// shell owns its application id.
    pub const fn bundle_id(&self) -> Option<&'static str> {
        match self.platform {
            Platform::Linux => None,
            Platform::MacOs => Some(MACOS_BUNDLE_ID),
        }
    }

    /// Name of the app's own directory under each per-user base directory. On macOS it is the
    /// bundle identifier, as Apple's file-system guidelines recommend for Application Support
    /// and Caches, so a rename of the display name keeps the user's history and settings.
    pub const fn support_dir_name(&self) -> &'static str {
        match self.platform {
            Platform::Linux => LINUX_SUPPORT_DIR_NAME,
            Platform::MacOs => MACOS_BUNDLE_ID,
        }
    }

    /// The app's own directory under a per-user base directory the shell resolved:
    /// `~/Library/Application Support` or `~/Library/Caches` on macOS
    /// (`FileManager.urls(for:in:)`), an XDG base directory on Linux. Session code derives its
    /// per-user paths through this, so the directory name is chosen in one place.
    pub fn dir_under(&self, base: &Path) -> PathBuf {
        base.join(self.support_dir_name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_identity_is_stable() {
        let identity = AppIdentity::linux();

        assert_eq!(identity.name, "OK Player");
        assert_eq!(identity.platform, Platform::Linux);
        assert_eq!(identity.bundle_id(), None);
        assert_eq!(identity.support_dir_name(), "ok-player");
    }

    #[test]
    fn linux_identity_serializes_to_its_original_shape() {
        assert_eq!(
            serde_json::to_value(AppIdentity::linux()).expect("identity serializes"),
            serde_json::json!({ "name": "OK Player", "platform": "linux" })
        );
        let parsed: AppIdentity =
            serde_json::from_str(r#"{"name":"OK Player","platform":"linux"}"#)
                .expect("the Linux document parses");
        assert_eq!(parsed, AppIdentity::linux());
    }

    #[test]
    fn macos_identity_carries_the_final_bundle_id_and_support_directory() {
        let identity = AppIdentity::macos();

        assert_eq!(identity.name, "OK Player");
        assert_eq!(identity.platform, Platform::MacOs);
        assert_eq!(identity.bundle_id(), Some("uk.oklabs.ok-player"));
        assert_eq!(identity.support_dir_name(), "uk.oklabs.ok-player");
        assert!(
            !MACOS_BUNDLE_ID.ends_with(".preview"),
            "the preview suffix must not reach the final bundle id"
        );
    }

    #[test]
    fn macos_identity_round_trips_through_serde() {
        let identity = AppIdentity::macos();
        let json = serde_json::to_value(&identity).expect("identity serializes");

        assert_eq!(
            json,
            serde_json::json!({ "name": "OK Player", "platform": "macos" })
        );
        assert_eq!(
            serde_json::from_value::<AppIdentity>(json).expect("identity parses"),
            identity
        );
    }

    #[test]
    fn a_shell_document_resolves_the_same_directories_as_core() {
        // What the Swift shell sends in its session config: only the name and the platform.
        let from_shell: AppIdentity =
            serde_json::from_str(r#"{"name":"OK Player","platform":"macos"}"#)
                .expect("the shell document parses");
        let application_support = Path::new("/Users/example/Library/Application Support");
        let caches = Path::new("/Users/example/Library/Caches");

        assert_eq!(
            from_shell.dir_under(application_support),
            PathBuf::from("/Users/example/Library/Application Support/uk.oklabs.ok-player")
        );
        assert_eq!(
            from_shell.dir_under(caches),
            PathBuf::from("/Users/example/Library/Caches/uk.oklabs.ok-player")
        );
        assert_eq!(
            from_shell.dir_under(application_support),
            AppIdentity::macos().dir_under(application_support)
        );
        assert_eq!(
            AppIdentity::linux().dir_under(Path::new("/home/example/.local/state")),
            PathBuf::from("/home/example/.local/state/ok-player")
        );
    }

    #[test]
    fn an_identity_names_a_known_platform() {
        for platform in ["test", "macOS", "MacOs", ""] {
            let document = format!(r#"{{"name":"OK Player","platform":"{platform}"}}"#);
            assert!(
                serde_json::from_str::<AppIdentity>(&document).is_err(),
                "{platform:?} is not a platform"
            );
        }
    }

    #[test]
    fn identity_platform_matches_the_platform_serde_name() {
        for platform in [Platform::Linux, Platform::MacOs] {
            let identity = AppIdentity::for_platform(platform);
            assert_eq!(
                serde_json::to_value(&identity).expect("identity serializes")["platform"],
                serde_json::Value::String(platform.id().to_owned())
            );
        }
    }

    #[test]
    fn platform_parses_from_its_lowercase_id() {
        assert_eq!(
            serde_json::from_str::<Platform>(r#""macos""#).expect("macos parses"),
            Platform::MacOs
        );
        assert_eq!(
            serde_json::from_str::<Platform>(r#""linux""#).expect("linux parses"),
            Platform::Linux
        );
        assert!(serde_json::from_str::<Platform>(r#""MacOs""#).is_err());
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn current_platform_is_linux_on_linux() {
        assert_eq!(Platform::current(), Platform::Linux);
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn current_platform_is_macos_on_macos() {
        assert_eq!(Platform::current(), Platform::MacOs);
    }
}
