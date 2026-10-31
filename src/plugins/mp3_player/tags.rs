//! Read-only ID3 text. Bound tag/frame sizes before handing text to Symphonia.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Tags {
    pub title: String,
    pub artist: String,
    pub album: String,
}
impl Tags {
    pub fn description(&self, filename: &str) -> String {
        let title = if self.title.is_empty() {
            filename
        } else {
            &self.title
        };
        if self.artist.is_empty() {
            title.into()
        } else {
            format!("{} — {title}", self.artist)
        }
    }
}

#[cfg(target_os = "macos")]
mod reader {
    use super::Tags;
    use std::io::{self, Read, Seek, SeekFrom};
    use symphonia_core::{
        io::BufReader,
        meta::{MetadataBuilder, StandardTagKey, Value},
    };
    const MAX_TAG: usize = 1024 * 1024;
    const MAX_TEXT_FRAME: usize = 16 * 1024;
    fn syncsafe(bytes: &[u8]) -> Option<usize> {
        bytes.iter().try_fold(0usize, |n, &b| {
            if b < 128 {
                Some((n << 7) | usize::from(b))
            } else {
                None
            }
        })
    }
    fn clean(text: &str) -> String {
        text.chars()
            .take(512)
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect::<String>()
            .trim()
            .into()
    }
    fn text_tags(builder: MetadataBuilder) -> Tags {
        let mut tags = Tags::default();
        for tag in builder.metadata().tags() {
            let Value::String(value) = &tag.value else {
                continue;
            };
            let destination = match tag.std_key {
                Some(StandardTagKey::TrackTitle) => &mut tags.title,
                Some(StandardTagKey::Artist) => &mut tags.artist,
                Some(StandardTagKey::Album) => &mut tags.album,
                _ => continue,
            };
            if destination.is_empty() {
                *destination = clean(value);
            }
        }
        tags
    }
    // Keep only the three bounded text frames. Artwork and unknown frames never
    // reach the parser, and an attacker-controlled frame size cannot allocate GBs.
    fn text_only(header: &[u8; 10], mut body: Vec<u8>) -> Option<Vec<u8>> {
        let version = header[3];
        if !(2..=4).contains(&version) || (version == 2 && header[5] & 0x40 != 0) {
            return None;
        }
        if version < 4 && header[5] & 0x80 != 0 {
            let mut out = Vec::with_capacity(body.len());
            let mut at = 0;
            while at < body.len() {
                let byte = body[at];
                out.push(byte);
                at += 1;
                if byte == 255 && body.get(at) == Some(&0) {
                    at += 1;
                }
            }
            body = out;
        }
        let mut at = 0;
        if version >= 3 && header[5] & 0x40 != 0 {
            let bytes = body.get(..4)?;
            let size = if version == 4 {
                syncsafe(bytes)?
            } else {
                u32::from_be_bytes(bytes.try_into().ok()?) as usize
            };
            at = if version == 3 {
                size.checked_add(4)?
            } else {
                size
            };
            if at < 4 || at > body.len() {
                return None;
            }
        }
        let mut frames = Vec::new();
        let mut count = 0;
        let width = if version == 2 { 6 } else { 10 };
        while at + width <= body.len() && count < 1024 {
            count += 1;
            let frame = &body[at..];
            let id_width = if version == 2 { 3 } else { 4 };
            let id = &frame[..id_width];
            if id.iter().all(|b| *b == 0) {
                break;
            }
            if !id
                .iter()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
            {
                return None;
            }
            let size = match version {
                2 => frame[3..6]
                    .iter()
                    .fold(0usize, |n, b| (n << 8) | usize::from(*b)),
                3 => u32::from_be_bytes(frame[4..8].try_into().ok()?) as usize,
                _ => syncsafe(&frame[4..8])?,
            };
            let end = at.checked_add(width)?.checked_add(size)?;
            if end > body.len() {
                return None;
            }
            if size <= MAX_TEXT_FRAME
                && matches!(id, b"TIT2" | b"TPE1" | b"TALB" | b"TT2" | b"TP1" | b"TAL")
            {
                frames.extend_from_slice(&body[at..end]);
            }
            at = end;
        }
        let size = frames.len();
        let mut data = vec![
            b'I',
            b'D',
            b'3',
            version,
            0,
            0,
            ((size >> 21) & 127) as u8,
            ((size >> 14) & 127) as u8,
            ((size >> 7) & 127) as u8,
            (size & 127) as u8,
        ];
        data.extend(frames);
        Some(data)
    }
    pub fn read<R: Read + Seek>(file: &mut R) -> io::Result<Tags> {
        let mut tags = Tags::default();
        let len = file.seek(SeekFrom::End(0))?;
        file.seek(SeekFrom::Start(0))?;
        let mut header = [0; 10];
        if file.read_exact(&mut header).is_ok() && &header[..3] == b"ID3" {
            if let Some(size) = syncsafe(&header[6..]) {
                if size <= MAX_TAG && (size as u64) + 10 <= len {
                    let mut body = vec![0; size];
                    file.read_exact(&mut body)?;
                    if let Some(data) = text_only(&header, body) {
                        let mut builder = MetadataBuilder::new();
                        if symphonia_metadata::id3v2::read_id3v2(
                            &mut BufReader::new(&data),
                            &mut builder,
                        )
                        .is_ok()
                        {
                            tags = text_tags(builder);
                        }
                    }
                }
            }
        }
        if len >= 128 {
            file.seek(SeekFrom::End(-128))?;
            let mut bytes = [0; 128];
            file.read_exact(&mut bytes)?;
            let mut builder = MetadataBuilder::new();
            if symphonia_metadata::id3v1::read_id3v1(&mut BufReader::new(&bytes), &mut builder)
                .is_ok()
            {
                let fallback = text_tags(builder);
                if tags.title.is_empty() {
                    tags.title = fallback.title;
                }
                if tags.artist.is_empty() {
                    tags.artist = fallback.artist;
                }
                if tags.album.is_empty() {
                    tags.album = fallback.album;
                }
            }
        }
        Ok(tags)
    }
}
#[cfg(target_os = "macos")]
pub(super) use reader::read;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_title_uses_filename_and_artist_is_optional() {
        let tags = Tags {
            artist: "Исполнитель".into(),
            ..Default::default()
        };
        assert_eq!(tags.description("界.mp3"), "Исполнитель — 界.mp3");
        assert_eq!(Tags::default().description("name.mp3"), "name.mp3");
    }
    #[cfg(target_os = "macos")]
    mod reading {
        use super::*;
        use std::io::Cursor;
        fn header(version: u8, len: usize) -> Vec<u8> {
            vec![
                b'I',
                b'D',
                b'3',
                version,
                0,
                0,
                ((len >> 21) & 127) as u8,
                ((len >> 14) & 127) as u8,
                ((len >> 7) & 127) as u8,
                (len & 127) as u8,
            ]
        }
        fn v23(frames: &[(&[u8], Vec<u8>)]) -> Vec<u8> {
            let mut body = Vec::new();
            for (id, text) in frames {
                body.extend_from_slice(id);
                body.extend_from_slice(&(text.len() as u32).to_be_bytes());
                body.extend([0, 0]);
                body.extend(text);
            }
            let mut tag = header(3, body.len());
            tag.extend(body);
            tag
        }
        fn v1() -> Vec<u8> {
            let mut data = vec![0; 128];
            data[..3].copy_from_slice(b"TAG");
            data[3..8].copy_from_slice(b"Caf\xe9 ");
            data[33..39].copy_from_slice(b"Artist");
            data[63..68].copy_from_slice(b"Album");
            data
        }
        #[test]
        fn unicode_v23_tags_override_v1_and_fill_missing_album_without_control_sequences() {
            let title = [vec![3], "Название 界\n\x1b[31m".as_bytes().to_vec()].concat();
            let mut data = v23(&[
                (b"TIT2", title),
                (b"TPE1", [vec![3], "Группа".as_bytes().to_vec()].concat()),
            ]);
            data.extend(v1());
            let tags = read(&mut Cursor::new(data)).unwrap();
            assert_eq!(tags.artist, "Группа");
            assert_eq!(tags.album, "Album");
            assert_eq!(tags.title, "Название 界  [31m");
            assert_eq!(read(&mut Cursor::new(v1())).unwrap().title, "Café");
        }
        #[test]
        fn legacy_v22_utf16_bom_and_v24_syncsafe_frames_are_read() {
            let utf16 = [
                vec![1, 255, 254],
                "Песня".encode_utf16().flat_map(u16::to_le_bytes).collect(),
            ]
            .concat();
            let mut body = b"TT2".to_vec();
            body.extend([0, 0, utf16.len() as u8]);
            body.extend(utf16);
            let mut data = header(2, body.len());
            data.extend(body);
            assert_eq!(read(&mut Cursor::new(data)).unwrap().title, "Песня");
            let text = [vec![3], vec![b'x'; 200]].concat();
            let mut body = b"TIT2".to_vec();
            body.extend([0, 0, 1, 73, 0, 0]);
            body.extend(text);
            let mut data = header(4, body.len());
            data.extend(body);
            assert_eq!(read(&mut Cursor::new(data)).unwrap().title.len(), 200);
        }
        #[test]
        fn malformed_huge_frames_oversized_tags_and_artwork_fall_back_without_large_allocations() {
            let mut body = b"TIT2".to_vec();
            body.extend(u32::MAX.to_be_bytes());
            body.extend([0, 0]);
            let mut data = header(3, body.len());
            data.extend(body);
            data.extend(v1());
            assert_eq!(read(&mut Cursor::new(data)).unwrap().title, "Café");
            let mut data = header(3, 2 * 1024 * 1024);
            data.extend(v1());
            assert_eq!(read(&mut Cursor::new(data)).unwrap().album, "Album");
            let data = v23(&[(b"APIC", vec![0; 50000]), (b"TIT2", b"\x03Song".to_vec())]);
            assert_eq!(read(&mut Cursor::new(data)).unwrap().title, "Song");
            assert_eq!(
                read(&mut Cursor::new(b"not audio")).unwrap(),
                Tags::default()
            );
        }
    }
}
