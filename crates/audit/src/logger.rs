//! Audit event logger.

use crate::chain::compute_event_hash;
use chrono::Utc;
use sentinel_core::{
    error::{SentinelError, SentinelResult},
    types::{AuditEvent, AuditEventType},
};
use std::sync::Mutex;
use uuid::Uuid;

pub struct AuditLogger {
    events: Mutex<Vec<AuditEvent>>,
    last_hash: Mutex<Option<String>>,
}

impl AuditLogger {
    pub fn new() -> Self {
        Self {
            events: Mutex::new(Vec::new()),
            last_hash: Mutex::new(None),
        }
    }

    pub async fn log_event(
        &self,
        event_type: AuditEventType,
        task_id: Option<Uuid>,
        action_id: Option<Uuid>,
        subject: String,
        details: serde_json::Value,
    ) -> SentinelResult<Uuid> {
        let previous_hash = self.last_hash.lock().unwrap().clone();

        let event = AuditEvent {
            id: Uuid::new_v4(),
            timestamp: Utc::now(),
            event_type: event_type.clone(),
            task_id,
            action_id,
            subject: subject.clone(),
            details: details.clone(),
            previous_hash: previous_hash.clone(),
            event_hash: String::new(), // computed below
        };

        // Compute hash of this event (including previous_hash for chaining)
        let event_hash = compute_event_hash(&event);
        let mut event = event;
        event.event_hash = event_hash.clone();

        // Store event
        self.events
            .lock()
            .map_err(|e| SentinelError::Audit(format!("Lock poisoned: {}", e)))?
            .push(event.clone());

        // Update last hash
        *self
            .last_hash
            .lock()
            .map_err(|e| SentinelError::Audit(format!("Lock poisoned: {}", e)))? = Some(event_hash);

        tracing::debug!(
            "📋 Audit: {:?} for task {:?} (subject: {})",
            event_type,
            task_id,
            subject
        );

        Ok(event.id)
    }

    pub fn get_events(&self) -> SentinelResult<Vec<AuditEvent>> {
        self.events
            .lock()
            .map(|events| events.clone())
            .map_err(|e| SentinelError::Audit(format!("Lock poisoned: {}", e)))
    }

    /// Verify the integrity of the audit chain.
    pub fn verify_chain(&self) -> SentinelResult<bool> {
        let events = self.get_events()?;
        let mut prev_hash: Option<String> = None;

        for event in &events {
            // Check that previous_hash matches
            if event.previous_hash != prev_hash {
                return Ok(false);
            }

            // Recompute hash and verify
            let computed = compute_event_hash(event);
            if computed != event.event_hash {
                return Ok(false);
            }

            prev_hash = Some(event.event_hash.clone());
        }

        Ok(true)
    }
}

impl Default for AuditLogger {
    fn default() -> Self {
        Self::new()
    }
}
