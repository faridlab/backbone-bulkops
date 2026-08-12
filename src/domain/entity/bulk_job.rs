use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use super::BulkJobStatus;
use super::AuditMetadata;

/// Strongly-typed ID for BulkJob
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BulkJobId(pub Uuid);

impl BulkJobId {
    pub fn new(id: Uuid) -> Self { Self(id) }
    pub fn generate() -> Self { Self(Uuid::new_v4()) }
    pub fn into_inner(self) -> Uuid { self.0 }
}

impl std::fmt::Display for BulkJobId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for BulkJobId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

impl From<Uuid> for BulkJobId {
    fn from(id: Uuid) -> Self { Self(id) }
}

impl From<BulkJobId> for Uuid {
    fn from(id: BulkJobId) -> Self { id.0 }
}

impl AsRef<Uuid> for BulkJobId {
    fn as_ref(&self) -> &Uuid { &self.0 }
}

impl std::ops::Deref for BulkJobId {
    type Target = Uuid;
    fn deref(&self) -> &Self::Target { &self.0 }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct BulkJob {
    pub id: Uuid,
    pub company_id: Uuid,
    pub operation_type: String,
    pub target_module: String,
    pub status: BulkJobStatus,
    pub total_items: i32,
    pub succeeded_count: i32,
    pub failed_count: i32,
    pub submitted_by: Option<Uuid>,
    #[serde(default)]
    #[sqlx(json)]
    pub metadata: AuditMetadata,
}

impl BulkJob {
    /// Create a builder for BulkJob
    pub fn builder() -> BulkJobBuilder {
        BulkJobBuilder::default()
    }

    /// Create a new BulkJob with required fields
    pub fn new(company_id: Uuid, operation_type: String, target_module: String, status: BulkJobStatus, total_items: i32, succeeded_count: i32, failed_count: i32) -> Self {
        Self {
            id: Uuid::new_v4(),
            company_id,
            operation_type,
            target_module,
            status,
            total_items,
            succeeded_count,
            failed_count,
            submitted_by: None,
            metadata: AuditMetadata::default(),
        }
    }

    /// Get the entity's unique identifier
    pub fn id(&self) -> &Uuid {
        &self.id
    }

    /// Get a strongly-typed ID for this entity
    pub fn typed_id(&self) -> BulkJobId {
        BulkJobId(self.id)
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
    pub fn status(&self) -> &BulkJobStatus {
        &self.status
    }


    // ==========================================================
    // Fluent Setters (with_* for optional fields)
    // ==========================================================

    /// Set the submitted_by field (chainable)
    pub fn with_submitted_by(mut self, value: Uuid) -> Self {
        self.submitted_by = Some(value);
        self
    }

    // ==========================================================
    // Partial Update
    // ==========================================================

    /// Apply partial updates from a map of field name to JSON value
    pub fn apply_patch(&mut self, fields: std::collections::HashMap<String, serde_json::Value>) {
        for (key, value) in fields {
            match key.as_str() {
                "company_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.company_id = v; }
                }
                "operation_type" => {
                    if let Ok(v) = serde_json::from_value(value) { self.operation_type = v; }
                }
                "target_module" => {
                    if let Ok(v) = serde_json::from_value(value) { self.target_module = v; }
                }
                "status" => {
                    if let Ok(v) = serde_json::from_value(value) { self.status = v; }
                }
                "total_items" => {
                    if let Ok(v) = serde_json::from_value(value) { self.total_items = v; }
                }
                "succeeded_count" => {
                    if let Ok(v) = serde_json::from_value(value) { self.succeeded_count = v; }
                }
                "failed_count" => {
                    if let Ok(v) = serde_json::from_value(value) { self.failed_count = v; }
                }
                "submitted_by" => {
                    if let Ok(v) = serde_json::from_value(value) { self.submitted_by = v; }
                }
                _ => {} // ignore unknown fields
            }
        }
    }

    // <<< CUSTOM METHODS START >>>
    // <<< CUSTOM METHODS END >>>
}

impl super::Entity for BulkJob {
    type Id = Uuid;

    fn entity_id(&self) -> &Self::Id {
        &self.id
    }

    fn entity_type() -> &'static str {
        "BulkJob"
    }
}

impl backbone_core::PersistentEntity for BulkJob {
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

impl backbone_orm::EntityRepoMeta for BulkJob {
    fn column_types() -> std::collections::HashMap<String, String> {
        let mut m = std::collections::HashMap::new();
        m.insert("id".to_string(), "uuid".to_string());
        m.insert("company_id".to_string(), "uuid".to_string());
        m.insert("status".to_string(), "bulk_job_status".to_string());
        m
    }
    fn search_fields() -> &'static [&'static str] {
        &["operation_type", "target_module"]
    }
    fn company_field() -> Option<&'static str> {
        Some("company_id")
    }
}

/// Builder for BulkJob entity
///
/// Provides a fluent API for constructing BulkJob instances.
/// System fields (id, metadata, timestamps) are auto-initialized.
#[derive(Debug, Clone, Default)]
pub struct BulkJobBuilder {
    company_id: Option<Uuid>,
    operation_type: Option<String>,
    target_module: Option<String>,
    status: Option<BulkJobStatus>,
    total_items: Option<i32>,
    succeeded_count: Option<i32>,
    failed_count: Option<i32>,
    submitted_by: Option<Uuid>,
}

impl BulkJobBuilder {
    /// Set the company_id field (required)
    pub fn company_id(mut self, value: Uuid) -> Self {
        self.company_id = Some(value);
        self
    }

    /// Set the operation_type field (required)
    pub fn operation_type(mut self, value: String) -> Self {
        self.operation_type = Some(value);
        self
    }

    /// Set the target_module field (required)
    pub fn target_module(mut self, value: String) -> Self {
        self.target_module = Some(value);
        self
    }

    /// Set the status field (default: `BulkJobStatus::default()`)
    pub fn status(mut self, value: BulkJobStatus) -> Self {
        self.status = Some(value);
        self
    }

    /// Set the total_items field (default: `0`)
    pub fn total_items(mut self, value: i32) -> Self {
        self.total_items = Some(value);
        self
    }

    /// Set the succeeded_count field (default: `0`)
    pub fn succeeded_count(mut self, value: i32) -> Self {
        self.succeeded_count = Some(value);
        self
    }

    /// Set the failed_count field (default: `0`)
    pub fn failed_count(mut self, value: i32) -> Self {
        self.failed_count = Some(value);
        self
    }

    /// Set the submitted_by field (optional)
    pub fn submitted_by(mut self, value: Uuid) -> Self {
        self.submitted_by = Some(value);
        self
    }

    /// Build the BulkJob entity
    ///
    /// Returns Err if any required field without a default is missing.
    pub fn build(self) -> Result<BulkJob, String> {
        let company_id = self.company_id.ok_or_else(|| "company_id is required".to_string())?;
        let operation_type = self.operation_type.ok_or_else(|| "operation_type is required".to_string())?;
        let target_module = self.target_module.ok_or_else(|| "target_module is required".to_string())?;

        Ok(BulkJob {
            id: Uuid::new_v4(),
            company_id,
            operation_type,
            target_module,
            status: self.status.unwrap_or(BulkJobStatus::default()),
            total_items: self.total_items.unwrap_or(0),
            succeeded_count: self.succeeded_count.unwrap_or(0),
            failed_count: self.failed_count.unwrap_or(0),
            submitted_by: self.submitted_by,
            metadata: AuditMetadata::default(),
        })
    }
}
