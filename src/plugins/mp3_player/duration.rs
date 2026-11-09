//! MP3 frame metadata only: bounded memory, no audio output or sample decoding.
use crate::task::Cancellation;
use std::io::{self, Read, Seek, SeekFrom};
use std::time::Duration;

#[derive(Clone, Copy)]
struct Header {
    bytes: u64,
    samples: u64,
    rate: u64,
}
fn header(b: [u8; 4]) -> Option<Header> {
    if b[0] != 255 || b[1] & 0xe0 != 0xe0 || b[1] & 6 != 2 {
        return None;
    }
    let version = (b[1] >> 3) & 3;
    if version == 1 {
        return None;
    }
    let index = usize::from(b[2] >> 4);
    let bitrate = if version == 3 {
        [
            0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 0,
        ][index]
    } else {
        [
            0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160, 0,
        ][index]
    };
    let frequency = usize::from((b[2] >> 2) & 3);
    let base = *[44100, 48000, 32000].get(frequency)?;
    let rate = base
        / match version {
            3 => 1,
            2 => 2,
            _ => 4,
        };
    if bitrate == 0 {
        return None;
    }
    let samples = if version == 3 { 1152 } else { 576 };
    Some(Header {
        bytes: (samples / 8 * bitrate * 1000 / rate) + u64::from((b[2] >> 1) & 1),
        samples,
        rate,
    })
}
/// Reuse the decoder's gapless duration when Xing/Info/VBRI declares frame counts.
/// Without those headers Symphonia may estimate VBR duration from a single bitrate,
/// so use a frame-header pass instead of publishing that estimate.
#[cfg(all(
    feature = "audio",
    any(target_os = "macos", target_os = "linux", target_os = "windows")
))]
pub(super) fn probe(
    file: &mut std::fs::File,
    cancel: &Cancellation,
) -> io::Result<Option<Duration>> {
    use rodio::Source;
    cancel.check()?;
    let length = file.metadata()?.len();
    file.rewind()?;
    let mut tag = [0u8; 10];
    if file.read_exact(&mut tag).is_err() {
        return Ok(None);
    }
    let mut offset = 0;
    if &tag[..3] == b"ID3" && tag[6..].iter().all(|b| *b < 128) {
        offset = 10 + tag[6..].iter().fold(0u64, |n, b| (n << 7) | u64::from(*b));
        if tag[3] == 4 && tag[5] & 0x10 != 0 {
            offset += 10;
        }
    }
    file.seek(SeekFrom::Start(offset))?;
    let mut bytes = [0; 4];
    let mut trusted = false;
    if file.read_exact(&mut bytes).is_ok() {
        if let Some(h) = header(bytes).filter(|h| offset + h.bytes <= length) {
            let mut frame = vec![0; h.bytes as usize];
            frame[..4].copy_from_slice(&bytes);
            file.read_exact(&mut frame[4..])?;
            let side = if (bytes[1] >> 3) & 3 == 3 {
                if bytes[3] >> 6 == 3 {
                    17
                } else {
                    32
                }
            } else if bytes[3] >> 6 == 3 {
                9
            } else {
                17
            };
            let xing = 4 + usize::from(bytes[1] & 1 == 0) * 2 + side;
            if let Some(info) = frame.get(xing..xing + 12) {
                trusted = (&info[..4] == b"Xing" || &info[..4] == b"Info")
                    && info[7] & 1 != 0
                    && u32::from_be_bytes(info[8..12].try_into().unwrap()) > 0;
                if trusted && info[7] & 2 != 0 {
                    trusted = frame.get(xing + 12..xing + 16).is_some_and(|b| {
                        u64::from(u32::from_be_bytes(b.try_into().unwrap())) <= length - offset
                    });
                }
            }
            trusted |= frame.get(36..54).is_some_and(|b| {
                &b[..4] == b"VBRI"
                    && u64::from(u32::from_be_bytes(b[10..14].try_into().unwrap()))
                        <= length - offset
                    && u32::from_be_bytes(b[14..18].try_into().unwrap()) > 0
            });
        }
    }
    if trusted {
        // Give the decoder only complete initial MPEG frames, never arbitrary
        // ID3/artwork/APE payloads. Three Layer III frames occupy at most 4323
        // bytes and suffice to initialize metadata plus the first audio packet.
        file.seek(SeekFrom::Start(offset))?;
        let mut prefix = Vec::new();
        let mut position = offset;
        for _ in 0..3 {
            cancel.check()?;
            let mut bytes = [0; 4];
            if file.read_exact(&mut bytes).is_err() {
                break;
            }
            let Some(h) = header(bytes).filter(|h| position + h.bytes <= length) else {
                break;
            };
            prefix.extend(bytes);
            let start = prefix.len();
            prefix.resize(start + h.bytes as usize - 4, 0);
            file.read_exact(&mut prefix[start..])?;
            position += h.bytes;
        }
        let decoder = rodio::Decoder::try_from(std::io::Cursor::new(prefix));
        cancel.check()?;
        if let Some(duration) = decoder.ok().and_then(|d| d.total_duration()) {
            return Ok(Some(duration));
        }
    }
    read(file, cancel)
}

