//! Bounded streaming PCM WAV/RF64 I/O. No audio devices and no full-waveform buffers.
use super::{
    audio::{check, Codec, Meters},
    model::{Options, Speed},
    protocol::{Collector, Payload, Prepared},
};
use std::{
    fs,
    io::{self, Read, Seek, SeekFrom, Write},
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};
const MAX_WAV: u64 = 64 * 1024 * 1024 * 1024;
pub(super) const RATE: u32 = 48_000;
pub(super) fn planned(
    prepared: &Prepared,
    options: &Options,
    cancel: &AtomicBool,
) -> io::Result<(u64, u64)> {
    let mut codec = Codec::new(RATE, options.speed)?;
    let (samples, frames) = codec.plan(prepared, options.repeats, cancel)?;
    let bytes = samples
        .checked_mul(2)
        .and_then(|n| n.checked_add(80))
        .ok_or_else(|| io::Error::other("WAV size overflow"))?;
    if bytes > MAX_WAV {
        return Err(io::Error::other(
            "Acoustic WAV would exceed 64 GiB; reduce repeats or payload size",
        ));
    }
    Ok((bytes, frames))
}
fn header(bytes: u64) -> Vec<u8> {
    let data = bytes - 80;
    let large = bytes - 8 > u64::from(u32::MAX);
    let mut b = Vec::from(if large {
        b"RF64".as_slice()
    } else {
        b"RIFF".as_slice()
    });
    b.extend(if large { u32::MAX } else { (bytes - 8) as u32 }.to_le_bytes());
    b.extend(b"WAVE");
    if large {
        b.extend(b"ds64");
        b.extend(28_u32.to_le_bytes());
        b.extend((bytes - 8).to_le_bytes());
        b.extend(data.to_le_bytes());
        b.extend((data / 2).to_le_bytes());
        b.extend(0_u32.to_le_bytes());
    } else {
        // A standard JUNK chunk keeps the header size identical, without making
        // ordinary recordings depend on RF64 support in external audio players.
        b.extend(b"JUNK");
        b.extend(28_u32.to_le_bytes());
        b.extend([0_u8; 28]);
    }
    b.extend(b"fmt ");
    b.extend(16_u32.to_le_bytes());
    b.extend(1_u16.to_le_bytes());
    b.extend(1_u16.to_le_bytes());
    b.extend(RATE.to_le_bytes());
    b.extend((RATE * 2).to_le_bytes());
    b.extend(2_u16.to_le_bytes());
    b.extend(16_u16.to_le_bytes());
    b.extend(b"data");
    b.extend(if large { u32::MAX } else { data as u32 }.to_le_bytes());
    b
}

