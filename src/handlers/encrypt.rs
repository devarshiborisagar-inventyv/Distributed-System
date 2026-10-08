use crate::error::AppError;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};

/// Sender: sign the actual data with the private key. Cannot fail.
pub fn sign_data(data: &str, signing_key: &SigningKey) -> String {
    let signature: Signature = signing_key.sign(data.as_bytes());

    format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(data),
        URL_SAFE_NO_PAD.encode(signature.to_bytes())
    )
}

/// Receiver: verify using the public key, get back the trusted data.
pub fn verify_data(token: &str, verifying_key: &VerifyingKey) -> Result<String, AppError> {
    let (data_b64, sig_b64) = token.split_once('.').ok_or(AppError::InvalidPayload)?;

    let data_bytes = URL_SAFE_NO_PAD
        .decode(data_b64)
        .map_err(|_| AppError::InvalidPayload)?;
    let sig_bytes = URL_SAFE_NO_PAD
        .decode(sig_b64)
        .map_err(|_| AppError::InvalidPayload)?;

    let signature = Signature::from_slice(&sig_bytes).map_err(|_| AppError::InvalidToken)?;

    // The core check: does this signature match this exact data, under this
    // exact public key?
    verifying_key
        .verify(&data_bytes, &signature)
        .map_err(|_| AppError::InvalidToken)?;

    String::from_utf8(data_bytes).map_err(|_| AppError::InvalidPayload)
}

/// Decode 32 base64 bytes, or say why not. An empty string means the
/// orchestrator has not pushed this node's key yet — a different problem from
/// a corrupt one, and a different status code.
fn key_bytes(s: &str) -> Result<[u8; 32], AppError> {
    if s.is_empty() {
        return Err(AppError::NotInitialized);
    }
    let bytes = URL_SAFE_NO_PAD
        .decode(s)
        .map_err(|_| AppError::InvalidKey)?;
    bytes
        .try_into()
        .map_err(|_| AppError::InvalidKey)
}

pub fn string_to_signing_key(s: &str) -> Result<SigningKey, AppError> {
    Ok(SigningKey::from_bytes(&key_bytes(s)?))
}

pub fn string_to_verifying_key(s: &str) -> Result<VerifyingKey, AppError> {
    VerifyingKey::from_bytes(&key_bytes(s)?)
        .map_err(|_| AppError::InvalidKey)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keypair() -> (SigningKey, VerifyingKey) {
        let sk = SigningKey::from_bytes(&[7u8; 32]);
        let vk = sk.verifying_key();
        (sk, vk)
    }

    #[test]
    fn round_trip() {
        let (sk, vk) = keypair();
        let token = sign_data("hello", &sk);
        assert_eq!(verify_data(&token, &vk).unwrap(), "hello");
    }

    #[test]
    fn rejects_tampering_without_panicking() {
        let (sk, vk) = keypair();
        let token = sign_data("hello", &sk);
        let (_, sig) = token.split_once('.').unwrap();
        let forged = format!("{}.{}", URL_SAFE_NO_PAD.encode("goodbye"), sig);
        assert!(matches!(
            verify_data(&forged, &vk),
            Err(AppError::InvalidToken)
        ));
    }

    #[test]
    fn garbage_is_an_error_not_a_panic() {
        let (_, vk) = keypair();
        for bad in ["", "garbage", "no-dot-here", "!!!.!!!"] {
            assert!(verify_data(bad, &vk).is_err(), "{bad:?} should fail");
        }
    }

    #[test]
    fn empty_key_reports_not_initialised() {
        assert!(matches!(
            string_to_verifying_key(""),
            Err(AppError::NotInitialized)
        ));
        assert!(matches!(
            string_to_signing_key(""),
            Err(AppError::NotInitialized)
        ));
        assert!(matches!(
            string_to_verifying_key("short"),
            Err(AppError::InvalidKey)
        ));
    }
}
