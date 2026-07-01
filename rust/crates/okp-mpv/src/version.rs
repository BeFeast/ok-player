#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BuildTimeMpv {
    pub detected: bool,
}

impl BuildTimeMpv {
    pub fn detected() -> Self {
        Self { detected: true }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_time_mpv_marker_is_detected() {
        assert!(BuildTimeMpv::detected().detected);
    }
}