pub(super) fn write(
    out: &mut impl Write,
    prepared: &Prepared,
    options: &Options,
    bytes: u64,
    meters: &Meters,
    cancel: &AtomicBool,
) -> io::Result<[u8; 32]> {
    if options.repeats == 0 {
        return Err(io::Error::other("WAV export needs a finite repeat count"));
    }
    let mut codec = Codec::new(RATE, options.speed)?;
    let mut hash = blake3::Hasher::new();
    let h = header(bytes);
    out.write_all(&h)?;
    hash.update(&h);
    let mut written = 80_u64;
    for _ in 0..options.repeats {
        for frame in prepared.frames() {
            check(cancel, meters)?;
            let wave = codec.encode(&frame)?;
            for samples in wave.chunks(4096) {
                check(cancel, meters)?;
                let mut pcm = [0_u8; 8192];
                for (i, s) in samples.iter().enumerate() {
                    pcm[i * 2..i * 2 + 2].copy_from_slice(
                        &((s.clamp(-1., 1.) * 32767.).round() as i16).to_le_bytes(),
                    );
                }
                let b = &pcm[..samples.len() * 2];
                out.write_all(b)?;
                hash.update(b);
                written += b.len() as u64;
            }
            meters.completed.fetch_add(1, Ordering::Relaxed);
            meters.sent.fetch_add(1, Ordering::Relaxed);
        }
    }
    if written != bytes {
        return Err(io::Error::other(
            "WAV duration changed during export; nothing published",
        ));
    }
    Ok(*hash.finalize().as_bytes())
}
struct Format {
    kind: u16,
    channels: usize,
    rate: u32,
    bits: usize,
    align: usize,
}
fn format(b: &[u8]) -> io::Result<Format> {
    if b.len() < 16 {
        return Err(io::Error::other("Truncated WAV format"));
    }
    let u16at = |i| u16::from_le_bytes([b[i], b[i + 1]]);
    let kind = u16at(0);
    let channels = usize::from(u16at(2));
    let rate = u32::from_le_bytes(b[4..8].try_into().unwrap());
    let align = usize::from(u16at(12));
    let bits = usize::from(u16at(14));
    if !(1..=8).contains(&channels)
        || !(8000..=192000).contains(&rate)
        || !matches!((kind, bits), (1, 16 | 24 | 32) | (3, 32))
        || align != channels * (bits / 8)
        || u32::from_le_bytes(b[8..12].try_into().unwrap()) != rate * align as u32
    {
        return Err(io::Error::other(
            "Use PCM 16/24/32-bit or float32 WAV, 1–8 channels, 8–192 kHz",
        ));
    }
    Ok(Format {
        kind,
        channels,
        rate,
        bits,
        align,
    })
}
fn u32read(r: &mut impl Read) -> io::Result<u32> {
    let mut b = [0; 4];
    r.read_exact(&mut b)?;
    Ok(u32::from_le_bytes(b))
}
fn layout(
    r: &mut (impl Read + Seek),
    len: u64,
    cancel: &AtomicBool,
) -> io::Result<(Format, u64, u64)> {
    let mut h = [0; 12];
    r.read_exact(&mut h)?;
    let rf = &h[..4] == b"RF64";
    if (!rf && &h[..4] != b"RIFF") || &h[8..] != b"WAVE" || len > MAX_WAV {
        return Err(io::Error::other(
            "Choose a WAV/RF64 recording, at most 64 GiB",
        ));
    }
    let mut limit = if rf {
        len
    } else {
        u64::from(u32::from_le_bytes(h[4..8].try_into().unwrap())) + 8
    };
    if limit > len || limit < 12 {
        return Err(io::Error::other("Truncated WAV container"));
    }
    let mut fmt = None;
    let mut ds = None;
    for _ in 0..1024 {
        if cancel.load(Ordering::Relaxed) {
            return Err(io::ErrorKind::Interrupted.into());
        }
        if r.stream_position()?.saturating_add(8) > limit {
            break;
        }
        let mut id = [0; 4];
        r.read_exact(&mut id)?;
        let size = u32read(r)?;
        let at = r.stream_position()?;
        let size = if rf && &id == b"data" && size == u32::MAX {
            ds.ok_or_else(|| io::Error::other("RF64 data precedes its size table"))?
        } else {
            u64::from(size)
        };
        if size > limit - at {
            return Err(io::Error::other("Truncated WAV chunk"));
        }
        if &id == b"ds64" && rf {
            if !(28..=1024).contains(&size) {
                return Err(io::Error::other("Invalid RF64 size table"));
            }
            let mut b = [0; 28];
            r.read_exact(&mut b)?;
            let riff = u64::from_le_bytes(b[..8].try_into().unwrap())
                .checked_add(8)
                .ok_or_else(|| io::Error::other("RF64 size overflow"))?;
            if riff > len || riff < at + size {
                return Err(io::Error::other("Truncated RF64 container"));
            }
            limit = riff;
            ds = Some(u64::from_le_bytes(b[8..16].try_into().unwrap()));
        } else if &id == b"fmt " {
            if size > 4096 {
                return Err(io::Error::other("WAV format too large"));
            }
            let mut b = vec![0; size as usize];
            r.read_exact(&mut b)?;
            fmt = Some(format(&b)?);
        } else if &id == b"data" {
            let fmt = fmt.ok_or_else(|| io::Error::other("WAV data precedes format"))?;
            if size % fmt.align as u64 != 0 {
                return Err(io::Error::other("Incomplete WAV sample frame"));
            }
            return Ok((fmt, at, size));
        }
        let next = at
            .checked_add(size)
            .and_then(|n| n.checked_add(size % 2))
            .filter(|n| *n <= limit)
            .ok_or_else(|| io::Error::other("WAV chunk overflow"))?;
        r.seek(SeekFrom::Start(next))?;
    }
    Err(io::Error::other("No supported WAV audio data"))
}
fn sample(b: &[u8], kind: u16, bits: usize) -> io::Result<f32> {
    let s = match (kind, bits) {
        (1, 16) => f32::from(i16::from_le_bytes(b.try_into().unwrap())) / 32768.,
        (1, 24) => (i32::from_le_bytes([0, b[0], b[1], b[2]]) >> 8) as f32 / 8388608.,
        (1, 32) => i32::from_le_bytes(b.try_into().unwrap()) as f32 / 2147483648.,
        (3, 32) => f32::from_le_bytes(b.try_into().unwrap()),
        _ => unreachable!(),
    };
    if !s.is_finite() {
        return Err(io::Error::other("Non-finite WAV samples"));
    }
    Ok(s.clamp(-1., 1.))
}
pub(super) fn read(
    path: &Path,
    options: &Options,
    meters: &Meters,
    cancel: &AtomicBool,
) -> io::Result<Payload> {
    let mut file = crate::filesystem::open_regular(path)?;
    let original = file.metadata()?;
    let (fmt, at, size) = layout(&mut file, original.len(), cancel)?;
    file.seek(SeekFrom::Start(at))?;
    let mut decoder = ggwave::Decoder::with_protocols(
        ggwave::Parameters {
            sample_rate_inp: fmt.rate as f32,
            ..Default::default()
        },
        &[
            Speed::Normal.protocol(),
            Speed::Fast.protocol(),
            Speed::Fastest.protocol(),
        ],
    )
    .map_err(io::Error::other)?;
    let mut collector = Collector::default();
    let mut remaining = size;
    let mut complete = false;
    while remaining > 0 {
        check(cancel, meters)?;
        let frames = 1024.min((remaining / fmt.align as u64) as usize);
        let mut raw = [0_u8; 32768];
        let n = frames * fmt.align;
        file.read_exact(&mut raw[..n])?;
        remaining -= n as u64;
        let mut mono = [0_f32; 1024];
        for (i, frame) in raw[..n].chunks_exact(fmt.align).enumerate() {
            for value in frame.chunks_exact(fmt.bits / 8) {
                mono[i] += sample(value, fmt.kind, fmt.bits)? / fmt.channels as f32;
            }
        }
        if let Some(packet) = decoder.push(&mono[..frames]) {
            meters.received.fetch_add(1, Ordering::Relaxed);
            complete = collector.ingest(&packet)?;
            let (a, b) = collector.progress();
            meters.completed.store(a, Ordering::Relaxed);
            meters.total.store(b, Ordering::Relaxed);
            if complete {
                break;
            }
        }
    }
    if !complete {
        return collector.finish(options);
    }
    check(cancel, meters)?;
    let after = file.metadata()?;
    if !crate::platform_fs::owns_file(path, &file)
        || crate::platform_fs::directory_link(&fs::symlink_metadata(path)?)
        || original.len() != after.len()
        || original.modified().ok() != after.modified().ok()
    {
        return Err(io::Error::other("WAV recording changed while reading"));
    }
    collector.finish(options)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU64;
    use zeroize::Zeroizing;
    pub(super) struct Temp(pub std::path::PathBuf);
    impl Temp {
        pub(super) fn new() -> io::Result<Self> {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "dn-simplex-wav-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path)?;
            Ok(Self(fs::canonicalize(path)?))
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn options() -> Options {
        Options {
            passphrase: Zeroizing::new("private waveform password".into()),
            speed: Speed::Fastest,
            repeats: 1,
        }
    }
    #[test]
    fn real_modem_wav_export_import_roundtrips_without_devices() -> io::Result<()> {
        let root = Temp::new()?;
        let options = options();
        let payload = Payload::new(None, "Ёжик: https://example.org".as_bytes().to_vec(), None)?;
        let prepared = Prepared::new(&payload, &options)?;
        let cancel = AtomicBool::new(false);
        let meters = Meters::default();
        let (size, _) = planned(&prepared, &options, &cancel)?;
        let path = root.0.join("recording.wav");
        let mut file = fs::File::create(&path)?;
        let expected = write(&mut file, &prepared, &options, size, &meters, &cancel)?;
        drop(file);
        assert_eq!(fs::metadata(&path)?.len(), size);
        assert_eq!(&fs::read(&path)?[..4], b"RIFF");
        assert_eq!(blake3::hash(&fs::read(&path)?).as_bytes(), &expected);
        let got = read(&path, &options, &Meters::default(), &cancel)?;
        assert_eq!(&*got.bytes, &*payload.bytes);
        Ok(())
    }
    #[test]
    fn rf64_supports_long_recordings_and_rejects_truncated_sizes() -> io::Result<()> {
        let bytes = 5_u64 * 1024 * 1024 * 1024;
        let b = header(bytes);
        assert_eq!(b.len(), 80);
        let cancel = AtomicBool::new(false);
        let mut cursor = std::io::Cursor::new(b.clone());
        let (fmt, at, size) = layout(&mut cursor, bytes, &cancel)?;
        assert_eq!((fmt.channels, fmt.bits, at, size), (1, 16, 80, bytes - 80));
        assert!(layout(&mut std::io::Cursor::new(b), 100, &cancel).is_err());
        assert!(layout(&mut std::io::Cursor::new(vec![0; 80]), 80, &cancel).is_err());
        Ok(())
    }
    #[test]
    fn pcm_formats_channel_bounds_non_finite_samples_and_cancellation() {
        assert_eq!(sample(&i16::MIN.to_le_bytes(), 1, 16).unwrap(), -1.);
        assert_eq!(sample(&[0, 0, 128], 1, 24).unwrap(), -1.);
        assert!(sample(&f32::NAN.to_le_bytes(), 3, 32).is_err());
        let mut b = header(80)[56..72].to_vec();
        assert!(format(&b).is_ok());
        b[2..4].copy_from_slice(&9_u16.to_le_bytes());
        assert!(format(&b).is_err());
        let cancel = AtomicBool::new(true);
        assert!(layout(&mut std::io::Cursor::new(header(80)), 80, &cancel).is_err());
    }
}
