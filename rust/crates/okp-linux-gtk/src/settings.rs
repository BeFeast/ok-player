use std::env;
use std::ops::{Deref, DerefMut};
use std::path::PathBuf;

use okp_core::settings_store::SettingsStore as PortableSettingsStore;

/// Supplies the XDG default path for the portable store.
#[derive(Debug)]
pub struct SettingsStore(PortableSettingsStore);

impl Default for SettingsStore {
    fn default() -> Self {
        Self::open_at(settings_path())
    }
}

impl SettingsStore {
    pub fn open_at(path: PathBuf) -> Self {
        Self(PortableSettingsStore::open_at(path))
    }
}

impl Deref for SettingsStore {
    type Target = PortableSettingsStore;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for SettingsStore {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

fn settings_path() -> PathBuf {
    if let Some(config_home) = env::var_os("XDG_CONFIG_HOME").filter(|value| !value.is_empty()) {
        return PathBuf::from(config_home).join("ok-player/settings.json");
    }

    if let Some(home) = env::var_os("HOME").filter(|value| !value.is_empty()) {
        return PathBuf::from(home).join(".config/ok-player/settings.json");
    }

    PathBuf::from("ok-player-settings.json")
}
