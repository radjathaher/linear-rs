use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand::RngCore;
use sha2::{Digest, Sha256};

/// PKCE code verifier and challenge pair.
#[derive(Debug, Clone)]
pub struct PkcePair {
    verifier: String,
    challenge: String,
}

impl PkcePair {
    /// Create a new random verifier/challenge pair following RFC 7636 recommendations.
    pub fn generate() -> Self {
        let verifier = generate_verifier();
        let challenge = generate_challenge(&verifier);
        Self {
            verifier,
            challenge,
        }
    }

    pub fn verifier(&self) -> &str {
        &self.verifier
    }

    pub fn challenge(&self) -> &str {
        &self.challenge
    }
}

fn generate_verifier() -> String {
    const BYTE_LEN: usize = 32;
    let mut bytes = [0u8; BYTE_LEN];
    rand::thread_rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

fn generate_challenge(verifier: &str) -> String {
    let digest = Sha256::digest(verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(digest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verifier_meets_length_requirement() {
        let pair = PkcePair::generate();
        assert!(pair.verifier().len() >= 43);
        assert!(pair.verifier().len() <= 128);
        assert!(!pair.challenge().is_empty());
    }
}
