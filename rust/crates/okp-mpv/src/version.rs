#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BuildTimeMpv {
    pub detected: bool,
    pub pkg_config_version: &'static str,
}

impl BuildTimeMpv {
    pub fn detected() -> Self {
        Self {
            detected: true,
            pkg_config_version: env!("OKP_LINKED_MPV_VERSION"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_time_mpv_marker_is_detected() {
        assert!(BuildTimeMpv::detected().detected);
        assert!(!BuildTimeMpv::detected().pkg_config_version.is_empty());
    }
}
