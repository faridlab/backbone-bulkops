use serde::{Deserialize, Serialize};
use sqlx::Type;
use std::str::FromStr;
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "bulk_item_status", rename_all = "snake_case")]
pub enum BulkItemStatus {
    Pending,
    Applying,
    Applied,
    Failed,
}

impl std::fmt::Display for BulkItemStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pending => write!(f, "pending"),
            Self::Applying => write!(f, "applying"),
            Self::Applied => write!(f, "applied"),
            Self::Failed => write!(f, "failed"),
        }
    }
}

impl FromStr for BulkItemStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "pending" => Ok(Self::Pending),
            "applying" => Ok(Self::Applying),
            "applied" => Ok(Self::Applied),
            "failed" => Ok(Self::Failed),
            _ => Err(format!("Unknown BulkItemStatus variant: {}", s)),
        }
    }
}

impl Default for BulkItemStatus {
    fn default() -> Self {
        Self::Pending
    }
}
