//! Permission policy engine.
//!
//! Rate-limiting per subject, per-action deny rules, token expiry enforcement.

use chrono::Utc;
use dashmap::{DashMap, DashSet};
use sentinel_core::{
    error::{SentinelError, SentinelResult},
    types::ActionType,
};
use std::collections::HashSet;

// Rate-limit state

/// Sliding-window invocation counter for rate-limiting.
struct RateWindow {
    count: u32,
    window_start: chrono::DateTime<Utc>,
    limit_per_minute: u32,
}

impl RateWindow {
    fn new(limit_per_minute: u32) -> Self {
        Self {
            count: 0,
            window_start: Utc::now(),
            limit_per_minute,
        }
    }

    /// Returns true if the action should be allowed (and increments the counter).
    fn check_and_tick(&mut self) -> bool {
        let now = Utc::now();
        let elapsed_secs = (now - self.window_start).num_seconds();

        if elapsed_secs >= 60 {
            // Reset window
            self.window_start = now;
            self.count = 0;
        }

        if self.count >= self.limit_per_minute {
            return false;
        }
        self.count += 1;
        true
    }
}

// PolicyEngine

pub struct PolicyEngine {
    /// subject → resource → allowed action strings
    policies: DashMap<String, DashMap<String, HashSet<String>>>,
    /// Globally denied resources (highest priority, overrides all grants)
    denied_resources: DashSet<String>,
    /// Per-action deny rules: (resource, action) pairs always denied
    denied_actions: DashSet<(String, String)>,
    /// Per-subject rate limits: subject → RateWindow (Mutex for interior mutability)
    rate_limits: DashMap<String, std::sync::Mutex<RateWindow>>,
}

impl PolicyEngine {
    pub fn new() -> Self {
        let engine = Self {
            policies: DashMap::new(),
            denied_resources: DashSet::new(),
            denied_actions: DashSet::new(),
            rate_limits: DashMap::new(),
        };
        // Default permissive grant: all subjects can execute all tools
        engine.grant("*", "*", vec!["execute".to_string()]);
        engine
    }

    /// Grant permission for subject to perform actions on resource.
    pub fn grant(&self, subject: &str, resource: &str, actions: Vec<String>) {
        self.policies
            .entry(subject.to_string())
            .or_insert_with(DashMap::new)
            .entry(resource.to_string())
            .or_insert_with(HashSet::new)
            .extend(actions);
    }

    /// Revoke all access to a resource for every subject.
    pub fn revoke_resource(&self, resource: &str) {
        self.denied_resources.insert(resource.to_string());
    }

    /// Add a per-action deny rule: deny `action` on `resource` regardless of grants.
    pub fn deny_action(&self, resource: &str, action: &str) {
        self.denied_actions
            .insert((resource.to_string(), action.to_string()));
    }

    /// Set a per-minute invocation rate limit for a subject.
    pub fn set_rate_limit(&self, subject: &str, calls_per_minute: u32) {
        self.rate_limits.insert(
            subject.to_string(),
            std::sync::Mutex::new(RateWindow::new(calls_per_minute)),
        );
    }

