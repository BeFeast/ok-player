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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppIdentity {
    pub name: String,
    pub platform: String,
    /// The app bundle identifier on macOS. `None` on Linux, where the GTK shell owns its
    /// application id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bundle_id: Option<String>,
    /// The app's directory name under the platform support root (`~/Library/Application
    /// Support` on macOS). `None` on Linux, where the shell resolves its XDG directories.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub support_dir_name: Option<String>,
}

impl AppIdentity {
    pub fn linux() -> Self {
        Self {
            name: "OK Player".to_owned(),
            platform: Platform::Linux.id().to_owned(),
            bundle_id: None,
            support_dir_name: None,
        }
    }

    /// The macOS identity. The Application Support directory is named after the bundle
    /// identifier, as Apple's file-system guidelines recommend, so it survives a rename of the
    /// display name.
    pub fn macos() -> Self {
        Self {
            name: "OK Player".to_owned(),
            platform: Platform::MacOs.id().to_owned(),
            bundle_id: Some(MACOS_BUNDLE_ID.to_owned()),
            support_dir_name: Some(MACOS_BUNDLE_ID.to_owned()),
        }
    }

    pub fn for_platform(platform: Platform) -> Self {
        match platform {
            Platform::Linux => Self::linux(),
            Platform::MacOs => Self::macos(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_identity_is_stable() {
        let identity = AppIdentity::linux();

        assert_eq!(identity.name, "OK Player");
        assert_eq!(identity.platform, "linux");
    }

    #[test]
    fn linux_identity_serializes_to_its_original_shape() {
        // The macOS-only fields are skipped when absent, so the Linux document is unchanged.
        assert_eq!(
            serde_json::to_value(AppIdentity::linux()).expect("identity serializes"),
            serde_json::json!({ "name": "OK Player", "platform": "linux" })
        );
        let parsed: AppIdentity =
            serde_json::from_str(r#"{"name":"OK Player","platform":"linux"}"#)
                .expect("a document without the macOS fields still parses");
        assert_eq!(parsed, AppIdentity::linux());
    }

    #[test]
    fn macos_identity_carries_the_final_bundle_id_and_support_directory() {
        let identity = AppIdentity::macos();

        assert_eq!(identity.name, "OK Player");
        assert_eq!(identity.platform, "macos");
        assert_eq!(identity.bundle_id.as_deref(), Some("uk.oklabs.ok-player"));
        assert_eq!(
            identity.support_dir_name.as_deref(),
            Some("uk.oklabs.ok-player")
        );
        assert!(
            !MACOS_BUNDLE_ID.ends_with(".preview"),
            "the preview suffix must not reach the final bundle id"
        );
    }

    #[test]
    fn macos_identity_round_trips_through_serde() {
        let identity = AppIdentity::macos();
        let json = serde_json::to_string(&identity).expect("identity serializes");
        let parsed: AppIdentity = serde_json::from_str(&json).expect("identity parses");

        assert_eq!(parsed, identity);
        assert_eq!(
            serde_json::to_value(&identity).expect("identity serializes"),
            serde_json::json!({
                "name": "OK Player",
                "platform": "macos",
                "bundle_id": "uk.oklabs.ok-player",
                "support_dir_name": "uk.oklabs.ok-player",
            })
        );
    }

    #[test]
    fn identity_platform_matches_the_platform_serde_name() {
        for platform in [Platform::Linux, Platform::MacOs] {
            let identity = AppIdentity::for_platform(platform);
            assert_eq!(
                serde_json::to_value(platform).expect("platform serializes"),
                serde_json::Value::String(identity.platform.clone())
            );
            assert_eq!(identity.platform, platform.id());
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
