use okp_core::AppIdentity;

#[unsafe(no_mangle)]
pub extern "C" fn okp_core_abi_version() -> u32 {
    1
}

pub fn identity_for_tests() -> AppIdentity {
    AppIdentity::linux()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abi_version_starts_at_one() {
        assert_eq!(okp_core_abi_version(), 1);
    }

    #[test]
    fn ffi_crate_can_reach_core() {
        assert_eq!(identity_for_tests().name, "OK Player");
    }
}
