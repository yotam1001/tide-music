//! Verify exact signed bytes against a built-in publisher key, never a server key.
use ring::signature::{UnparsedPublicKey, ED25519};
use serde::Deserialize;

const DOMAIN: &[u8] = b"Tide update manifest v1\n";
pub(super) const MAX_MANIFEST: usize = 16 * 1024;
const MAX_VALIDITY: u64 = 90 * 24 * 60 * 60;
const PUBLIC_KEY: &str = include_str!("../../update-public-key.hex");

#[derive(Clone, Debug)]
pub(super) struct Authorization {
    issued: u64,
    expires: u64,
}
impl Authorization {
    pub(super) fn valid(&self, now: u64) -> bool {
        self.issued <= now.saturating_add(300) && self.expires > now
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    payload: String,
    signature: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema: u32,
    version: String,
    platform: String,
    filename: String,
    size: u64,
    sha256: String,
    issued_at: u64,
    expires_at: u64,
}
fn hex<const N: usize>(value: &str) -> Result<[u8; N], String> {
    if value.len() != N * 2 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Invalid update signature encoding".into());
    }
    let mut bytes = [0; N];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
            .map_err(|_| "Invalid update signature encoding")?;
    }
    Ok(bytes)
}
pub(super) fn authenticate(
    envelope: &[u8],
    version: &str,
    digest: &str,
    size: u64,
    now: u64,
) -> Result<Authorization, String> {
    verify(
        envelope,
        version,
        digest,
        size,
        now,
        &hex::<32>(PUBLIC_KEY.trim())?,
    )
}
fn verify(
    bytes: &[u8],
    version: &str,
    digest: &str,
    size: u64,
    now: u64,
    key: &[u8; 32],
) -> Result<Authorization, String> {
    if bytes.len() > MAX_MANIFEST {
        return Err("Signed update manifest is too large".into());
    }
    let envelope: Envelope =
        serde_json::from_slice(bytes).map_err(|_| "Invalid signed update manifest")?;
    let signature = hex::<64>(&envelope.signature)?;
    let mut message = Vec::with_capacity(DOMAIN.len() + envelope.payload.len());
    message.extend_from_slice(DOMAIN);
    message.extend_from_slice(envelope.payload.as_bytes());
    UnparsedPublicKey::new(&ED25519, key)
        .verify(&message, &signature)
        .map_err(|_| "Publisher signature verification failed. Nothing was installed.")?;
    let manifest: Manifest =
        serde_json::from_str(&envelope.payload).map_err(|_| "Invalid signed installer identity")?;
    if manifest.schema != 1
        || manifest.version != version
        || manifest.platform != "windows-x86_64"
        || manifest.filename != "TideSetup.exe"
        || manifest.size != size
        || !(1..=super::MAX_INSTALLER).contains(&manifest.size)
        || manifest.sha256 != digest
        || manifest.issued_at > now.saturating_add(300)
        || manifest.expires_at <= now
        || manifest.expires_at <= manifest.issued_at
        || manifest.expires_at - manifest.issued_at > MAX_VALIDITY
    {
        return Err("Signed update identity is expired or does not match this release".into());
    }
    Ok(Authorization {
        issued: manifest.issued_at,
        expires: manifest.expires_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::{
        rand::SystemRandom,
        signature::{Ed25519KeyPair, KeyPair},
    };
    use serde_json::json;

    const NOW: u64 = 1_800_000_000;
    fn manifest() -> serde_json::Value {
        json!({"schema":1,"version":"0.4.1","platform":"windows-x86_64",
            "filename":"TideSetup.exe","size":4,"sha256":"fixture",
            "issued_at":NOW,"expires_at":NOW+3600})
    }
    fn key() -> Ed25519KeyPair {
        Ed25519KeyPair::from_pkcs8(
            Ed25519KeyPair::generate_pkcs8(&SystemRandom::new())
                .unwrap()
                .as_ref(),
        )
        .unwrap()
    }
    fn signed(key: &Ed25519KeyPair, manifest: &serde_json::Value) -> Vec<u8> {
        let payload = serde_json::to_string(manifest).unwrap();
        let message = [DOMAIN, payload.as_bytes()].concat();
        let signature: String = key
            .sign(&message)
            .as_ref()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        serde_json::to_vec(&json!({"payload":payload,"signature":signature})).unwrap()
    }
    fn check(bytes: &[u8], key: &Ed25519KeyPair) -> Result<Authorization, String> {
        verify(
            bytes,
            "0.4.1",
            "fixture",
            4,
            NOW,
            &key.public_key().as_ref().try_into().unwrap(),
        )
    }
    #[test]
    fn python_signer_and_rust_verifier_agree_on_exact_bytes() {
        let bytes = include_bytes!("fixtures/python-envelope.json");
        let identity: Manifest =
            serde_json::from_slice(include_bytes!("fixtures/python-identity.json")).unwrap();
        let key = hex::<32>(include_str!("fixtures/python-public-key.hex").trim()).unwrap();
        assert!(verify(
            bytes,
            &identity.version,
            &identity.sha256,
            identity.size,
            identity.issued_at,
            &key
        )
        .is_ok());
        assert!(hex::<32>(PUBLIC_KEY.trim()).is_ok());
    }
    #[test]
    fn signatures_bind_the_installer_and_cannot_be_replaced_by_a_server_key() {
        let publisher = key();
        let bytes = signed(&publisher, &manifest());
        let auth = check(&bytes, &publisher).unwrap();
        assert!(auth.valid(NOW));
        assert!(!auth.valid(NOW + 3600));
        assert!(check(&bytes, &key()).is_err());
        // Editing payload bytes without the key invalidates the signature.
        let mut edited: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        edited["payload"] = json!(edited["payload"]
            .as_str()
            .unwrap()
            .replace("fixture", "changed"));
        assert!(check(&serde_json::to_vec(&edited).unwrap(), &publisher).is_err());
        assert!(authenticate(&bytes, "0.4.1", "fixture", 4, NOW).is_err());
    }
    #[test]
    fn even_correct_signatures_cannot_authorize_wrong_expired_or_malformed_releases() {
        let key = key();
        for (field, value) in [
            ("schema", json!(2)),
            ("version", json!("0.4.0")),
            ("platform", json!("other")),
            ("filename", json!("Other.exe")),
            ("size", json!(5)),
            ("sha256", json!("changed")),
            ("expires_at", json!(NOW)),
            ("issued_at", json!(NOW + 301)),
            ("expires_at", json!(NOW + MAX_VALIDITY + 1)),
            ("command", json!("run arbitrary code")),
        ] {
            let mut data = manifest();
            data[field] = value;
            assert!(check(&signed(&key, &data), &key).is_err(), "{field}");
        }
        for bytes in [Vec::new(), b"{}".to_vec(), vec![b' '; MAX_MANIFEST + 1]] {
            assert!(check(&bytes, &key).is_err());
        }
        assert!(hex::<64>(&"0".repeat(127)).is_err());
        assert!(hex::<32>("not a public key").is_err());
    }
}
