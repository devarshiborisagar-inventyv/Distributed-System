use ed25519_dalek::SigningKey;
use rand::rngs::OsRng;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};

fn main() {
    let signing_key = SigningKey::generate(&mut OsRng);
    let verifying_key = signing_key.verifying_key();

    println!("SIGNING_KEY={}", URL_SAFE_NO_PAD.encode(signing_key.to_bytes()));
    println!("VERIFYING_KEY={}", URL_SAFE_NO_PAD.encode(verifying_key.to_bytes()));
}