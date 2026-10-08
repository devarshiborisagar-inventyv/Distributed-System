use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use ed25519_dalek::SigningKey;
use rand::rngs::OsRng;

/// Named so the private half can't be shipped to a follower by index slip:
/// anyone holding `signing` can forge leader writes.
pub struct ClusterKeys {
    pub signing: String,
    pub verifying: String,
}

pub fn generate_keys() -> ClusterKeys {
    let signing_key = SigningKey::generate(&mut OsRng);
    let verifying_key = signing_key.verifying_key();

    ClusterKeys {
        signing: URL_SAFE_NO_PAD.encode(signing_key.to_bytes()),
        verifying: URL_SAFE_NO_PAD.encode(verifying_key.to_bytes()),
    }
}
