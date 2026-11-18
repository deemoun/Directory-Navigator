//! Version 2 simplex carousel. No handshakes, acknowledgements or peer replies.
//! CRC filters damaged frames; the complete bounded envelope is authenticated
//! before decompression, filenames, offers or plaintext reach the host.
use super::model::{Options, MAX_BYTES};
use chacha20poly1305::{
    aead::{Aead, Payload as AeadPayload},
    KeyInit, XChaCha20Poly1305,
};
use std::io::{self, Read, Write};
use zeroize::Zeroizing;
pub(super) const CHUNK: usize = 112;
const WIRE_LIMIT: usize = MAX_BYTES + 4096;
const MAGIC: &[u8; 4] = b"DNA2";
const AAD: &[u8] = b"Directory Navigator acoustic simplex 2";
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Offer {
    pub name: Option<String>,
    pub size: usize,
    pub hash: [u8; 32],
    pub modified: Option<i64>,
    compressed: bool,
}
impl Offer {
    pub fn validate(&self) -> io::Result<()> {
        if self.size > MAX_BYTES {
            return Err(io::Error::other("Acoustic payload limit is 1 MiB"));
        }
        if let Some(name) = &self.name {
            if name.len() > 255
                || name.contains(['/', '\\'])
                || crate::peer::manifest::relative(name).is_err()
            {
                return Err(io::Error::other("Acoustic filename is not portable"));
            }
        }
        Ok(())
    }
}
#[derive(Clone)]
pub(super) struct Payload {
    pub offer: Offer,
    pub bytes: Zeroizing<Vec<u8>>,
}
impl Payload {
    pub fn new(name: Option<String>, bytes: Vec<u8>, modified: Option<i64>) -> io::Result<Self> {
        let offer = Offer {
            name,
            size: bytes.len(),
            hash: *blake3::hash(&bytes).as_bytes(),
            modified,
            compressed: false,
        };
        offer.validate()?;
        if offer.name.is_none() && std::str::from_utf8(&bytes).is_err() {
            return Err(io::Error::other("Acoustic text must be UTF-8"));
        }
        Ok(Self {
            offer,
            bytes: Zeroizing::new(bytes),
        })
    }
}
#[derive(Clone, PartialEq, Eq)]
struct Descriptor {
    id: [u8; 8],
    salt: [u8; 16],
    nonce: [u8; 24],
    len: u32,
    hash: [u8; 32],
}
impl Descriptor {
    fn encode(&self) -> Vec<u8> {
        let mut out = Vec::from(MAGIC.as_slice());
        out.push(0);
        out.extend(self.id);
        out.extend(self.salt);
        out.extend(self.nonce);
        out.extend(self.len.to_le_bytes());
        out.extend(self.hash);
        crc(out)
    }
    fn decode(b: &[u8]) -> Option<Self> {
        if b.len() != 93 || &b[..4] != MAGIC || b[4] != 0 || !valid_crc(b) {
            return None;
        }
        let v = Self {
            id: b[5..13].try_into().ok()?,
            salt: b[13..29].try_into().ok()?,
            nonce: b[29..53].try_into().ok()?,
            len: u32::from_le_bytes(b[53..57].try_into().ok()?),
            hash: b[57..89].try_into().ok()?,
        };
        (v.len >= 20 && v.len as usize <= WIRE_LIMIT).then_some(v)
    }
    fn aad(&self) -> Vec<u8> {
        let mut v = AAD.to_vec();
        v.extend(self.id);
        v.extend(self.salt);
        v
    }
}
fn crc(mut b: Vec<u8>) -> Vec<u8> {
    let c = crc32fast::hash(&b);
    b.extend(c.to_le_bytes());
    b
}
fn valid_crc(b: &[u8]) -> bool {
    b.len() >= 4
        && crc32fast::hash(&b[..b.len() - 4])
            == u32::from_le_bytes(b[b.len() - 4..].try_into().unwrap())
}
fn key(passphrase: &str, salt: &[u8]) -> io::Result<Zeroizing<[u8; 32]>> {
    let mut out = Zeroizing::new([0; 32]);
    argon2::Argon2::new(
        argon2::Algorithm::Argon2id,
        argon2::Version::V0x13,
        argon2::Params::new(19 * 1024, 2, 1, Some(32))
            .map_err(|_| io::Error::other("Invalid acoustic KDF parameters"))?,
    )
    .hash_password_into(passphrase.as_bytes(), salt, out.as_mut())
    .map_err(|_| io::Error::other("Acoustic key derivation failed"))?;
    Ok(out)
}
pub(super) struct Prepared {
    descriptor: Descriptor,
    encrypted: Vec<u8>,
}
impl Prepared {
    pub fn new(payload: &Payload, options: &Options) -> io::Result<Self> {
        options.validate()?;
        payload.offer.validate()?;
        let mut zipped = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
        zipped.write_all(&payload.bytes)?;
        let zipped = Zeroizing::new(zipped.finish()?);
        let mut offer = payload.offer.clone();
        offer.compressed = zipped.len() < payload.bytes.len();
        let header = serde_json::to_vec(&offer).map_err(io::Error::other)?;
        if header.len() > 2048 {
            return Err(io::Error::other("Acoustic manifest too large"));
        }
        let mut plain = Zeroizing::new(Vec::with_capacity(4 + header.len() + payload.bytes.len()));
        plain.extend((header.len() as u32).to_le_bytes());
        plain.extend(header);
        plain.extend_from_slice(if offer.compressed {
            &zipped
        } else {
            &payload.bytes
        });
        let mut d = Descriptor {
            id: [0; 8],
            salt: [0; 16],
            nonce: [0; 24],
            len: 0,
            hash: [0; 32],
        };
        for random in [&mut d.id[..], &mut d.salt[..], &mut d.nonce[..]] {
            getrandom::fill(random).map_err(io::Error::other)?;
        }
        let key = key(&options.passphrase, &d.salt)?;
        let encrypted = XChaCha20Poly1305::new(key.as_ref().into())
            .encrypt(
                (&d.nonce).into(),
                AeadPayload {
                    msg: &plain,
                    aad: &d.aad(),
                },
            )
            .map_err(|_| io::Error::other("Acoustic encryption failed"))?;
        if encrypted.len() > WIRE_LIMIT {
            return Err(io::Error::other("Acoustic envelope too large"));
        }
        d.len = encrypted.len() as u32;
        d.hash = *blake3::hash(&encrypted).as_bytes();
        Ok(Self {
            descriptor: d,
            encrypted,
        })
    }
    pub fn blocks(&self) -> usize {
        self.encrypted.len().div_ceil(CHUNK)
    }
    /// A header every sixteen blocks permits joining in the middle of playback.
    pub fn frames(&self) -> impl Iterator<Item = Vec<u8>> + '_ {
        (0..self.blocks())
            .flat_map(|index| {
                let mut out = Vec::from(MAGIC.as_slice());
                out.push(1);
                out.extend(self.descriptor.id);
                out.extend((index as u32).to_le_bytes());
                out.extend_from_slice(
                    &self.encrypted[index * CHUNK..((index + 1) * CHUNK).min(self.encrypted.len())],
                );
                (index % 16 == 0)
                    .then(|| self.descriptor.encode())
                    .into_iter()
                    .chain(Some(crc(out)))
            })
            .chain(Some(self.descriptor.encode()))
    }
}
#[derive(Default)]
pub(super) struct Collector {
    descriptor: Option<Descriptor>,
    parts: Vec<Option<Vec<u8>>>,
    received: usize,
}
impl Collector {
    pub fn progress(&self) -> (u64, u64) {
        (self.received as u64, self.parts.len() as u64)
    }
    pub fn ingest(&mut self, b: &[u8]) -> io::Result<bool> {
        if let Some(d) = Descriptor::decode(b) {
            if let Some(old) = &self.descriptor {
                if old != &d {
                    return Ok(false);
                }
            } else {
                self.parts = vec![None; (d.len as usize).div_ceil(CHUNK)];
                self.descriptor = Some(d);
            }
            return Ok(false);
        }
        let Some(d) = &self.descriptor else {
            return Ok(false);
        };
        if b.len() < 22
            || b.len() > CHUNK + 21
            || &b[..4] != MAGIC
            || b[4] != 1
            || b[5..13] != d.id
            || !valid_crc(b)
        {
            return Ok(false);
        }
        let index = u32::from_le_bytes(b[13..17].try_into().unwrap()) as usize;
        if index >= self.parts.len() {
            return Ok(false);
        }
        let data = &b[17..b.len() - 4];
        if data.len() != CHUNK.min(d.len as usize - index * CHUNK) {
            return Ok(false);
        }
        match &self.parts[index] {
            Some(old) if old != data => {
                return Err(io::Error::other(
                    "Conflicting acoustic blocks; stop and receive again",
                ))
            }
            Some(_) => return Ok(false),
            None => {
                self.parts[index] = Some(data.to_vec());
                self.received += 1;
            }
        }
        Ok(self.received == self.parts.len())
    }
    pub fn finish(self, options: &Options) -> io::Result<Payload> {
        options.validate()?;
        let d = self
            .descriptor
            .ok_or_else(|| io::Error::other("No acoustic header received"))?;
        if self.received != self.parts.len() {
            return Err(io::Error::other(
                "Recording ended with missing blocks; nothing retained. Record another repetition.",
            ));
        }
        let bytes = self
            .parts
            .into_iter()
            .flatten()
            .flatten()
            .collect::<Vec<_>>();
        if blake3::hash(&bytes).as_bytes() != &d.hash {
            return Err(io::Error::other(
                "Acoustic checksum failed; nothing retained",
            ));
        }
        let key = key(&options.passphrase, &d.salt)?;
        let plain = Zeroizing::new(
            XChaCha20Poly1305::new(key.as_ref().into())
                .decrypt(
                    (&d.nonce).into(),
                    AeadPayload {
                        msg: &bytes,
                        aad: &d.aad(),
                    },
                )
                .map_err(|_| {
                    io::Error::other("Incorrect passphrase or damaged recording; nothing retained")
                })?,
        );
        if plain.len() < 4 {
            return Err(io::Error::other("Malformed acoustic envelope"));
        }
        let n = u32::from_le_bytes(plain[..4].try_into().unwrap()) as usize;
        if n > 2048 || n > plain.len() - 4 {
            return Err(io::Error::other("Invalid acoustic manifest length"));
        }
        let offer: Offer = serde_json::from_slice(&plain[4..4 + n]).map_err(io::Error::other)?;
        offer.validate()?;
        let data = &plain[4 + n..];
        let bytes = if offer.compressed {
            let mut decoder = flate2::read::ZlibDecoder::new(data);
            let mut out = Zeroizing::new(Vec::new());
            (&mut decoder)
                .take((offer.size + 1) as u64)
                .read_to_end(&mut out)?;
            if decoder.total_in() as usize != data.len() {
                return Err(io::Error::other("Invalid compressed acoustic payload"));
            }
            out
        } else {
            Zeroizing::new(data.to_vec())
        };
        if bytes.len() != offer.size || blake3::hash(&bytes).as_bytes() != &offer.hash {
            return Err(io::Error::other(
                "Acoustic payload checksum or size failed; nothing retained",
            ));
        }
        if offer.name.is_none() {
            std::str::from_utf8(&bytes).map_err(io::Error::other)?;
        }
        Ok(Payload { offer, bytes })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn options() -> Options {
        Options {
            passphrase: Zeroizing::new("private fixture passphrase".into()),
            ..Default::default()
        }
    }
    fn collect(p: &Prepared, o: &Options) -> io::Result<Payload> {
        let mut c = Collector::default();
        for frame in p.frames() {
            c.ingest(&frame)?;
        }
        c.finish(o)
    }
    #[test]
    fn one_way_blocks_recover_late_join_loss_duplicates_and_reordering() -> io::Result<()> {
        let o = options();
        let bytes = (0_u32..4000).flat_map(u32::to_le_bytes).collect::<Vec<_>>();
        let payload = Payload::new(Some("Ёжик.bin".into()), bytes.clone(), None)?;
        let p = Prepared::new(&payload, &o)?;
        let frames = p.frames().collect::<Vec<_>>();
        assert!(frames
            .iter()
            .all(|f| f.len() <= ggwave::MAX_LENGTH_VARIABLE));
        let mut c = Collector::default();
        // Start mid-cycle: early data has no descriptor and is safely ignored.
        for (i, f) in frames.iter().enumerate().skip(5) {
            if i % 3 != 0 {
                c.ingest(f)?;
                c.ingest(f)?;
            }
        }
        for f in frames.iter().rev() {
            c.ingest(f)?;
        }
        assert_eq!(c.progress().0, c.progress().1);
        let got = c.finish(&o)?;
        assert_eq!(&*got.bytes, &bytes);
        assert_eq!(got.offer.name.as_deref(), Some("Ёжик.bin"));
        Ok(())
    }
    #[test]
    fn wrong_passphrase_and_corruption_never_return_plaintext() -> io::Result<()> {
        let o = options();
        let p = Prepared::new(&Payload::new(None, b"private text".to_vec(), None)?, &o)?;
        let mut wrong = options();
        wrong.passphrase = Zeroizing::new("different private password".into());
        assert!(collect(&p, &wrong)
            .err()
            .unwrap()
            .to_string()
            .contains("passphrase"));
        let mut c = Collector::default();
        for mut f in p.frames() {
            if f[4] == 1 {
                f[17] ^= 1;
                let n = f.len();
                let hash = crc32fast::hash(&f[..n - 4]);
                f[n - 4..].copy_from_slice(&hash.to_le_bytes());
            }
            c.ingest(&f)?;
        }
        assert!(c.finish(&o).err().unwrap().to_string().contains("checksum"));
        Ok(())
    }
    #[test]
    fn incomplete_invalid_and_foreign_frames_remain_bounded() -> io::Result<()> {
        let o = options();
        let p = Prepared::new(
            &Payload::new(Some("test.bin".into()), vec![1; 20_000], None)?,
            &o,
        )?;
        let mut c = Collector::default();
        let header = p.descriptor.encode();
        c.ingest(&header)?;
        for n in 0..header.len() {
            assert!(!c.ingest(&header[..n])?);
        }
        let mut oversized = p.descriptor.clone();
        oversized.len = u32::MAX;
        assert!(Descriptor::decode(&oversized.encode()).is_none());
        assert!(c.progress().1 <= WIRE_LIMIT.div_ceil(CHUNK) as u64);
        let foreign = Prepared::new(&Payload::new(None, b"foreign".to_vec(), None)?, &o)?;
        for f in foreign.frames() {
            assert!(!c.ingest(&f)?);
        }
        assert_eq!(c.progress().0, 0);
        assert!(c
            .finish(&o)
            .err()
            .unwrap()
            .to_string()
            .contains("missing blocks"));
        Ok(())
    }
    #[test]
    fn larger_payloads_compress_and_roundtrip_without_raising_decoded_limits() -> io::Result<()> {
        let o = options();
        let bytes = "Ёжик\r\n"
            .as_bytes()
            .iter()
            .copied()
            .cycle()
            .take(MAX_BYTES - 8)
            .collect::<Vec<_>>();
        let payload = Payload::new(Some("large.txt".into()), bytes.clone(), Some(123))?;
        let p = Prepared::new(&payload, &o)?;
        assert!(p.encrypted.len() < bytes.len() / 10);
        let got = collect(&p, &o)?;
        assert_eq!(&*got.bytes, &bytes);
        assert_eq!(got.offer.modified, Some(123));
        assert!(Payload::new(Some("big".into()), vec![0; MAX_BYTES + 1], None).is_err());
        for name in ["../bad", "a/b", "a\\b", "CON", "."] {
            assert!(Payload::new(Some(name.into()), vec![], None).is_err());
        }
        let empty = Payload::new(Some("empty.bin".into()), vec![], None)?;
        assert!(collect(&Prepared::new(&empty, &o)?, &o)?.bytes.is_empty());
        Ok(())
    }
    #[test]
    fn malformed_authenticated_manifests_and_compression_bombs_are_refused() -> io::Result<()> {
        let o = options();
        let payload = Payload::new(Some("file.bin".into()), vec![0; 64_000], None)?;
        let mut p = Prepared::new(&payload, &o)?;
        let mut offer = payload.offer.clone();
        offer.size = 1;
        offer.compressed = true;
        let json = serde_json::to_vec(&offer).unwrap();
        let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
        encoder.write_all(&payload.bytes)?;
        let mut plain = (json.len() as u32).to_le_bytes().to_vec();
        plain.extend(json);
        plain.extend(encoder.finish()?);
        let k = key(&o.passphrase, &p.descriptor.salt)?;
        p.encrypted = XChaCha20Poly1305::new(k.as_ref().into())
            .encrypt(
                (&p.descriptor.nonce).into(),
                AeadPayload {
                    msg: &plain,
                    aad: &p.descriptor.aad(),
                },
            )
            .unwrap();
        p.descriptor.len = p.encrypted.len() as u32;
        p.descriptor.hash = *blake3::hash(&p.encrypted).as_bytes();
        assert!(collect(&p, &o).is_err());
        Ok(())
    }
    #[test]
    fn old_bidirectional_packets_are_not_interpreted_as_simplex() {
        let mut c = Collector::default();
        assert!(!c.ingest(b"DNAC\x01old protocol reply").unwrap());
        assert_eq!(c.progress(), (0, 0));
    }
}
