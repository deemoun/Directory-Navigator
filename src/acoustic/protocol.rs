//! Stop-and-wait framing. Retries reuse exact bytes; replay filtering precedes Noise.
use std::{
    collections::VecDeque,
    io,
    time::{Duration, Instant},
};
pub const CHUNK: usize = 80;
pub const MESSAGE_LIMIT: usize = 2048;
const PAYLOAD_TIMEOUT: Duration = Duration::from_secs(20 * 60);
const MAGIC: &[u8; 4] = b"DNAC";
#[derive(Clone, Debug, PartialEq, Eq)]
struct Frame {
    session: [u8; 8],
    role: bool,
    ack: bool,
    seq: u16,
    part: u8,
    total: u8,
    payload: Vec<u8>,
}
impl Frame {
    fn encode(&self) -> Vec<u8> {
        let mut b = Vec::with_capacity(104);
        b.extend(MAGIC);
        b.push(1);
        b.push(u8::from(self.role) | u8::from(self.ack) << 1);
        b.extend(self.session);
        b.extend(self.seq.to_le_bytes());
        b.push(self.part);
        b.push(self.total);
        b.extend(&self.payload);
        b.extend(crc32fast::hash(&b).to_le_bytes());
        b
    }
    fn decode(b: &[u8]) -> Option<Self> {
        if !(22..=CHUNK + 22).contains(&b.len()) || &b[..4] != MAGIC || b[4] != 1 || b[5] > 3 {
            return None;
        }
        let end = b.len() - 4;
        if crc32fast::hash(&b[..end]) != u32::from_le_bytes(b[end..].try_into().ok()?) {
            return None;
        }
        let f = Self {
            session: b[6..14].try_into().ok()?,
            role: b[5] & 1 != 0,
            ack: b[5] & 2 != 0,
            seq: u16::from_le_bytes(b[14..16].try_into().ok()?),
            part: b[16],
            total: b[17],
            payload: b[18..end].to_vec(),
        };
        if f.total == 0
            || usize::from(f.total) > MESSAGE_LIMIT.div_ceil(CHUNK)
            || f.part >= f.total
            || (f.ack && f.payload.len() != 4)
        {
            return None;
        }
        Some(f)
    }
    fn acknowledgement(&self) -> Self {
        Self {
            role: !self.role,
            ack: true,
            payload: crc32fast::hash(&self.payload).to_le_bytes().to_vec(),
            ..self.clone()
        }
    }
}
pub trait Radio {
    fn now(&self) -> Instant {
        Instant::now()
    }
    fn send(&mut self, packet: &[u8]) -> io::Result<()>;
    fn receive(&mut self, wait: Duration) -> io::Result<Option<Vec<u8>>>;
}
pub struct Link<R: Radio> {
    pub radio: R,
    role: bool,
    session: Option<[u8; 8]>,
    send_seq: u16,
    recv_seq: u16,
    parts: Vec<Vec<u8>>,
    last: Vec<Frame>,
    ready: VecDeque<Vec<u8>>,
}
impl<R: Radio> Link<R> {
    pub fn new(radio: R, initiator: bool, session: [u8; 8]) -> Self {
        Self {
            radio,
            role: initiator,
            session: initiator.then_some(session),
            send_seq: 0,
            recv_seq: 0,
            parts: vec![],
            last: vec![],
            ready: VecDeque::new(),
        }
    }
    fn ingest(&mut self, bytes: &[u8]) -> io::Result<Option<Frame>> {
        let Some(f) = Frame::decode(bytes) else {
            return Ok(None);
        };
        if f.role == self.role {
            return Ok(None);
        }
        if self.session.is_none() && !f.ack && f.seq == 0 && f.part == 0 {
            self.session = Some(f.session);
        }
        if self.session != Some(f.session) {
            return Ok(None);
        }
        if f.ack {
            return Ok(Some(f));
        }
        if f.seq != self.recv_seq {
            if self.last.iter().any(|old| old == &f) {
                self.radio.send(&f.acknowledgement().encode())?;
            }
            return Ok(None);
        }
        if usize::from(f.part) < self.parts.len() {
            if self.parts[usize::from(f.part)] == f.payload {
                self.radio.send(&f.acknowledgement().encode())?;
            }
            return Ok(None);
        }
        if usize::from(f.part) != self.parts.len()
            || self.parts.len() * CHUNK + f.payload.len() > MESSAGE_LIMIT
        {
            return Ok(None);
        }
        if !self.last.is_empty() && self.last[0].seq == f.seq && self.last[0].total != f.total {
            return Err(io::Error::other("Acoustic fragment count changed"));
        }
        if f.part == 0 {
            self.last.clear();
        }
        self.parts.push(f.payload.clone());
        self.last.push(f.clone());
        self.radio.send(&f.acknowledgement().encode())?;
        if self.parts.len() == usize::from(f.total) {
            if self.ready.len() >= 2 {
                return Err(io::Error::other("Acoustic receive queue limit"));
            }
            self.ready.push_back(self.parts.concat());
            self.parts.clear();
            self.recv_seq = self
                .recv_seq
                .checked_add(1)
                .ok_or_else(|| io::Error::other("Acoustic sequence limit"))?;
        }
        Ok(None)
    }
    pub fn pump(&mut self, wait: Duration) -> io::Result<()> {
        if let Some(bytes) = self.radio.receive(wait)? {
            self.ingest(&bytes)?;
        }
        Ok(())
    }
    pub fn send(&mut self, message: &[u8]) -> io::Result<()> {
        if message.is_empty() || message.len() > MESSAGE_LIMIT {
            return Err(io::Error::other("Acoustic message size limit"));
        }
        let session = self
            .session
            .ok_or_else(|| io::Error::other("Acoustic session not established"))?;
        for (part, payload) in message.chunks(CHUNK).enumerate() {
            let f = Frame {
                session,
                role: self.role,
                ack: false,
                seq: self.send_seq,
                part: part as u8,
                total: message.len().div_ceil(CHUNK) as u8,
                payload: payload.to_vec(),
            };
            let bytes = f.encode();
            let expected = f.acknowledgement();
            let mut acknowledged = false;
            for _ in 0..3 {
                self.radio.send(&bytes)?;
                let until = self.radio.now() + Duration::from_secs(15);
                while self.radio.now() < until {
                    if let Some(packet) = self.radio.receive(Duration::from_millis(20))? {
                        if self.ingest(&packet)?.as_ref() == Some(&expected) {
                            acknowledged = true;
                            break;
                        }
                    }
                }
                if acknowledged {
                    break;
                }
            }
            if !acknowledged {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Acoustic acknowledgement timed out; delivery may be unconfirmed",
                ));
            }
        }
        self.send_seq = self
            .send_seq
            .checked_add(1)
            .ok_or_else(|| io::Error::other("Acoustic sequence limit"))?;
        Ok(())
    }
    pub fn receive(&mut self) -> io::Result<Vec<u8>> {
        self.receive_for(Duration::from_secs(120))
    }
    fn receive_for(&mut self, timeout: Duration) -> io::Result<Vec<u8>> {
        let until = Instant::now() + timeout;
        loop {
            if let Some(b) = self.ready.pop_front() {
                return Ok(b);
            }
            if Instant::now() >= until {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Acoustic reception timed out",
                ));
            }
            self.pump(Duration::from_millis(20))?;
        }
    }
}
pub struct Secure<R: Radio> {
    pub link: Link<R>,
    noise: snow::TransportState,
    pub code: String,
}
fn noise_error(e: snow::Error) -> io::Error {
    io::Error::other(format!("Acoustic authentication: {e}"))
}
impl<R: Radio> Secure<R> {
    pub fn handshake(radio: R, initiator: bool) -> io::Result<Self> {
        let parameters = "Noise_XX_25519_ChaChaPoly_BLAKE2s"
            .parse()
            .map_err(noise_error)?;
        let builder = snow::Builder::new(parameters)
            .prologue(b"DN acoustic protocol 1")
            .map_err(noise_error)?;
        let key = builder.generate_keypair().map_err(noise_error)?;
        let mut session = [0; 8];
        session.copy_from_slice(&blake3::hash(&key.private).as_bytes()[..8]);
        let builder = builder
            .local_private_key(&key.private)
            .map_err(noise_error)?;
        let mut noise = if initiator {
            builder.build_initiator()
        } else {
            builder.build_responder()
        }
        .map_err(noise_error)?;
        let mut link = Link::new(radio, initiator, session);
        let mut buffer = [0; MESSAGE_LIMIT];
        if initiator {
            let n = noise.write_message(&[], &mut buffer).map_err(noise_error)?;
            link.send(&buffer[..n])?;
            noise
                .read_message(&link.receive()?, &mut buffer)
                .map_err(noise_error)?;
            let n = noise.write_message(&[], &mut buffer).map_err(noise_error)?;
            link.send(&buffer[..n])?;
        } else {
            noise
                .read_message(&link.receive()?, &mut buffer)
                .map_err(noise_error)?;
            let n = noise.write_message(&[], &mut buffer).map_err(noise_error)?;
            link.send(&buffer[..n])?;
            noise
                .read_message(&link.receive()?, &mut buffer)
                .map_err(noise_error)?;
        }
        let hash = blake3::derive_key(
            "Directory Navigator acoustic comparison code v1",
            noise.get_handshake_hash(),
        );
        let code = format!(
            "{:08}",
            u64::from_le_bytes(hash[..8].try_into().unwrap()) % 100_000_000
        );
        let noise = noise.into_transport_mode().map_err(noise_error)?;
        Ok(Self { link, noise, code })
    }
    pub fn send(&mut self, plain: &[u8]) -> io::Result<()> {
        let mut out = [0; MESSAGE_LIMIT];
        let n = self
            .noise
            .write_message(plain, &mut out)
            .map_err(noise_error)?;
        self.link.send(&out[..n])
    }
    pub fn receive(&mut self) -> io::Result<Vec<u8>> {
        self.receive_for(Duration::from_secs(120))
    }
    pub fn receive_payload(&mut self) -> io::Result<Vec<u8>> {
        self.receive_for(PAYLOAD_TIMEOUT)
    }
    fn receive_for(&mut self, timeout: Duration) -> io::Result<Vec<u8>> {
        let cipher = self.link.receive_for(timeout)?;
        let mut plain = vec![0; MESSAGE_LIMIT];
        let n = self
            .noise
            .read_message(&cipher, &mut plain)
            .map_err(noise_error)?;
        plain.truncate(n);
        Ok(plain)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn framing_is_bounded_and_detects_corruption() {
        let f = Frame {
            session: [3; 8],
            role: true,
            ack: false,
            seq: 0,
            part: 0,
            total: 1,
            payload: vec![2; 80],
        };
        let b = f.encode();
        assert_eq!(Frame::decode(&b), Some(f));
        for i in 0..b.len() {
            let mut corrupt = b.clone();
            corrupt[i] ^= 1;
            assert!(Frame::decode(&corrupt).is_none());
        }
        assert!(Frame::decode(&[0; 200]).is_none());
    }
    struct Memory {
        tx: std::sync::mpsc::Sender<Vec<u8>>,
        rx: std::sync::mpsc::Receiver<Vec<u8>>,
    }
    impl Radio for Memory {
        fn send(&mut self, b: &[u8]) -> io::Result<()> {
            self.tx.send(b.to_vec()).map_err(io::Error::other)
        }
        fn receive(&mut self, d: Duration) -> io::Result<Option<Vec<u8>>> {
            match self.rx.recv_timeout(d) {
                Ok(b) => Ok(Some(b)),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Ok(None),
                Err(e) => Err(io::Error::other(e)),
            }
        }
    }
    #[test]
    fn encrypted_two_endpoints_compare_and_exchange() {
        let (a, ar) = std::sync::mpsc::channel();
        let (b, br) = std::sync::mpsc::channel();
        let t = std::thread::spawn(move || {
            let mut s = Secure::handshake(Memory { tx: b, rx: ar }, false).unwrap();
            let code = s.code.clone();
            assert_eq!(s.receive().unwrap(), b"confirmed");
            s.send(b"confirmed").unwrap();
            assert_eq!(s.receive().unwrap(), "Привет Ёё".as_bytes());
            s.send(b"retained").unwrap();
            code
        });
        let mut s = Secure::handshake(Memory { tx: a, rx: br }, true).unwrap();
        s.send(b"confirmed").unwrap();
        assert_eq!(s.receive().unwrap(), b"confirmed");
        s.send("Привет Ёё".as_bytes()).unwrap();
        assert_eq!(s.receive().unwrap(), b"retained");
        assert_eq!(s.code, t.join().unwrap());
    }
}

#[cfg(test)]
mod resilience {
    use super::*;
    #[derive(Default)]
    struct Recorder {
        packets: Vec<Vec<u8>>,
    }
    impl Radio for Recorder {
        fn send(&mut self, b: &[u8]) -> io::Result<()> {
            self.packets.push(b.to_vec());
            Ok(())
        }
        fn receive(&mut self, _: Duration) -> io::Result<Option<Vec<u8>>> {
            Ok(None)
        }
    }
    #[test]
    fn duplicates_do_not_advance_message_or_encryption_sequence() {
        let mut l = Link::new(Recorder::default(), false, [0; 8]);
        let f = Frame {
            session: [7; 8],
            role: true,
            ack: false,
            seq: 0,
            part: 0,
            total: 2,
            payload: vec![1; CHUNK],
        };
        l.ingest(&f.encode()).unwrap();
        l.ingest(&f.encode()).unwrap();
        assert_eq!(l.parts.len(), 1);
        let mut changed = f.clone();
        changed.payload[0] ^= 1;
        l.ingest(&changed.encode()).unwrap();
        assert_eq!(l.radio.packets.len(), 2);
        let final_part = Frame {
            part: 1,
            payload: vec![9],
            ..f.clone()
        };
        l.ingest(&final_part.encode()).unwrap();
        assert_eq!(l.recv_seq, 1);
        assert_eq!(l.ready.len(), 1);
        l.ingest(&f.encode()).unwrap();
        l.ingest(&final_part.encode()).unwrap();
        assert_eq!(l.recv_seq, 1);
        assert_eq!(l.ready.len(), 1);
        assert_eq!(l.ready.pop_front().unwrap().len(), CHUNK + 1);
    }
    #[test]
    fn rejects_out_of_order_wrong_session_and_conflicting_fragments() {
        let mut l = Link::new(Recorder::default(), false, [0; 8]);
        let mut f = Frame {
            session: [3; 8],
            role: true,
            ack: false,
            seq: 0,
            part: 0,
            total: 2,
            payload: vec![4; CHUNK],
        };
        l.ingest(&f.encode()).unwrap();
        f.session = [9; 8];
        f.part = 1;
        l.ingest(&f.encode()).unwrap();
        assert_eq!(l.parts.len(), 1);
        f.session = [3; 8];
        f.total = 3;
        assert!(l.ingest(&f.encode()).is_err());
    }
    struct Retry {
        now: Instant,
        sends: Vec<Vec<u8>>,
        ack: bool,
    }
    impl Radio for Retry {
        fn now(&self) -> Instant {
            self.now
        }
        fn send(&mut self, b: &[u8]) -> io::Result<()> {
            self.sends.push(b.to_vec());
            Ok(())
        }
        fn receive(&mut self, _: Duration) -> io::Result<Option<Vec<u8>>> {
            self.now += Duration::from_secs(16);
            Ok(if self.ack && self.sends.len() == 2 {
                Some(
                    Frame::decode(self.sends.last().unwrap())
                        .unwrap()
                        .acknowledgement()
                        .encode(),
                )
            } else {
                None
            })
        }
    }
    #[test]
    fn retries_identical_bytes_and_stops_after_finite_attempts() {
        let mut l = Link::new(
            Retry {
                now: Instant::now(),
                sends: vec![],
                ack: true,
            },
            true,
            [2; 8],
        );
        l.send(b"encrypted packet").unwrap();
        assert_eq!(l.radio.sends.len(), 2);
        assert_eq!(l.radio.sends[0], l.radio.sends[1]);
        let mut l = Link::new(
            Retry {
                now: Instant::now(),
                sends: vec![],
                ack: false,
            },
            true,
            [2; 8],
        );
        assert_eq!(l.send(b"lost").unwrap_err().kind(), io::ErrorKind::TimedOut);
        assert_eq!(l.radio.sends.len(), 3);
    }
    #[test]
    fn ggwave_round_trip_survives_fragmented_audio_without_hardware() {
        let payload = b"DNAC audible interoperability fixture";
        let e = ggwave::Encoder::new(Default::default()).unwrap();
        let mut d =
            ggwave::Decoder::with_protocols(Default::default(), &[ggwave::Protocol::AudibleNormal])
                .unwrap();
        let mut samples = e
            .encode(payload, ggwave::Protocol::AudibleNormal, 10)
            .unwrap();
        samples.resize(samples.len() + 16 * 1024, 0.);
        let found = samples.chunks(317).find_map(|samples| d.push(samples));
        assert_eq!(found.as_deref(), Some(payload.as_slice()));
    }
}

#[cfg(test)]
mod payload_deadline {
    use super::*;
    #[test]
    fn payload_deadline_covers_a_full_offer_at_actual_encoded_wire_length() {
        // Account for three complete attempts per fragment plus ACKs and output padding.
        // This checks waveform sample counts, never elapsed benchmark time.
        let encoder = ggwave::Encoder::new(Default::default()).unwrap();
        let mut seconds = 0.;
        let payload = vec![0; 1024 + 1 + 16];
        for (part, chunk) in payload.chunks(CHUNK).enumerate() {
            let f = Frame {
                session: [1; 8],
                role: true,
                ack: false,
                seq: 3,
                part: part as u8,
                total: payload.len().div_ceil(CHUNK) as u8,
                payload: chunk.to_vec(),
            };
            let data = encoder
                .encode(&f.encode(), ggwave::Protocol::AudibleNormal, 10)
                .unwrap();
            let ack = encoder
                .encode(
                    &f.acknowledgement().encode(),
                    ggwave::Protocol::AudibleNormal,
                    10,
                )
                .unwrap();
            seconds += (data.len() + ack.len() + 9600) as f64 / 48000.;
        }
        assert!(PAYLOAD_TIMEOUT.as_secs_f64() > seconds * 3.);
    }
}
