use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use super::BulkItemStatus;
use super::AuditMetadata;

/// Strongly-typed ID for BulkJobItem
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BulkJobItemId(pub Uuid);

impl BulkJobItemId {
    pub fn new(id: Uuid) -> Self { Self(id) }
    pub fn generate() -> Self { Self(Uuid::new_v4()) }
    pub fn into_inner(self) -> Uuid { self.0 }
}

impl std::fmt::Display for BulkJobItemId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for BulkJobItemId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

impl From<Uuid> for BulkJobItemId {
    fn from(id: Uuid) -> Self { Self(id) }
}

impl From<BulkJobItemId> for Uuid {
    fn from(id: BulkJobItemId) -> Self { id.0 }
}

impl AsRef<Uuid> for BulkJobItemId {
    fn as_ref(&self) -> &Uuid { &self.0 }
}

impl std::ops::Deref for BulkJobItemId {
    type Target = Uuid;
    fn deref(&self) -> &Self::Target { &self.0 }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct BulkJobItem {
    pub id: Uuid,
    pub job_id: Uuid,
    pub item_key: String,
    pub status: BulkItemStatus,
    pub payload: String,
    pub applied_ref_type: Option<String>,
    pub applied_ref_id: Option<Uuid>,
    pub error_detail: Option<String>,
    #[serde(default)]
    #[sqlx(json)]
    pub metadata: AuditMetadata,
}

impl BulkJobItem {
    /// Create a builder for BulkJobItem
    pub fn builder() -> BulkJobItemBuilder {
        BulkJobItemBuilder::default()
    }

    /// Create a new BulkJobItem with required fields
    pub fn new(job_id: Uuid, item_key: String, status: BulkItemStatus, payload: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            job_id,
            item_key,
            status,
            payload,
            applied_ref_type: None,
            applied_ref_id: None,
            error_detail: None,
            metadata: AuditMetadata::default(),
        }
    }

    /// Get the entity's unique identifier
    pub fn id(&self) -> &Uuid {
        &self.id
    }

    /// Get a strongly-typed ID for this entity
    pub fn typed_id(&self) -> BulkJobItemId {
        BulkJobItemId(self.id)
    }

    /// Get when this entity was created
    pub fn created_at(&self) -> Option<&DateTime<Utc>> {
        self.metadata.created_at.as_ref()
    }

    /// Get when this entity was last updated
    pub fn updated_at(&self) -> Option<&DateTime<Utc>> {
        self.metadata.updated_at.as_ref()
    }

    /// Check if this entity is soft deleted
    pub fn is_deleted(&self) -> bool {
        self.metadata.deleted_at.is_some()
    }

    /// Check if this entity is active (not deleted)
    pub fn is_active(&self) -> bool {
        self.metadata.deleted_at.is_none()
    }

    /// Get when this entity was deleted
    pub fn deleted_at(&self) -> Option<&DateTime<Utc>> {
        self.metadata.deleted_at.as_ref()
    }

    /// Get who created this entity
    pub fn created_by(&self) -> Option<&Uuid> {
        self.metadata.created_by.as_ref()
    }

    /// Get who last updated this entity
    pub fn updated_by(&self) -> Option<&Uuid> {
        self.metadata.updated_by.as_ref()
    }

    /// Get who deleted this entity
    pub fn deleted_by(&self) -> Option<&Uuid> {
        self.metadata.deleted_by.as_ref()
    }

    /// Get the current status
    pub fn status(&self) -> &BulkItemStatus {
        &self.status
    }


    // ==========================================================
    // Fluent Setters (with_* for optional fields)
    // ==========================================================

    /// Set the applied_ref_type field (chainable)
    pub fn with_applied_ref_type(mut self, value: String) -> Self {
        self.applied_ref_type = Some(value);
        self
    }

    /// Set the applied_ref_id field (chainable)
    pub fn with_applied_ref_id(mut self, value: Uuid) -> Self {
        self.applied_ref_id = Some(value);
        self
    }

    /// Set the error_detail field (chainable)
    pub fn with_error_detail(mut self, value: String) -> Self {
        self.error_detail = Some(value);
        self
    }

    // ==========================================================
    // Partial Update
    // ==========================================================

    /// Apply partial updates from a map of field name to JSON value
    pub fn apply_patch(&mut self, fields: std::collections::HashMap<String, serde_json::Value>) {
        for (key, value) in fields {
            match key.as_str() {
                "job_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.job_id = v; }
                }
                "item_key" => {
                    if let Ok(v) = serde_json::from_value(value) { self.item_key = v; }
                }
                "status" => {
                    if let Ok(v) = serde_json::from_value(value) { self.status = v; }
                }
                "payload" => {
                    if let Ok(v) = serde_json::from_value(value) { self.payload = v; }
                }
                "applied_ref_type" => {
                    if let Ok(v) = serde_json::from_value(value) { self.applied_ref_type = v; }
                }
                "applied_ref_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.applied_ref_id = v; }
                }
                "error_detail" => {
                    if let Ok(v) = serde_json::from_value(value) { self.error_detail = v; }
                }
                _ => {} // ignore unknown fields
            }
        }
    }

    // <<< CUSTOM METHODS START >>>
    // <<< CUSTOM METHODS END >>>
}