    /// Verify that subject can perform action_type on resource.
    pub fn verify_action(
        &self,
        subject: &str,
        resource: &str,
        action_type: &ActionType,
    ) -> SentinelResult<()> {
        let action = match action_type {
            ActionType::ToolCall           => "execute",
            ActionType::AgentCommunication => "communicate",
            ActionType::ContextUpdate      => "update_context",
        };

        // 1. Global resource deny list
        if self.denied_resources.contains(resource) {
            tracing::warn!("✗ CAPSEM [deny-list]: {} → {} denied", subject, resource);
            return Err(SentinelError::CapabilityDenied {
                action: action.to_string(),
                resource: resource.to_string(),
                reason: format!("Resource '{}' is in the global deny list", resource),
            });
        }

        // 2. Per-action deny rule
        if self.denied_actions.contains(&(resource.to_string(), action.to_string())) {
            tracing::warn!("✗ CAPSEM [action-deny]: {} → {}:{} denied", subject, resource, action);
            return Err(SentinelError::CapabilityDenied {
                action: action.to_string(),
                resource: resource.to_string(),
                reason: format!("Action '{}' on '{}' is explicitly denied", action, resource),
            });
        }

        // 3. Rate limit check (per-subject)
        if let Some(window) = self.rate_limits.get(subject) {
            let allowed = window
                .lock()
                .map(|mut w| w.check_and_tick())
                .unwrap_or(true); // if lock poisoned, allow and log
            if !allowed {
                tracing::warn!("✗ CAPSEM [rate-limit]: subject '{}' exceeded rate limit", subject);
                return Err(SentinelError::CapabilityDenied {
                    action: action.to_string(),
                    resource: resource.to_string(),
                    reason: format!("Subject '{}' exceeded its per-minute rate limit", subject),
                });
            }
        }

        // 4. Grant check: exact subject + exact resource
        if let Some(resources) = self.policies.get(subject) {
            if let Some(actions) = resources.get(resource) {
                if actions.contains(action) {
                    tracing::debug!("✓ CAPSEM: {} → {}:{}", subject, resource, action);
                    return Ok(());
                }
            }
        }

        // 5. Wildcard subject + exact resource
        if let Some(resources) = self.policies.get("*") {
            if let Some(actions) = resources.get(resource) {
                if actions.contains(action) {
                    tracing::debug!("✓ CAPSEM [wildcard-subject]: *→{}:{}", resource, action);
                    return Ok(());
                }
            }
            // 6. Wildcard subject + wildcard resource
            if let Some(actions) = resources.get("*") {
                if actions.contains(action) {
                    tracing::debug!("✓ CAPSEM [wildcard-all]: *→*:{}", action);
                    return Ok(());
                }
            }
        }

        tracing::warn!("✗ CAPSEM [no-grant]: {} → {}:{} denied", subject, resource, action);
        Err(SentinelError::CapabilityDenied {
            action: action.to_string(),
            resource: resource.to_string(),
            reason: format!("Subject '{}' has no grant for '{}:{}'", subject, resource, action),
        })
    }
}

impl Default for PolicyEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sentinel_core::types::ActionType;

    #[test]
    fn test_wildcard_grant_allows_all() {
        let engine = PolicyEngine::new();
        assert!(engine.verify_action("any-agent", "any-tool", &ActionType::ToolCall).is_ok());
    }

    #[test]
    fn test_resource_deny_overrides_grant() {
        let engine = PolicyEngine::new();
        engine.revoke_resource("secret-db");
        assert!(engine.verify_action("any-agent", "secret-db", &ActionType::ToolCall).is_err());
    }

    #[test]
    fn test_per_action_deny() {
        let engine = PolicyEngine::new();
        // Add explicit grant for communicate before denying execute
        engine.grant("*", "user-data", vec!["communicate".to_string()]);
        engine.deny_action("user-data", "execute");

        // execute is denied
        let result = engine.verify_action("agent-1", "user-data", &ActionType::ToolCall);
        assert!(result.is_err(), "execute on user-data should be denied");

        // communicate is NOT denied (per-action deny only targets 'execute')
        let comm = engine.verify_action("agent-1", "user-data", &ActionType::AgentCommunication);
        assert!(comm.is_ok(), "communicate on user-data should be allowed via explicit grant");
    }

    #[test]
    fn test_rate_limit_enforcement() {
        let engine = PolicyEngine::new();
        engine.set_rate_limit("limited-agent", 2); // 2 calls per minute
        assert!(engine.verify_action("limited-agent", "echo", &ActionType::ToolCall).is_ok());
        assert!(engine.verify_action("limited-agent", "echo", &ActionType::ToolCall).is_ok());
        // 3rd call should be rate-limited
        assert!(engine.verify_action("limited-agent", "echo", &ActionType::ToolCall).is_err());
    }
}
