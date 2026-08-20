use crate::snapshot::SnapshotError;
use aes_gcm::{
    aead::{Aead, KeyInit, Payload},
    Aes256Gcm, Nonce,
};
use sha2::{Digest, Sha256};

const MAGIC: &[u8; 4] = b"TLNA";
const CONTAINER_VERSION: u16 = 1;
const FLAG_AEAD: u16 = 1;
const MAX_BYTES: usize = 16_777_216;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginStateEntry {
    pub plugin_id: u32,
    pub blob: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContainerBody {
    pub tirs: Vec<u8>,
    pub plugin_state: Vec<PluginStateEntry>,
    pub journal_cursor: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodedContainer {
    pub instance_id: [u8; 16],
    pub body: ContainerBody,
    pub aead: bool,
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    fn u8(&mut self) -> Result<u8, SnapshotError> {
        if self.pos >= self.data.len() {
            return Err(SnapshotError::new("unexpected end of container"));
        }
        let b = self.data[self.pos];
        self.pos += 1;
        Ok(b)
    }

    fn u16(&mut self) -> Result<u16, SnapshotError> {
        let lo = self.u8()? as u16;
        let hi = self.u8()? as u16;
        Ok(lo | (hi << 8))
    }

    fn u32(&mut self) -> Result<u32, SnapshotError> {
        let b0 = self.u8()? as u32;
        let b1 = self.u8()? as u32;
        let b2 = self.u8()? as u32;
        let b3 = self.u8()? as u32;
        Ok(b0 | (b1 << 8) | (b2 << 16) | (b3 << 24))
    }

    fn u64(&mut self) -> Result<u64, SnapshotError> {
        let lo = self.u32()? as u64;
        let hi = self.u32()? as u64;
        Ok(lo | (hi << 32))
    }

    fn bytes(&mut self, n: usize) -> Result<&'a [u8], SnapshotError> {
        if self.remaining() < n {
            return Err(SnapshotError::new("unexpected end of container"));
        }
        let start = self.pos;
        self.pos += n;
        Ok(&self.data[start..self.pos])
    }
}

struct Writer {
    buf: Vec<u8>,
}

impl Writer {
    fn new() -> Self {
        Self { buf: Vec::new() }
    }

    fn u16(&mut self, v: u16) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    fn u32(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    fn u64(&mut self, v: u64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    fn bytes(&mut self, b: &[u8]) {
        self.buf.extend_from_slice(b);
    }
}

fn encode_body(body: &ContainerBody) -> Vec<u8> {
    let mut w = Writer::new();
    w.u32(body.tirs.len() as u32);
    w.bytes(&body.tirs);
    w.u32(body.plugin_state.len() as u32);
    for e in &body.plugin_state {
        w.u32(e.plugin_id);
        w.u32(e.blob.len() as u32);
        w.bytes(&e.blob);
    }
    w.u64(body.journal_cursor);
    w.buf
}

fn decode_body(bytes: &[u8]) -> Result<ContainerBody, SnapshotError> {
    let mut r = Reader::new(bytes);
    let tirs_len = r.u32()? as usize;
    let tirs = r.bytes(tirs_len)?.to_vec();
    let count = r.u32()? as usize;
    let mut plugin_state = Vec::with_capacity(count);
    for _ in 0..count {
        let plugin_id = r.u32()?;
        let blob_len = r.u32()? as usize;
        let blob = r.bytes(blob_len)?.to_vec();
        plugin_state.push(PluginStateEntry { plugin_id, blob });
    }
    let journal_cursor = r.u64()?;
    if r.remaining() != 0 {
        return Err(SnapshotError::new("trailing container body bytes"));
    }
    Ok(ContainerBody {
        tirs,
        plugin_state,
        journal_cursor,
    })
}

fn aad(instance_id: &[u8; 16], flags: u16) -> [u8; 26] {
    let mut out = [0u8; 26];
    out[0..4].copy_from_slice(MAGIC);
    out[4..6].copy_from_slice(&CONTAINER_VERSION.to_le_bytes());
    out[6..8].copy_from_slice(&flags.to_le_bytes());
    out[8..24].copy_from_slice(instance_id);
    out
}

fn finish_header(
    flags: u16,
    instance_id: [u8; 16],
    integrity: [u8; 32],
    nonce: [u8; 12],
    body: &[u8],
) -> Vec<u8> {
    let mut w = Writer::new();
    w.bytes(MAGIC);
    w.u16(CONTAINER_VERSION);
    w.u16(flags);
    w.bytes(&instance_id);
    w.bytes(&integrity);
    w.bytes(&nonce);
    w.u32(body.len() as u32);
    w.bytes(body);
    w.buf
}

pub fn encode_container(body: &ContainerBody, instance_id: [u8; 16]) -> Vec<u8> {
    let body_bytes = encode_body(body);
    let hash = Sha256::digest(&body_bytes);
    let mut integrity = [0u8; 32];
    integrity.copy_from_slice(&hash);
    finish_header(0, instance_id, integrity, [0u8; 12], &body_bytes)
}

pub fn encode_container_aead(
    body: &ContainerBody,
    instance_id: [u8; 16],
    key: &[u8; 32],
    nonce: &[u8; 12],
) -> Result<Vec<u8>, SnapshotError> {
    let plaintext = encode_body(body);
    let flags = FLAG_AEAD;
    let aad_bytes = aad(&instance_id, flags);
    let cipher =
        Aes256Gcm::new_from_slice(key).map_err(|_| SnapshotError::new("invalid AEAD key"))?;
    let ct = cipher
        .encrypt(
            Nonce::from_slice(nonce),
            Payload {
                msg: &plaintext,
                aad: &aad_bytes,
            },
        )
        .map_err(|_| SnapshotError::new("AEAD encrypt failed"))?;
    if ct.len() < 16 {
        return Err(SnapshotError::new("AEAD ciphertext too short"));
    }
    let (cipher_body, tag) = ct.split_at(ct.len() - 16);
    let mut integrity = [0u8; 32];
    integrity[..16].copy_from_slice(tag);
    Ok(finish_header(
        flags,
        instance_id,
        integrity,
        *nonce,
        cipher_body,
    ))
}

pub fn decode_container(
    bytes: &[u8],
    aead_key: Option<&[u8; 32]>,
) -> Result<DecodedContainer, SnapshotError> {
    if bytes.len() > MAX_BYTES {
        return Err(SnapshotError::new("container too large"));
    }
    let mut r = Reader::new(bytes);
    let magic = r.bytes(4)?;
    if magic != MAGIC {
        return Err(SnapshotError::new("bad container magic"));
    }
    let version = r.u16()?;
    if version != CONTAINER_VERSION {
        return Err(SnapshotError::new(format!(
            "unsupported containerVersion {version}"
        )));
    }
    let flags = r.u16()?;
    if flags & !FLAG_AEAD != 0 {
        return Err(SnapshotError::new("reserved container flags must be 0"));
    }
    let aead = flags & FLAG_AEAD != 0;
    let mut instance_id = [0u8; 16];
    instance_id.copy_from_slice(r.bytes(16)?);
    let mut integrity = [0u8; 32];
    integrity.copy_from_slice(r.bytes(32)?);
    let mut nonce = [0u8; 12];
    nonce.copy_from_slice(r.bytes(12)?);
    let body_len = r.u32()? as usize;
    let body_bytes = r.bytes(body_len)?.to_vec();
    if r.remaining() != 0 {
        return Err(SnapshotError::new("trailing container bytes"));
    }
    let plaintext = if aead {
        let key = aead_key.ok_or_else(|| SnapshotError::new("AEAD container requires key"))?;
        if integrity[16..] != [0u8; 16] {
            return Err(SnapshotError::new("AEAD integrity padding must be zero"));
        }
        let mut sealed = body_bytes.clone();
        sealed.extend_from_slice(&integrity[..16]);
        let cipher =
            Aes256Gcm::new_from_slice(key).map_err(|_| SnapshotError::new("invalid AEAD key"))?;
        let aad_bytes = aad(&instance_id, flags);
        cipher
            .decrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: &sealed,
                    aad: &aad_bytes,
                },
            )
            .map_err(|_| SnapshotError::new("AEAD decrypt failed"))?
    } else {
        if nonce != [0u8; 12] {
            return Err(SnapshotError::new("checksum nonce must be zero"));
        }
        let hash = Sha256::digest(&body_bytes);
        if hash.as_slice() != integrity {
            return Err(SnapshotError::new("container checksum mismatch"));
        }
        body_bytes
    };
    Ok(DecodedContainer {
        instance_id,
        body: decode_body(&plaintext)?,
        aead,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_body() -> ContainerBody {
        ContainerBody {
            tirs: b"TIRS-fixture".to_vec(),
            plugin_state: Vec::new(),
            journal_cursor: 0,
        }
    }

    #[test]
    fn checksum_round_trip() {
        let body = sample_body();
        let bytes = encode_container(&body, [0u8; 16]);
        let decoded = decode_container(&bytes, None).unwrap();
        assert!(!decoded.aead);
        assert_eq!(decoded.body, body);
    }

    #[test]
    fn checksum_mismatch_is_rejected() {
        let mut bytes = encode_container(&sample_body(), [0u8; 16]);
        let last = bytes.len() - 1;
        bytes[last] ^= 1;
        assert!(decode_container(&bytes, None).is_err());
    }

    #[test]
    fn bad_magic_is_rejected() {
        let mut bytes = encode_container(&sample_body(), [0u8; 16]);
        bytes[0] = b'X';
        assert!(decode_container(&bytes, None).is_err());
    }

    #[test]
    fn tirs_magic_is_not_a_container() {
        assert!(decode_container(b"TIRS", None).is_err());
    }

    #[test]
    fn aead_round_trip() {
        let body = sample_body();
        let key = [0x11u8; 32];
        let nonce = [0x22u8; 12];
        let bytes = encode_container_aead(&body, [0u8; 16], &key, &nonce).unwrap();
        let decoded = decode_container(&bytes, Some(&key)).unwrap();
        assert!(decoded.aead);
        assert_eq!(decoded.body, body);
        assert!(decode_container(&bytes, Some(&[0x33u8; 32])).is_err());
    }
}
