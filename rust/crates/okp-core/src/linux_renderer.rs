//! Pure renderer policy for Linux package environments.
//!
//! The GTK shell observes package and device facts, while the decision and
//! remediation text remain portable and unit-testable.

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum LinuxRendererMode {
    #[default]
    Automatic,
    SoftwareNoDri,
}

impl LinuxRendererMode {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Automatic => "automatic",
            Self::SoftwareNoDri => "software-no-dri",
        }
    }

    pub const fn requires_software_surface(self) -> bool {
        matches!(self, Self::SoftwareNoDri)
    }

    pub const fn mpv_hwdec_override(self) -> Option<&'static str> {
        match self {
            Self::Automatic => None,
            Self::SoftwareNoDri => Some("no"),
        }
    }

    pub const fn environment_overrides(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Self::Automatic => &[],
            Self::SoftwareNoDri => &[("GSK_RENDERER", "cairo")],
        }
    }
}

pub const fn select_linux_renderer(flatpak: bool, dri_accessible: bool) -> LinuxRendererMode {
    if flatpak && !dri_accessible {
        LinuxRendererMode::SoftwareNoDri
    } else {
        LinuxRendererMode::Automatic
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flatpak_without_dri_selects_cpu_software_surface() {
        let mode = select_linux_renderer(true, false);

        assert_eq!(mode, LinuxRendererMode::SoftwareNoDri);
        assert!(mode.requires_software_surface());
        assert_eq!(mode.mpv_hwdec_override(), Some("no"));
        assert_eq!(mode.environment_overrides(), &[("GSK_RENDERER", "cairo")]);
    }

    #[test]
    fn normal_dri_and_non_flatpak_installs_remain_automatic() {
        for (flatpak, dri_accessible) in [(true, true), (false, true), (false, false)] {
            let mode = select_linux_renderer(flatpak, dri_accessible);

            assert_eq!(mode, LinuxRendererMode::Automatic);
            assert!(!mode.requires_software_surface());
            assert_eq!(mode.mpv_hwdec_override(), None);
            assert!(mode.environment_overrides().is_empty());
        }
    }
}
