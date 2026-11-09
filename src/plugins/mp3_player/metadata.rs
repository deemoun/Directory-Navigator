//! Read-only, bounded music metadata. Audio samples and pictures are never decoded.
use super::tags::Tags;
use crate::task::Cancellation;
use std::{
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    time::Duration,
};
const MAX_TAG: usize = 1024 * 1024;
const MAX_CHUNKS: usize = 65536;
#[derive(Default)]
pub(super) struct Info {
    pub tags: Tags,
    pub duration: Option<Duration>,
}
fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "Malformed music metadata")
}
fn unsupported(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::Unsupported, message)
}
fn samples(count: u64, rate: u32) -> Option<Duration> {
    if rate == 0 || count == 0 {
        return None;
    }
    let rate = u64::from(rate);
    Some(
        Duration::from_secs(count / rate)
            + Duration::from_nanos(count % rate * 1_000_000_000 / rate),
    )
}
fn clean(value: &[u8]) -> String {
    String::from_utf8_lossy(value)
        .trim_end_matches('\0')
        .chars()
        .take(512)
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .trim()
        .into()
}
fn field(tags: &mut Tags, key: &[u8], value: &[u8]) {
    let destination = if key.eq_ignore_ascii_case(b"TITLE") || key == b"INAM" {
        &mut tags.title
    } else if key.eq_ignore_ascii_case(b"ARTIST") || key == b"IART" {
        &mut tags.artist
    } else if key.eq_ignore_ascii_case(b"ALBUM") || key == b"IPRD" {
        &mut tags.album
    } else {
        return;
    };
    if destination.is_empty() {
        *destination = clean(value);
    }
}
fn le32(bytes: &[u8]) -> u32 {
    u32::from_le_bytes(bytes.try_into().unwrap())
}
fn take<'a>(data: &mut &'a [u8], n: usize) -> io::Result<&'a [u8]> {
    if data.len() < n {
        return Err(invalid());
    }
    let (first, rest) = data.split_at(n);
    *data = rest;
    Ok(first)
}
fn comments(mut data: &[u8], tags: &mut Tags, cancel: &Cancellation) -> io::Result<()> {
    let vendor = le32(take(&mut data, 4)?) as usize;
    take(&mut data, vendor)?;
    let count = le32(take(&mut data, 4)?);
    if count > MAX_CHUNKS as u32 {
        return Err(invalid());
    }
    for _ in 0..count {
        cancel.check()?;
        let length = le32(take(&mut data, 4)?) as usize;
        let value = take(&mut data, length)?;
        if let Some(at) = value.iter().position(|b| *b == b'=') {
            field(tags, &value[..at], &value[at + 1..]);
        }
    }
    Ok(())
}
pub(super) fn read(file: &mut File, cancel: &Cancellation) -> io::Result<Info> {
    cancel.check()?;
    file.rewind()?;
    let mut header = [0; 12];
    let n = file.read(&mut header)?;
    file.rewind()?;
    if n >= 4 && &header[..4] == b"fLaC" {
        return flac(file, cancel);
    }
    if n >= 12 && &header[..4] == b"RIFF" && &header[8..12] == b"WAVE" {
        return wav(file, cancel);
    }
    if n >= 4 && &header[..4] == b"OggS" {
        return vorbis(file, cancel);
    }
    // Keep the established tolerant MP3 import and filename fallback behavior.
    Ok(Info {
        tags: super::tags::read(file)?,
        duration: super::duration::probe(file, cancel)?,
    })
}
fn flac(file: &mut File, cancel: &Cancellation) -> io::Result<Info> {
    let length = file.metadata()?.len();
    file.seek(SeekFrom::Start(4))?;
    let mut info = Info::default();
    for index in 0..MAX_CHUNKS {
        cancel.check()?;
        let mut header = [0; 4];
        file.read_exact(&mut header)?;
        let size = header[1..]
            .iter()
            .fold(0usize, |n, b| (n << 8) | usize::from(*b));
        let start = file.stream_position()?;
        let end = start
            .checked_add(size as u64)
            .filter(|end| *end <= length)
            .ok_or_else(invalid)?;
        let kind = header[0] & 127;
        if index == 0 && (kind != 0 || size != 34) {
            return Err(invalid());
        }
        match kind {
            0 if size == 34 && index == 0 => {
                let mut stream = [0; 34];
                file.read_exact(&mut stream)?;
                let bits = u64::from_be_bytes(stream[10..18].try_into().unwrap());
                let rate = (bits >> 44) as u32;
                if rate == 0 {
                    return Err(invalid());
                }
                info.duration = samples(bits & 0x0f_ffff_ffff, rate);
            }
            4 if size <= MAX_TAG => {
                let mut data = vec![0; size];
                file.read_exact(&mut data)?;
                let mut tags = Tags::default();
                if comments(&data, &mut tags, cancel).is_ok() {
                    info.tags = tags;
                }
                cancel.check()?;
            }
            127 | 0 => return Err(invalid()),
            _ => {}
        }
        file.seek(SeekFrom::Start(end))?;
        if header[0] & 128 != 0 {
            return Ok(info);
        }
    }
    Err(invalid())
}
fn wav(file: &mut File, cancel: &Cancellation) -> io::Result<Info> {
    let length = file.metadata()?.len();
    let mut header = [0; 12];
    file.read_exact(&mut header)?;
    let end = u64::from(le32(&header[4..8])) + 8;
    if end > length || end < 12 {
        return Err(invalid());
    }
    let mut info = Info::default();
    let mut format = None;
    let mut data_size = None;
    for _ in 0..MAX_CHUNKS {
        cancel.check()?;
        let position = file.stream_position()?;
        if position == end {
            if let (Some((rate, align)), Some(bytes)) = (format, data_size) {
                if bytes % u64::from(align) != 0 {
                    return Err(invalid());
                }
                info.duration = samples(bytes / u64::from(align), rate);
            }
            return Ok(info);
        }
        if position + 8 > end {
            return Err(invalid());
        }
        let mut chunk = [0; 8];
        file.read_exact(&mut chunk)?;
        let size = le32(&chunk[4..]) as usize;
        let start = file.stream_position()?;
        let next = start
            .checked_add(size as u64)
            .filter(|p| *p <= end)
            .ok_or_else(invalid)?;
        match &chunk[..4] {
            b"fmt " => {
                if size < 16 {
                    return Err(invalid());
                }
                let mut fmt = [0; 40];
                let n = size.min(40);
                file.read_exact(&mut fmt[..n])?;
                let mut code = u16::from_le_bytes(fmt[..2].try_into().unwrap());
                if code == 0xfffe {
                    if size < 40
                        || fmt[26..40] != [0, 0, 0, 0, 16, 0, 128, 0, 0, 170, 0, 56, 155, 113]
                    {
                        return Err(unsupported("Unsupported WAV encoding"));
                    }
                    code = u16::from_le_bytes(fmt[24..26].try_into().unwrap());
                }
                if ![1, 3].contains(&code) {
                    return Err(unsupported("Only PCM and floating-point WAV are supported"));
                }
                let channels = u16::from_le_bytes(fmt[2..4].try_into().unwrap());
                let rate = le32(&fmt[4..8]);
                let align = u16::from_le_bytes(fmt[12..14].try_into().unwrap());
                let bits = u16::from_le_bytes(fmt[14..16].try_into().unwrap());
                if channels == 0
                    || rate == 0
                    || align == 0
                    || u32::from(align) != u32::from(channels) * u32::from(bits.div_ceil(8))
                {
                    return Err(invalid());
                }
                if (code == 1 && ![8, 16, 24, 32].contains(&bits))
                    || (code == 3 && ![32, 64].contains(&bits))
                {
                    return Err(unsupported("Unsupported WAV sample format"));
                }
                format = Some((rate, align));
            }
            b"data" if data_size.is_none() => data_size = Some(size as u64),
            b"LIST" if size <= MAX_TAG => {
                let mut data = vec![0; size];
                file.read_exact(&mut data)?;
                if data.starts_with(b"INFO") {
                    let mut rest = &data[4..];
                    while rest.len() >= 8 {
                        cancel.check()?;
                        let h = take(&mut rest, 8)?;
                        let n = le32(&h[4..]) as usize;
                        let value = take(&mut rest, n)?;
                        field(&mut info.tags, &h[..4], value);
                        if !n.is_multiple_of(2) {
                            take(&mut rest, 1)?;
                        }
                    }
                }
            }
            b"ID3 " | b"id3 " if size <= MAX_TAG => {
                let mut data = vec![0; size];
                file.read_exact(&mut data)?;
                let tags = super::tags::read(&mut io::Cursor::new(data))?;
                if info.tags.title.is_empty() {
                    info.tags.title = tags.title;
                }
                if info.tags.artist.is_empty() {
                    info.tags.artist = tags.artist;
                }
                if info.tags.album.is_empty() {
                    info.tags.album = tags.album;
                }
            }
            _ => {}
        }
        let padded = next + (size as u64 % 2);
        if padded > end {
            return Err(invalid());
        }
        file.seek(SeekFrom::Start(padded))?;
    }
    Err(invalid())
}
const fn crc_table() -> [u32; 256] {
    let mut table = [0; 256];
    let mut i = 0;
    while i < 256 {
        let mut value = (i as u32) << 24;
        let mut bit = 0;
        while bit < 8 {
            value = if value & 0x8000_0000 != 0 {
                (value << 1) ^ 0x04c1_1db7
            } else {
                value << 1
            };
            bit += 1;
        }
        table[i] = value;
        i += 1;
    }
    table
}
fn ogg_crc(parts: &[&[u8]]) -> u32 {
    const TABLE: [u32; 256] = crc_table();
    let mut crc = 0u32;
    for bytes in parts {
        for byte in *bytes {
            crc = (crc << 8) ^ TABLE[usize::from((crc >> 24) as u8 ^ *byte)];
        }
    }
    crc
}
fn vorbis(file: &mut File, cancel: &Cancellation) -> io::Result<Info> {
    let length = file.metadata()?.len();
    let mut info = Info::default();
    let mut stream = None;
    let mut sequence = 0u32;
    let mut continued = false;
    let mut packet = Vec::new();
    let mut discarded = false;
    let mut packets = 0usize;
    let mut rate = 0;
    let mut last_granule = 0;
    loop {
        cancel.check()?;
        if file.stream_position()? == length {
            return Ok(info);
        }
        let mut page = [0; 27];
        file.read_exact(&mut page)?;
        if &page[..4] != b"OggS" || page[4] != 0 {
            return Err(invalid());
        }
        let serial = le32(&page[14..18]);
        if stream.is_none() {
            if page[5] & 2 == 0 {
                return Err(invalid());
            }
            stream = Some(serial);
        }
        if stream != Some(serial) {
            return Err(unsupported(
                "Chained and multiplexed Ogg streams are unsupported",
            ));
        }
        if le32(&page[18..22]) != sequence || (page[5] & 1 != 0) != continued {
            return Err(invalid());
        }
        sequence = sequence.wrapping_add(1);
        let mut lacing = vec![0; usize::from(page[26])];
        file.read_exact(&mut lacing)?;
        let size = lacing.iter().map(|n| u64::from(*n)).sum::<u64>();
        let start = file.stream_position()?;
        if start + size > length {
            return Err(invalid());
        }
        let mut body = vec![0; size as usize];
        file.read_exact(&mut body)?;
        let expected = le32(&page[22..26]);
        page[22..26].fill(0);
        if ogg_crc(&[&page, &lacing, &body]) != expected {
            return Err(invalid());
        }
        if packets < 2 {
            let mut position = 0;
            for n in &lacing {
                let end = position + usize::from(*n);
                if packets < 2 {
                    if !discarded && packet.len() + usize::from(*n) > MAX_TAG {
                        if packets == 0 {
                            return Err(invalid());
                        }
                        packet.clear();
                        discarded = true;
                    }
                    if !discarded {
                        packet.extend_from_slice(&body[position..end]);
                    }
                }
                position = end;
                if *n < 255 {
                    if packets == 0 {
                        if packet.len() < 30 || !packet.starts_with(b"\x01vorbis") {
                            return Err(unsupported("Only Ogg Vorbis audio is supported"));
                        }
                        if le32(&packet[7..11]) != 0 || packet[11] == 0 || packet[29] & 1 == 0 {
                            return Err(invalid());
                        }
                        rate = le32(&packet[12..16]);
                        if rate == 0 {
                            return Err(invalid());
                        }
                    } else if packets == 1 && !discarded {
                        if !packet.starts_with(b"\x03vorbis") {
                            return Err(invalid());
                        }
                        let mut tags = Tags::default();
                        if comments(&packet[7..], &mut tags, cancel).is_ok() {
                            info.tags = tags;
                        }
                        cancel.check()?;
                    }
                    packet.clear();
                    discarded = false;
                    packets = packets.saturating_add(1);
                }
            }
        }
        if let Some(last) = lacing.last() {
            continued = *last == 255;
        }
        let granule = u64::from_le_bytes(page[6..14].try_into().unwrap());
        if granule != u64::MAX {
            if granule < last_granule {
                return Err(invalid());
            }
            last_granule = granule;
        }
        if page[5] & 4 != 0 {
            if continued || packets < 2 || granule == u64::MAX {
                return Err(invalid());
            }
            info.duration = samples(granule, rate);
            if file.stream_position()? != length {
                return Err(unsupported("Chained Ogg streams are unsupported"));
            }
            return Ok(info);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };
    struct Fixture(PathBuf);
    impl Fixture {
        fn new(bytes: &[u8]) -> io::Result<Self> {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "dn-music-meta-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            use std::io::Write;
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)?;
            file.write_all(bytes)?;
            Ok(Self(path))
        }
        fn read(&self) -> io::Result<Info> {
            read(&mut File::open(&self.0)?, &Cancellation::default())
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }
    const FLAC: &[u8] = include_bytes!("../../../tests/fixtures/music/tone.flac");
    const WAV: &[u8] = include_bytes!("../../../tests/fixtures/music/tone.wav");
    const OGG: &[u8] = include_bytes!("../../../tests/fixtures/music/tone.ogg");
    #[test]
    fn cyrillic_flac_and_vorbis_comments_keep_exact_unicode_without_audio() -> io::Result<()> {
        let mut comments = Vec::new();
        comments.extend(0u32.to_le_bytes()); // Empty vendor.
        comments.extend(3u32.to_le_bytes());
        for text in [
            "TITLE=Ёжик Mix е\u{308}",
            "ARTIST=Группа Ёлка",
            "ALBUM=Альбом Ёё",
        ] {
            comments.extend((text.len() as u32).to_le_bytes());
            comments.extend(text.as_bytes());
        }
        let mut stream = [0u8; 34];
        stream[10..18].copy_from_slice(&((44100u64 << 44) | 44100).to_be_bytes());
        let mut flac = b"fLaC\0\0\0\x22".to_vec();
        flac.extend(stream);
        flac.push(0x84);
        flac.extend(&(comments.len() as u32).to_be_bytes()[1..]);
        flac.extend(&comments);
        let mut identification = vec![0u8; 30];
        identification[..7].copy_from_slice(b"\x01vorbis");
        identification[11] = 1;
        identification[12..16].copy_from_slice(&44100u32.to_le_bytes());
        identification[29] = 1;
        let mut comment = b"\x03vorbis".to_vec();
        comment.extend(comments);
        comment.push(1);
        let mut ogg = Vec::new();
        for (sequence, packet) in [identification, comment].iter().enumerate() {
            assert!(packet.len() < 255);
            let mut page = [0u8; 27];
            page[..4].copy_from_slice(b"OggS");
            page[5] = if sequence == 0 { 2 } else { 4 };
            page[14..18].copy_from_slice(&7u32.to_le_bytes());
            page[18..22].copy_from_slice(&(sequence as u32).to_le_bytes());
            page[26] = 1;
            let lacing = [packet.len() as u8];
            let crc = ogg_crc(&[&page, &lacing, packet]);
            page[22..26].copy_from_slice(&crc.to_le_bytes());
            ogg.extend(page);
            ogg.extend(lacing);
            ogg.extend(packet);
        }
        for bytes in [flac, ogg] {
            let f = Fixture::new(&bytes)?;
            let info = f.read()?;
            assert_eq!(info.tags.title, "Ёжик Mix е\u{308}");
            assert_eq!(info.tags.artist, "Группа Ёлка");
            assert_eq!(info.tags.album, "Альбом Ёё");
            assert_eq!(fs::read(&f.0)?, bytes);
        }
        Ok(())
    }
    #[test]
    fn formats_read_duration_text_and_decode_different_sample_formats_without_a_device(
    ) -> io::Result<()> {
        use rodio::Source;
        for (bytes, title, channels, rate) in [
            (FLAC, "FLAC 界", 2, 48000),
            (WAV, "WAV title", 1, 44100),
            (OGG, "Vorbis title", 2, 48000),
        ] {
            let f = Fixture::new(bytes)?;
            let info = f.read()?;
            assert_eq!(info.tags.title, title);
            assert_eq!(info.tags.artist, "DN test artist");
            assert_eq!(info.tags.album, "Format tests");
            assert_eq!(info.duration, Some(Duration::from_secs(12)));
            let decoder = rodio::Decoder::try_from(io::Cursor::new(bytes)).unwrap();
            assert_eq!(decoder.channels().get(), channels);
            assert_eq!(decoder.sample_rate().get(), rate);
            assert!(decoder.take(10000).any(|s| s.abs() > 0.00001));
            assert_eq!(fs::read(&f.0)?, bytes);
        }
        Ok(())
    }
    #[test]
    fn truncated_chunks_corrupt_ogg_and_unsupported_wave_are_reported() -> io::Result<()> {
        for bytes in [FLAC[..20].to_vec(), WAV[..30].to_vec(), OGG[..26].to_vec()] {
            assert!(Fixture::new(&bytes)?.read().is_err());
        }
        let mut corrupt = OGG.to_vec();
        let end = corrupt.len() - 1;
        corrupt[end] ^= 1;
        assert!(Fixture::new(&corrupt)?.read().is_err());
        let mut unsupported = WAV.to_vec();
        let fmt = unsupported.windows(4).position(|b| b == b"fmt ").unwrap();
        unsupported[fmt + 8..fmt + 10].copy_from_slice(&2u16.to_le_bytes());
        assert_eq!(
            Fixture::new(&unsupported)?.read().err().unwrap().kind(),
            io::ErrorKind::Unsupported
        );
        let f = Fixture::new(FLAC)?;
        let cancel = Cancellation::default();
        cancel.cancel();
        assert!(read(&mut File::open(&f.0)?, &cancel).is_err());
        Ok(())
    }
    #[test]
    fn comment_lengths_are_bounded_and_ogg_opus_is_not_vorbis() -> io::Result<()> {
        let cancel = Cancellation::default();
        assert!(comments(&u32::MAX.to_le_bytes(), &mut Tags::default(), &cancel).is_err());
        let mut page = vec![0; 27];
        page[..4].copy_from_slice(b"OggS");
        page[5] = 6;
        page[26] = 1;
        let lacing = [8];
        let body = b"OpusHead";
        let crc = ogg_crc(&[&page, &lacing, body]);
        page[22..26].copy_from_slice(&crc.to_le_bytes());
        page.extend(lacing);
        page.extend(body);
        assert_eq!(
            Fixture::new(&page)?.read().err().unwrap().kind(),
            io::ErrorKind::Unsupported
        );
        Ok(())
    }
    #[test]
    fn oversized_vorbis_comments_are_skipped_without_losing_duration() -> io::Result<()> {
        fn page(body: &[u8], flags: u8, sequence: u32, granule: u64) -> Vec<u8> {
            let lacing = body.chunks(255).map(|b| b.len() as u8).collect::<Vec<_>>();
            let mut h = vec![0; 27];
            h[..4].copy_from_slice(b"OggS");
            h[5] = flags;
            h[6..14].copy_from_slice(&granule.to_le_bytes());
            h[18..22].copy_from_slice(&sequence.to_le_bytes());
            h[26] = lacing.len() as u8;
            let crc = ogg_crc(&[&h, &lacing, body]);
            h[22..26].copy_from_slice(&crc.to_le_bytes());
            h.extend(lacing);
            h.extend(body);
            h
        }
        let mut identification = vec![0; 30];
        identification[..7].copy_from_slice(b"\x01vorbis");
        identification[11] = 2;
        identification[12..16].copy_from_slice(&48000u32.to_le_bytes());
        identification[29] = 1;
        let mut bytes = page(&identification, 2, 0, 0);
        let mut comment = vec![0; MAX_TAG + 1];
        comment[..7].copy_from_slice(b"\x03vorbis");
        let chunks = comment.chunks(65025).collect::<Vec<_>>();
        for (index, chunk) in chunks.iter().enumerate() {
            let last = index + 1 == chunks.len();
            bytes.extend(page(
                chunk,
                if index > 0 { 1 } else { 0 } | if last { 4 } else { 0 },
                index as u32 + 1,
                if last { 576000 } else { 0 },
            ));
        }
        let info = Fixture::new(&bytes)?.read()?;
        assert_eq!(info.duration, Some(Duration::from_secs(12)));
        assert!(info.tags.title.is_empty());
        Ok(())
    }
    #[test]
    fn floating_point_and_extensible_wave_headers_match_decoded_samples() -> io::Result<()> {
        for (bits, extensible) in [(32u16, false), (64, true)] {
            let mut fmt = vec![0u8; if extensible { 40 } else { 16 }];
            fmt[..2].copy_from_slice(&(if extensible { 0xfffeu16 } else { 3 }).to_le_bytes());
            fmt[2..4].copy_from_slice(&1u16.to_le_bytes());
            fmt[4..8].copy_from_slice(&48000u32.to_le_bytes());
            let align = bits / 8;
            fmt[8..12].copy_from_slice(&(48000u32 * u32::from(align)).to_le_bytes());
            fmt[12..14].copy_from_slice(&align.to_le_bytes());
            fmt[14..16].copy_from_slice(&bits.to_le_bytes());
            if extensible {
                fmt[16..18].copy_from_slice(&22u16.to_le_bytes());
                fmt[18..20].copy_from_slice(&bits.to_le_bytes());
                fmt[24..40]
                    .copy_from_slice(&[3, 0, 0, 0, 0, 0, 16, 0, 128, 0, 0, 170, 0, 56, 155, 113]);
            }
            let pcm = if bits == 32 {
                0.25f32.to_le_bytes().to_vec()
            } else {
                0.25f64.to_le_bytes().to_vec()
            };
            let mut bytes = b"RIFF".to_vec();
            bytes.extend((20 + fmt.len() as u32 + pcm.len() as u32).to_le_bytes());
            bytes.extend(b"WAVEfmt ");
            bytes.extend((fmt.len() as u32).to_le_bytes());
            bytes.extend(fmt);
            bytes.extend(b"data");
            bytes.extend((pcm.len() as u32).to_le_bytes());
            bytes.extend(pcm);
            let info = Fixture::new(&bytes)?.read()?;
            assert_eq!(info.duration, samples(1, 48000));
            let mut decoder = rodio::Decoder::try_from(io::Cursor::new(bytes)).unwrap();
            assert!((decoder.next().unwrap() - 0.25).abs() < 0.000001);
        }
        Ok(())
    }
}
