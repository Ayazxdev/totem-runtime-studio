//! Capability token issuance and validation.

use chrono::{Duration, Utc};
use sentinel_core::{
    error::{SentinelError, SentinelResult},
    types::CapabilityToken,
};
use uuid::Uuid;

pub struct CapabilityManager {
    tokens: dashmap::DashMap<Uuid, CapabilityToken>,
}

impl CapabilityManager {
    pub fn new() -> Self {
        Self {
            tokens: dashmap::DashMap::new(),
        }
    }

    /// Issue a new capability token.
    pub fn issue_token(
        &self,
        subject: String,
        resource: String,
        actions: Vec<String>,
    ) -> CapabilityToken {
        let token = CapabilityToken {
            id: Uuid::new_v4(),
            subject,
            resource,
            actions,
            constraints: Default::default(),
            issued_at: Utc::now(),
            expires_at: Some(Utc::now() + Duration::hours(24)),
        };
        self.tokens.insert(token.id, token.clone());
        token
    }

    /// Validate a capability token.
    pub fn validate(&self, token_id: &Uuid) -> SentinelResult<CapabilityToken> {
        let token = self
            .tokens
            .get(token_id)
            .ok_or_else(|| SentinelError::InvalidToken(format!("Token {} not found", token_id)))?
            .clone();

        // Check expiration
        if let Some(expires_at) = token.expires_at {
            if Utc::now() > expires_at {
                return Err(SentinelError::InvalidToken(format!(
                    "Token {} expired at {}",
                    token_id, expires_at
                )));
            }
        }

        Ok(token)
    }

    /// Check if a token grants permission for a specific action on a resource.
    pub fn check_permission(
        &self,
        subject: &str,
        resource: &str,
        action: &str,
    ) -> SentinelResult<()> {
        // Find matching token
        for entry in self.tokens.iter() {
            let token = entry.value();
            if token.subject == subject
                && token.resource == resource
                && token.actions.contains(&action.to_string())
            {
                // Validate expiry
                if let Some(expires_at) = token.expires_at {
                    if Utc::now() > expires_at {
                        continue;
                    }
                }
                return Ok(());
            }
        }

        Err(SentinelError::CapabilityDenied {
            action: action.to_string(),
            resource: resource.to_string(),
            reason: format!("No valid capability token found for subject '{}'", subject),
        })
    }
}

impl Default for CapabilityManager {
    fn default() -> Self {
        Self::new()
    }
}
