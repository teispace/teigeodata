//! Pack signatures (feature `signature`): Ed25519 over the head digest, with a domain prefix so a
//! pack signature can never be replayed as another kind of signature from the same key.
//!
//! The signature block is an 8-byte key ID (the first bytes of BLAKE3 of the public key) and the
//! 64-byte signature. Verification uses `verify_strict`, which rejects malleable signatures and
//! weak keys.

use ed25519_dalek::{Signature, VerifyingKey};

use crate::{Error, Pack};

/// The bytes signed before the head digest.
pub const SIGNATURE_DOMAIN: &[u8; 29] = b"teigeodata pack signature v0\0";

/// The key ID: the first 8 bytes of BLAKE3 of the 32-byte public key.
#[must_use]
pub fn key_id(key: &VerifyingKey) -> [u8; 8] {
    let mut out = [0; 8];
    out.copy_from_slice(
        blake3::hash(key.as_bytes())
            .as_bytes()
            .get(..8)
            .unwrap_or(&[0; 8]),
    );
    out
}

fn message(digest: &[u8; 32]) -> [u8; 61] {
    let mut m = [0; 61];
    let (domain, rest) = m.split_at_mut(SIGNATURE_DOMAIN.len());
    domain.copy_from_slice(SIGNATURE_DOMAIN);
    rest.copy_from_slice(digest);
    m
}

impl Pack<'_> {
    /// Verifies the signature against a set of trusted keys and returns the index of the key that
    /// signed it.
    ///
    /// # Errors
    /// [`Error::Signature`] if the pack is unsigned, signed by none of `keys`, or the signature
    /// is invalid.
    pub fn verify_signature(&self, keys: &[VerifyingKey]) -> Result<usize, Error> {
        let (id, sig) = self
            .signature()
            .ok_or(Error::Signature("the pack is not signed"))?;
        let index = keys
            .iter()
            .position(|k| key_id(k) == id)
            .ok_or(Error::Signature("signed by a key that is not trusted"))?;
        let key = keys.get(index).ok_or(Error::Signature("no key"))?;
        key.verify_strict(&message(self.head_digest()), &Signature::from_bytes(&sig))
            .map_err(|_| Error::Signature("the signature is invalid"))?;
        Ok(index)
    }
}

/// A signer for [`crate::Layout::set_signature`]: returns the 72-byte block for a digest.
#[cfg(feature = "write")]
pub fn signer(key: &ed25519_dalek::SigningKey) -> impl Fn(&[u8; 32]) -> [u8; 72] + '_ {
    use ed25519_dalek::Signer as _;
    move |digest| {
        let mut block = [0; 72];
        let (id, sig) = block.split_at_mut(8);
        id.copy_from_slice(&key_id(&key.verifying_key()));
        sig.copy_from_slice(&key.sign(&message(digest)).to_bytes());
        block
    }
}