impl super::Entity for BulkJobItem {
    type Id = Uuid;

    fn entity_id(&self) -> &Self::Id {
        &self.id
    }

    fn entity_type() -> &'static str {
        "BulkJobItem"
    }
}

impl backbone_core::PersistentEntity for BulkJobItem {
    fn entity_id(&self) -> String {
        self.id.to_string()
    }
    fn set_entity_id(&mut self, id: String) {
        if let Ok(uuid) = uuid::Uuid::parse_str(&id) {
            self.id = uuid;
        }
    }
    fn created_at(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        self.metadata.created_at
    }
    fn set_created_at(&mut self, ts: chrono::DateTime<chrono::Utc>) {
        self.metadata.created_at = Some(ts);
    }
    fn updated_at(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        self.metadata.updated_at
    }
    fn set_updated_at(&mut self, ts: chrono::DateTime<chrono::Utc>) {
        self.metadata.updated_at = Some(ts);
    }
    fn deleted_at(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        self.metadata.deleted_at
    }
    fn set_deleted_at(&mut self, ts: Option<chrono::DateTime<chrono::Utc>>) {
        self.metadata.deleted_at = ts;
    }
}

impl backbone_orm::EntityRepoMeta for BulkJobItem {
    fn column_types() -> std::collections::HashMap<String, String> {
        let mut m = std::collections::HashMap::new();
        m.insert("id".to_string(), "uuid".to_string());
        m.insert("job_id".to_string(), "uuid".to_string());
        m.insert("applied_ref_id".to_string(), "uuid".to_string());
        m.insert("status".to_string(), "bulk_item_status".to_string());
        m
    }
    fn search_fields() -> &'static [&'static str] {
        &["item_key", "payload"]
    }
}

/// Builder for BulkJobItem entity
///
/// Provides a fluent API for constructing BulkJobItem instances.
/// System fields (id, metadata, timestamps) are auto-initialized.
#[derive(Debug, Clone, Default)]
pub struct BulkJobItemBuilder {
    job_id: Option<Uuid>,
    item_key: Option<String>,
    status: Option<BulkItemStatus>,
    payload: Option<String>,
    applied_ref_type: Option<String>,
    applied_ref_id: Option<Uuid>,
    error_detail: Option<String>,
}

impl BulkJobItemBuilder {
    /// Set the job_id field (required)
    pub fn job_id(mut self, value: Uuid) -> Self {
        self.job_id = Some(value);
        self
    }

    /// Set the item_key field (required)
    pub fn item_key(mut self, value: String) -> Self {
        self.item_key = Some(value);
        self
    }

    /// Set the status field (default: `BulkItemStatus::default()`)
    pub fn status(mut self, value: BulkItemStatus) -> Self {
        self.status = Some(value);
        self
    }

    /// Set the payload field (required)
    pub fn payload(mut self, value: String) -> Self {
        self.payload = Some(value);
        self
    }

    /// Set the applied_ref_type field (optional)
    pub fn applied_ref_type(mut self, value: String) -> Self {
        self.applied_ref_type = Some(value);
        self
    }

    /// Set the applied_ref_id field (optional)
    pub fn applied_ref_id(mut self, value: Uuid) -> Self {
        self.applied_ref_id = Some(value);
        self
    }

    /// Set the error_detail field (optional)
    pub fn error_detail(mut self, value: String) -> Self {
        self.error_detail = Some(value);
        self
    }

    /// Build the BulkJobItem entity
    ///
    /// Returns Err if any required field without a default is missing.
    pub fn build(self) -> Result<BulkJobItem, String> {
        let job_id = self.job_id.ok_or_else(|| "job_id is required".to_string())?;
        let item_key = self.item_key.ok_or_else(|| "item_key is required".to_string())?;
        let payload = self.payload.ok_or_else(|| "payload is required".to_string())?;

        Ok(BulkJobItem {
            id: Uuid::new_v4(),
            job_id,
            item_key,
            status: self.status.unwrap_or(BulkItemStatus::default()),
            payload,
            applied_ref_type: self.applied_ref_type,
            applied_ref_id: self.applied_ref_id,
            error_detail: self.error_detail,
            metadata: AuditMetadata::default(),
        })
    }
}
