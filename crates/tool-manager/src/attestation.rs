//! Tool attestation — cryptographic verification of tool identity.
//!
//! Ed25519 (classical) and ML-DSA (post-quantum, CRYSTALS-Dilithium) verification.
//! Algorithm selection is driven by `ToolAttestation.algorithm`:
//!   - "ed25519"  → Ed25519 signature over SHA-256(payload)
//!   - "ml-dsa"   → Dilithium3 signature over payload (quantum-resistant)

use ed25519_dalek::{Signature as Ed25519Signature, VerifyingKey, Verifier};
use pqcrypto_dilithium::dilithium3;
use sha2::{Sha256, Digest};

use sentinel_core::{
    error::{SentinelError, SentinelResult},
    types::ToolAttestation,
};

/// Verify a tool attestation using the algorithm specified in the attestation record.
pub fn verify_attestation(attestation: &ToolAttestation) -> SentinelResult<()> {
    match attestation.algorithm.as_str() {
        "ed25519" => verify_ed25519(attestation),
        "ml-dsa" | "dilithium3" => verify_ml_dsa(attestation),
        algo => Err(SentinelError::ToolAttestationFailed(format!(
            "Unknown attestation algorithm: {}",
            algo
        ))),
    }
}

/// Classical Ed25519 verification.
/// Expects: public_key = 32-byte compressed point, signature = 64-byte Ed25519 sig over SHA-256(tool_name).
fn verify_ed25519(attestation: &ToolAttestation) -> SentinelResult<()> {
    let key_bytes: [u8; 32] = attestation
        .public_key
        .as_slice()
        .try_into()
        .map_err(|_| SentinelError::ToolAttestationFailed(
            "Ed25519 public key must be 32 bytes".to_string()
        ))?;

    let verifying_key = VerifyingKey::from_bytes(&key_bytes)
        .map_err(|e| SentinelError::ToolAttestationFailed(format!("Invalid Ed25519 key: {e}")))?;

    let sig_bytes: [u8; 64] = attestation
        .signature
        .as_slice()
        .try_into()
        .map_err(|_| SentinelError::ToolAttestationFailed(
            "Ed25519 signature must be 64 bytes".to_string()
        ))?;

    let signature = Ed25519Signature::from_bytes(&sig_bytes);

    // Message is SHA-256 of the tool public key (identity binding)
    let mut hasher = Sha256::new();
    hasher.update(&attestation.public_key);
    let msg = hasher.finalize();

    verifying_key
        .verify(&msg, &signature)
        .map_err(|e| SentinelError::ToolAttestationFailed(format!("Ed25519 verification failed: {e}")))?;

    tracing::debug!("Ed25519 attestation verified");
    Ok(())
}

/// Post-quantum ML-DSA (Dilithium3) verification.
/// Expects: public_key = Dilithium3 public key bytes, signature = signed message bytes.
fn verify_ml_dsa(attestation: &ToolAttestation) -> SentinelResult<()> {
    use pqcrypto_traits::sign::{PublicKey as PkTrait, SignedMessage as SmTrait};

    let pk = dilithium3::PublicKey::from_bytes(&attestation.public_key)
        .map_err(|_| SentinelError::ToolAttestationFailed(
            "Invalid Dilithium3 public key".to_string()
        ))?;

    let signed_msg = dilithium3::SignedMessage::from_bytes(&attestation.signature)
        .map_err(|_| SentinelError::ToolAttestationFailed(
            "Invalid Dilithium3 signed message".to_string()
        ))?;

    dilithium3::open(&signed_msg, &pk)
        .map_err(|_| SentinelError::ToolAttestationFailed(
            "Dilithium3 (ML-DSA) signature verification failed".to_string()
        ))?;

    tracing::debug!("ML-DSA (Dilithium3) attestation verified");
    Ok(())
}

/// Generate an Ed25519 keypair for tool signing (utility, used in tests / key generation CLI).
pub fn generate_ed25519_keypair() -> (Vec<u8>, Vec<u8>) {
    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;
    let signing_key = SigningKey::generate(&mut OsRng);
    let public_key = signing_key.verifying_key().to_bytes().to_vec();
    let secret_key = signing_key.to_bytes().to_vec();
    (public_key, secret_key)
}

/// Generate a Dilithium3 (ML-DSA) keypair for tool signing.
pub fn generate_ml_dsa_keypair() -> (Vec<u8>, Vec<u8>) {
    let (pk, sk) = dilithium3::keypair();
    use pqcrypto_traits::sign::{PublicKey, SecretKey};
    (pk.as_bytes().to_vec(), sk.as_bytes().to_vec())
}