/// Count frames, including VBR streams without a trustworthy duration tag.
/// Unknown/free-format/truncated streams remain playable with unknown duration.
pub(super) fn read<R: Read + Seek>(
    file: &mut R,
    cancel: &Cancellation,
) -> io::Result<Option<Duration>> {
    cancel.check()?;
    let length = file.seek(SeekFrom::End(0))?;
    file.rewind()?;
    let mut first = [0; 10];
    if length < 10 {
        return Ok(None);
    }
    file.read_exact(&mut first)?;
    let mut offset = 0;
    if &first[..3] == b"ID3" {
        if first[6..10].iter().any(|b| *b >= 128) {
            return Ok(None);
        }
        let size = first[6..10]
            .iter()
            .fold(0u64, |n, b| (n << 7) | u64::from(*b));
        offset = 10
            + size
            + if first[3] == 4 && first[5] & 0x10 != 0 {
                10
            } else {
                0
            };
    }
    let mut samples = 0u64;
    let mut sample_rate = 0;
    let mut searched = 0;
    while offset + 4 <= length {
        cancel.check()?;
        file.seek(SeekFrom::Start(offset))?;
        let mut b = [0; 4];
        file.read_exact(&mut b)?;
        let Some(h) = header(b) else {
            if samples > 0 {
                // ID3v1 and APE metadata may follow the final frame.
                if (length - offset == 128 && &b[..3] == b"TAG") || &b == b"APET" {
                    break;
                }
                return Ok(None);
            }
            searched += 1;
            if searched > 65536 {
                return Ok(None);
            }
            offset += 1;
            continue;
        };
        if offset + h.bytes > length || (sample_rate != 0 && sample_rate != h.rate) {
            return Ok(None);
        }
        sample_rate = h.rate;
        samples = samples
            .checked_add(h.samples)
            .ok_or_else(|| io::Error::other("MP3 duration overflow"))?;
        offset += h.bytes;
    }
    if samples == 0 || (offset != length && length - offset < 4) {
        return Ok(None);
    }
    Ok(Some(
        Duration::from_secs(samples / sample_rate)
            + Duration::from_nanos((samples % sample_rate) * 1_000_000_000 / sample_rate),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    fn frames(indices: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        for index in indices {
            let bytes = [255, 251, *index << 4, 0];
            let h = header(bytes).unwrap();
            out.extend(bytes);
            out.resize(out.len() + h.bytes as usize - 4, 0);
        }
        out
    }
    #[test]
    fn cbr_vbr_and_long_tracks_use_frame_samples_not_file_size() -> io::Result<()> {
        let c = Cancellation::default();
        let expected = Duration::from_secs_f64(1152.0 * 100.0 / 44100.0);
        for data in [
            frames(&[9; 100]),
            frames(
                &(0..100)
                    .map(|i| if i % 2 == 0 { 5 } else { 12 })
                    .collect::<Vec<_>>(),
            ),
        ] {
            let got = read(&mut Cursor::new(data), &c)?.unwrap();
            assert!(got.abs_diff(expected) < Duration::from_nanos(2));
        }
        assert_eq!(
            super::super::format_time(Duration::from_secs(60001)),
            "1000:01"
        );
        Ok(())
    }
    #[test]
    fn truncation_tags_unknown_data_and_cancellation() -> io::Result<()> {
        let c = Cancellation::default();
        let mut data = frames(&[9, 9]);
        data.pop();
        assert!(read(&mut Cursor::new(data), &c)?.is_none());
        assert!(read(&mut Cursor::new(vec![0; 100]), &c)?.is_none());
        let mut data = b"ID3\x04\0\0\0\0\0\x03abc".to_vec();
        data.extend(frames(&[9, 9]));
        assert!(read(&mut Cursor::new(data), &c)?.is_some());
        c.cancel();
        assert!(read(&mut Cursor::new(frames(&[9])), &c).is_err());
        Ok(())
    }
    #[test]
    fn real_mp3_fixture_has_duration_without_a_device() -> io::Result<()> {
        let data = include_bytes!("../../../tests/fixtures/mp3/tone.mp3");
        assert!(
            read(&mut Cursor::new(data), &Cancellation::default())?
                .unwrap()
                .as_secs()
                >= 11
        );
        Ok(())
    }
}
