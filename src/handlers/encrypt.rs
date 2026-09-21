use ed25519_dalek::{SigningKey, VerifyingKey, Signature, Signer, Verifier};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
// Sender: sign the actual data with the private key
pub fn sign_data(data: &str, signing_key: &SigningKey) -> String {
    let signature: Signature = signing_key.sign(data.as_bytes());

    format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(data),
        URL_SAFE_NO_PAD.encode(signature.to_bytes())
    )
}

// Receiver: verify using the public key, get back the trusted data
pub fn verify_data(token: &str, verifying_key: &VerifyingKey) -> Option<String> {
    let (data_b64, sig_b64) = token.split_once('.')?;
    let data_bytes = URL_SAFE_NO_PAD.decode(data_b64).ok()?;
    let sig_bytes = URL_SAFE_NO_PAD.decode(sig_b64).ok()?;

    let signature = Signature::from_slice(&sig_bytes).ok()?;

    // This is the core check: does this signature actually match
    // this exact data, under this exact public key?
    verifying_key.verify(&data_bytes, &signature).ok()?;

    String::from_utf8(data_bytes).ok()
}

pub fn string_to_signing_key(s: &str) -> Result<SigningKey, String> {
    // Step 1: decode base64 text back into raw bytes
    let bytes = URL_SAFE_NO_PAD.decode(s)
        .map_err(|e| format!("invalid base64: {e}"))?;

    // Step 2: SigningKey requires EXACTLY 32 bytes — enforce that
    let arr: [u8; 32] = bytes.try_into()
        .map_err(|_| "signing key must be exactly 32 bytes".to_string())?;

    // Step 3: construct the key from the fixed-size array
    Ok(SigningKey::from_bytes(&arr))
}

pub fn string_to_verifying_key(s: &str) -> Result<VerifyingKey, String> {
    // Step 1: decode base64 text back into raw bytes
    let bytes = URL_SAFE_NO_PAD.decode(s)
        .map_err(|e| format!("invalid base64: {e}"))?;

    // Step 2: VerifyingKey requires EXACTLY 32 bytes — enforce that
    let arr: [u8; 32] = bytes.try_into()
        .map_err(|_| "verifying key must be exactly 32 bytes".to_string())?;

    // Step 3: construct the key from the fixed-size array
    Ok(VerifyingKey::from_bytes(&arr).unwrap())
}