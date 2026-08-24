use sha2::{Digest, Sha256};
use std::fmt;

const MAGIC: &[u8; 4] = b"TLID";
pub const IDENTITY_VERSION: u16 = 1;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdentityError {
    pub message: String,
}

impl IdentityError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for IdentityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for IdentityError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PluginIdentityInput<'a> {
    pub name: &'a str,
    pub version: &'a str,
    pub schema: &'a [u8],
    pub metadata: &'a [u8],
    pub implementation_digest: Option<&'a [u8; 32]>,
}

fn check_utf8_field(label: &str, s: &str) -> Result<(), IdentityError> {
    if s.is_empty() {
        return Err(IdentityError::new(format!("{label} must be non-empty")));
    }
    if s.contains('\0') {
        return Err(IdentityError::new(format!("U+0000 in {label}")));
    }
    Ok(())
}

pub fn encode_plugin_identity(input: &PluginIdentityInput<'_>) -> Result<Vec<u8>, IdentityError> {
    check_utf8_field("name", input.name)?;
    check_utf8_field("version", input.version)?;
    let mut buf = Vec::new();
    buf.extend_from_slice(MAGIC);
    buf.extend_from_slice(&IDENTITY_VERSION.to_le_bytes());
    let name = input.name.as_bytes();
    buf.extend_from_slice(&(name.len() as u32).to_le_bytes());
    buf.extend_from_slice(name);
    let version = input.version.as_bytes();
    buf.extend_from_slice(&(version.len() as u32).to_le_bytes());
    buf.extend_from_slice(version);
    buf.extend_from_slice(&(input.schema.len() as u32).to_le_bytes());
    buf.extend_from_slice(input.schema);
    buf.extend_from_slice(&(input.metadata.len() as u32).to_le_bytes());
    buf.extend_from_slice(input.metadata);
    match input.implementation_digest {
        None => buf.push(0),
        Some(digest) => {
            buf.push(1);
            buf.extend_from_slice(digest);
        }
    }
    Ok(buf)
}

pub fn hash_canonical_bytes(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

pub fn hash_plugin_identity(input: &PluginIdentityInput<'_>) -> Result<[u8; 32], IdentityError> {
    Ok(hash_canonical_bytes(&encode_plugin_identity(input)?))
}

pub fn assign_local_ids(hashes: &[[u8; 32]]) -> Result<Vec<u32>, IdentityError> {
    let n = hashes.len();
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&i, &j| hashes[i].cmp(&hashes[j]));
    for w in order.windows(2) {
        if hashes[w[0]] == hashes[w[1]] {
            return Err(IdentityError::new("duplicate plugin identity hash"));
        }
    }
    let mut ids = vec![0u32; n];
    for (id, &idx) in order.iter().enumerate() {
        ids[idx] = id as u32;
    }
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex32(s: &str) -> [u8; 32] {
        let mut out = [0u8; 32];
        for i in 0..32 {
            out[i] = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).unwrap();
        }
        out
    }

    fn vector_a() -> PluginIdentityInput<'static> {
        PluginIdentityInput {
            name: "echo",
            version: "1.0.0",
            schema: b"(schema echo v1)",
            metadata: b"",
            implementation_digest: None,
        }
    }

    #[test]
    fn golden_vector_a() {
        let bytes = encode_plugin_identity(&vector_a()).unwrap();
        assert_eq!(
            bytes,
            decode_hex(
                "544c49440100040000006563686f05000000312e302e301000000028736368656d61206563686f207631290000000000"
            )
        );
        assert_eq!(
            hash_plugin_identity(&vector_a()).unwrap(),
            hex32("b53818f082a602686525d386618246569a4f74a4997aa3dbe5006f5644ab5ba3")
        );
    }

    #[test]
    fn golden_vector_b_version_change() {
        let input = PluginIdentityInput {
            version: "2.0.0",
            ..vector_a()
        };
        assert_eq!(
            hash_plugin_identity(&input).unwrap(),
            hex32("db97a544bfdef8e2fd10e80b152b7418568c3f856b72bff0701de5d8b335cb2d")
        );
        assert_ne!(
            hash_plugin_identity(&input).unwrap(),
            hash_plugin_identity(&vector_a()).unwrap()
        );
    }

    #[test]
    fn golden_vector_c_impl_digest() {
        let digest = [0x11u8; 32];
        let input = PluginIdentityInput {
            implementation_digest: Some(&digest),
            ..vector_a()
        };
        assert_eq!(
            hash_plugin_identity(&input).unwrap(),
            hex32("d93ad774747d2adb651866d0d24ef29783f0dcd0f431cda761da1c03473f67d0")
        );
    }

    #[test]
    fn golden_vector_d_and_sort_by_hash() {
        let clock = PluginIdentityInput {
            name: "clock",
            version: "1.0.0",
            schema: b"(schema clock v1)",
            metadata: b"",
            implementation_digest: None,
        };
        let echo_h = hash_plugin_identity(&vector_a()).unwrap();
        let clock_h = hash_plugin_identity(&clock).unwrap();
        assert_eq!(
            clock_h,
            hex32("2a1ca0a188fe61311c1dd8d9f73f2685d9ae8e6199db222da78977c3d3526dfa")
        );
        assert!(clock_h < echo_h);
        assert_eq!(assign_local_ids(&[echo_h, clock_h]).unwrap(), vec![1, 0]);
        assert_eq!(assign_local_ids(&[clock_h, echo_h]).unwrap(), vec![0, 1]);
    }

    #[test]
    fn assign_local_ids_rejects_duplicate_hashes() {
        let h = hash_plugin_identity(&vector_a()).unwrap();
        match assign_local_ids(&[h, h]) {
            Err(e) => assert!(e.message.contains("duplicate")),
            Ok(v) => panic!("expected reject, got {v:?}"),
        }
    }

    #[test]
    fn encode_rejects_empty_name() {
        let input = PluginIdentityInput {
            name: "",
            ..vector_a()
        };
        assert!(encode_plugin_identity(&input).is_err());
    }

    #[test]
    fn hash_canonical_bytes_is_sha256() {
        let bytes = encode_plugin_identity(&vector_a()).unwrap();
        assert_eq!(
            hash_canonical_bytes(&bytes),
            hash_plugin_identity(&vector_a()).unwrap()
        );
    }

    fn decode_hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }
}
