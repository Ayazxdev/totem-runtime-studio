//! Hash-chain integrity for audit events.

use blake3::Hasher;
use sentinel_core::types::AuditEvent;

/// Compute BLAKE3 hash of an audit event.
/// The hash includes: id, timestamp, event_type, subject, details, previous_hash.
pub fn compute_event_hash(event: &AuditEvent) -> String {
    let mut hasher = Hasher::new();

    hasher.update(event.id.as_bytes());
    hasher.update(event.timestamp.to_rfc3339().as_bytes());
    hasher.update(format!("{:?}", event.event_type).as_bytes());
    hasher.update(event.subject.as_bytes());
    hasher.update(event.details.to_string().as_bytes());

    if let Some(ref prev) = event.previous_hash {
        hasher.update(prev.as_bytes());
    }

    hex::encode(hasher.finalize().as_bytes())
}
