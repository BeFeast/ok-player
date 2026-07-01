use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppIdentity {
    pub name: String,
    pub platform: String,
}

impl AppIdentity {
    pub fn linux() -> Self {
        Self {
            name: "OK Player".to_owned(),
            platform: "linux".to_owned(),
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
}
