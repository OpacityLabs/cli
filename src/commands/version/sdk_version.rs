use serde::{Deserialize, Serialize};

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct SdkVersionOut {
    pub min_sdk_version: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_sdk_version: Option<u64>,
}

impl PartialEq for SdkVersionOut {
    fn eq(&self, other: &Self) -> bool {
        self.min_sdk_version == other.min_sdk_version
            && self.max_sdk_version == other.max_sdk_version
    }
}

impl Eq for SdkVersionOut {}

impl SdkVersionOut {
    pub fn new(default_version: u64) -> Self {
        Self {
            min_sdk_version: default_version,
            max_sdk_version: None,
        }
    }

    pub fn sdk_version_intersection(lhs: SdkVersionOut, rhs: SdkVersionOut) -> SdkVersionOut {
        SdkVersionOut {
            min_sdk_version: lhs.min_sdk_version.max(rhs.min_sdk_version),
            max_sdk_version: Self::sdk_version_minimum_of_max(
                lhs.max_sdk_version,
                rhs.max_sdk_version,
            ),
        }
    }

    pub fn sdk_version_minimum_of_max(lhs: Option<u64>, rhs: Option<u64>) -> Option<u64> {
        match (lhs, rhs) {
            (Some(lhs), Some(rhs)) => Some(lhs.min(rhs)),
            (Some(lhs), None) => Some(lhs),
            (None, Some(rhs)) => Some(rhs),
            (None, None) => None,
        }
    }

    pub fn sdk_version_union(lhs: SdkVersionOut, rhs: SdkVersionOut) -> SdkVersionOut {
        SdkVersionOut {
            min_sdk_version: lhs.min_sdk_version.min(rhs.min_sdk_version),
            max_sdk_version: match (lhs.max_sdk_version, rhs.max_sdk_version) {
                (Some(lhs), Some(rhs)) => Some(lhs.max(rhs)),
                _ => None,
            },
        }
    }
}
