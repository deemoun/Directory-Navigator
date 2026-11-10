//! Validate 7z allocation-driving counts before the library builds its index.
//! Encoded headers are decoded as a single synthetic member, then preflighted.
//! Original packed-data offsets stay unchanged; overlays never touch disk.
use super::{error, password, Input, Password};
use crate::{
    archive::reader::{MAX_ENTRIES, MAX_METADATA, MAX_STREAM},
    task::Cancellation,
};
use std::{
    fs::File,
    io::{self, Read, Seek, SeekFrom, Write},
    sync::Arc,
};
const MAX_STREAMS: usize = 200_000;
const MAX_CODER_STREAMS: usize = 64;
#[derive(Default)]
struct Folder {
    outputs: usize,
    crc: bool,
    streams: usize,
}
struct Parser<'a> {
    bytes: &'a [u8],
    at: usize,
    source_size: u64,
    graph_streams: usize,
}
impl<'a> Parser<'a> {
    fn new(bytes: &'a [u8], source_size: u64) -> Self {
        Self {
            bytes,
            at: 0,
            source_size,
            graph_streams: 0,
        }
    }
    fn take(&mut self, count: usize) -> io::Result<&'a [u8]> {
        let end = self
            .at
            .checked_add(count)
            .filter(|end| *end <= self.bytes.len())
            .ok_or_else(|| error("Truncated 7z header"))?;
        let bytes = &self.bytes[self.at..end];
        self.at = end;
        Ok(bytes)
    }
    fn byte(&mut self) -> io::Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn number(&mut self) -> io::Result<u64> {
        let first = self.byte()?;
        let mut mask = 0x80;
        let mut value = 0;
        for shift in 0..8 {
            if first & mask == 0 {
                return Ok(value | (u64::from(first & (mask - 1)) << (8 * shift)));
            }
            value |= u64::from(self.byte()?) << (8 * shift);
            mask >>= 1;
        }
        Ok(value)
    }
    fn count(&mut self, limit: usize) -> io::Result<usize> {
        usize::try_from(self.number()?)
            .ok()
            .filter(|n| *n <= limit)
            .ok_or_else(|| error("7z header count exceeds DN limits"))
    }
    fn tag(&mut self, expected: u8) -> io::Result<()> {
        if self.byte()? != expected {
            return Err(error("Invalid 7z header property"));
        }
        Ok(())
    }
    fn crcs(&mut self, count: usize) -> io::Result<Vec<bool>> {
        let defined = if self.byte()? != 0 {
            vec![true; count]
        } else {
            let bits = self.take(count.div_ceil(8))?;
            (0..count)
                .map(|n| bits[n / 8] & (0x80 >> (n % 8)) != 0)
                .collect()
        };
        self.take(defined.iter().filter(|v| **v).count() * 4)?;
        Ok(defined)
    }
    fn folder(&mut self) -> io::Result<Folder> {
        let coders = self.count(MAX_CODER_STREAMS)?;
        if coders == 0 {
            return Err(error("Empty 7z coder graph"));
        }
        let (mut inputs, mut outputs) = (0usize, 0usize);
        for _ in 0..coders {
            let flags = self.byte()?;
            let size = usize::from(flags & 15);
            if size == 0 || flags & 0x80 != 0 {
                return Err(error("Unsupported 7z coder graph"));
            }
            self.take(size)?;
            let (a, b) = if flags & 0x10 != 0 {
                (
                    self.count(MAX_CODER_STREAMS)?,
                    self.count(MAX_CODER_STREAMS)?,
                )
            } else {
                (1, 1)
            };
            inputs += a;
            outputs += b;
            if a == 0 || b == 0 || inputs > MAX_CODER_STREAMS || outputs > MAX_CODER_STREAMS {
                return Err(error("7z coder graph exceeds limits"));
            }
            if flags & 0x20 != 0 {
                let n = self.count(MAX_METADATA)?;
                self.take(n)?;
            }
        }
        self.graph_streams += inputs + outputs;
        if self.graph_streams > MAX_STREAMS {
            return Err(error("7z coder graph exceeds limits"));
        }
        let pairs = outputs - 1;
        if pairs >= inputs {
            return Err(error("Invalid 7z stream bindings"));
        }
        for _ in 0..pairs {
            self.count(inputs - 1)?;
            self.count(outputs - 1)?;
        }
        let packed = inputs - pairs;
        if packed > 1 {
            for _ in 0..packed {
                self.count(inputs - 1)?;
            }
        }
        Ok(Folder {
            outputs,
            crc: false,
            streams: 1,
        })
    }
    fn streams(&mut self, encoded: bool) -> io::Result<()> {
        let mut tag = self.byte()?;
        if tag == 6 {
            let position = self.number()?;
            let count = self.count(MAX_ENTRIES)?;
            self.tag(9)?;
            let mut end = 32u64
                .checked_add(position)
                .ok_or_else(|| error("7z offset overflow"))?;
            for _ in 0..count {
                end = end
                    .checked_add(self.number()?)
                    .ok_or_else(|| error("7z size overflow"))?;
            }
            if end > self.source_size {
                return Err(error("7z packed data exceeds source size"));
            }
            tag = self.byte()?;
            if tag == 10 {
                self.crcs(count)?;
                tag = self.byte()?;
            }
            if tag != 0 {
                return Err(error("Invalid 7z pack info"));
            }
            tag = self.byte()?;
        }
        let mut folders = Vec::new();
        if tag == 7 {
            self.tag(11)?;
            let count = self.count(if encoded { 1 } else { MAX_ENTRIES })?;
            if self.byte()? != 0 {
                return Err(error("External 7z folders are unsupported"));
            }
            for _ in 0..count {
                folders.push(self.folder()?);
            }
            self.tag(12)?;
            for folder in &folders {
                for _ in 0..folder.outputs {
                    let value = self.number()?;
                    if value
                        > if encoded {
                            MAX_METADATA as u64
                        } else {
                            MAX_STREAM
                        }
                    {
                        return Err(error("7z unpacked size exceeds limits"));
                    }
                }
            }
            tag = self.byte()?;
            if tag == 10 {
                let crcs = self.crcs(count)?;
                for (folder, crc) in folders.iter_mut().zip(crcs) {
                    folder.crc = crc;
                }
                tag = self.byte()?;
            }
            if tag != 0 {
                return Err(error("Invalid 7z unpack info"));
            }
            tag = self.byte()?;
        }
        if tag == 8 {
            tag = self.byte()?;
            if tag == 13 {
                let mut total = 0usize;
                for folder in &mut folders {
                    folder.streams = self.count(MAX_ENTRIES)?;
                    total += folder.streams;
                }
                if total > MAX_ENTRIES {
                    return Err(error("7z substream count exceeds limits"));
                }
                tag = self.byte()?;
            }
            if tag == 9 {
                for folder in &folders {
                    for _ in 1..folder.streams {
                        if self.number()? > MAX_STREAM {
                            return Err(error("7z unpacked size exceeds limits"));
                        }
                    }
                }
                tag = self.byte()?;
            }
            if tag == 10 {
                let count = folders
                    .iter()
                    .map(|f| {
                        if f.streams == 1 && f.crc {
                            0
                        } else {
                            f.streams
                        }
                    })
                    .sum();
                self.crcs(count)?;
                tag = self.byte()?;
            }
            if tag != 0 {
                return Err(error("Invalid 7z substreams info"));
            }
            tag = self.byte()?;
        }
        if tag != 0 || (encoded && (folders.len() != 1 || folders[0].streams != 1)) {
            return Err(error("Invalid 7z stream info"));
        }
        Ok(())
    }
    fn plain(&mut self) -> io::Result<()> {
        self.tag(1)?;
        let mut tag = self.byte()?;
        if tag == 2 {
            while self.byte()? != 0 {
                let n = self.count(MAX_METADATA)?;
                self.take(n)?;
            }
            tag = self.byte()?;
        }
        if tag == 4 {
            self.streams(false)?;
            tag = self.byte()?;
        }
        if tag == 5 {
            self.count(MAX_ENTRIES)?;
            loop {
                let property = self.byte()?;
                if property == 0 {
                    break;
                }
                let size = self.count(MAX_METADATA)?;
                let bytes = self.take(size)?;
                if property == 0x11 && bytes.first() != Some(&0) {
                    return Err(error("External 7z names are unsupported"));
                }
            }
            tag = self.byte()?;
        }
        if tag != 0 || self.at != self.bytes.len() {
            return Err(error("Unsupported 7z header structure"));
        }
        Ok(())
    }
}
pub(super) struct Overlay {
    input: Input,
    signature: [u8; 32],
    tail: Arc<[u8]>,
    base: u64,
    position: u64,
}
impl Overlay {
    fn new(input: Input, base: u64, tail: Vec<u8>) -> Self {
        let mut signature = [0; 32];
        signature[..8].copy_from_slice(b"7z\xbc\xaf\x27\x1c\0\x04");
        signature[12..20].copy_from_slice(&(base - 32).to_le_bytes());
        signature[20..28].copy_from_slice(&(tail.len() as u64).to_le_bytes());
        signature[28..32].copy_from_slice(&crc32fast::hash(&tail).to_le_bytes());
        let crc = crc32fast::hash(&signature[12..]);
        signature[8..12].copy_from_slice(&crc.to_le_bytes());
        Self {
            input,
            signature,
            tail: tail.into(),
            base,
            position: 0,
        }
    }
}
impl Read for Overlay {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.input.cancel.check().map_err(io::Error::other)?;
        let end = self.base + self.tail.len() as u64;
        if bytes.is_empty() || self.position >= end {
            return Ok(0);
        }
        let n = if self.position < 32 {
            let start = self.position as usize;
            let n = bytes.len().min(32 - start);
            bytes[..n].copy_from_slice(&self.signature[start..start + n]);
            n
        } else if self.position >= self.base {
            let start = (self.position - self.base) as usize;
            let n = bytes.len().min(self.tail.len() - start);
            bytes[..n].copy_from_slice(&self.tail[start..start + n]);
            n
        } else {
            let n = bytes
                .len()
                .min((self.base - self.position).min(65536) as usize);
            self.input.seek(SeekFrom::Start(self.position))?;
            self.input.read(&mut bytes[..n])?
        };
        self.position += n as u64;
        Ok(n)
    }
}
impl Seek for Overlay {
    fn seek(&mut self, seek: SeekFrom) -> io::Result<u64> {
        self.input.cancel.check().map_err(io::Error::other)?;
        let position = match seek {
            SeekFrom::Start(n) => i128::from(n),
            SeekFrom::Current(n) => i128::from(self.position) + i128::from(n),
            SeekFrom::End(n) => i128::from(self.base + self.tail.len() as u64) + i128::from(n),
        };
        self.position = u64::try_from(position).map_err(|_| error("Invalid 7z seek"))?;
        Ok(self.position)
    }
}
struct Header {
    bytes: Vec<u8>,
    cancel: Cancellation,
}
impl Write for Header {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.cancel.check().map_err(io::Error::other)?;
        if self.bytes.len().saturating_add(bytes.len()) > MAX_METADATA {
            return Err(error("Decoded 7z header exceeds metadata limit"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
pub(super) fn open(
    file: File,
    value: Option<&Password>,
    cancel: &Cancellation,
) -> io::Result<sevenz_rust2::ArchiveReader<Overlay>> {
    cancel.check()?;
    let size = file.metadata()?.len();
    let mut input = Input::new(file, cancel);
    let mut signature = [0; 32];
    input.read_exact(&mut signature)?;
    if !signature.starts_with(b"7z\xbc\xaf\x27\x1c")
        || signature[6] != 0
        || u32::from_le_bytes(signature[8..12].try_into().unwrap())
            != crc32fast::hash(&signature[12..])
    {
        return Err(error("Invalid 7z start header"));
    }
    let offset = u64::from_le_bytes(signature[12..20].try_into().unwrap());
    let length = u64::from_le_bytes(signature[20..28].try_into().unwrap());
    if length > MAX_METADATA as u64
        || offset
            .checked_add(32)
            .and_then(|n| n.checked_add(length))
            .is_none_or(|n| n > size)
    {
        return Err(error("Invalid or oversized 7z header"));
    }
    input.seek(SeekFrom::Start(32 + offset))?;
    let mut header = vec![0; length as usize];
    input.read_exact(&mut header)?;
    if crc32fast::hash(&header) != u32::from_le_bytes(signature[28..32].try_into().unwrap()) {
        return Err(error("7z header checksum mismatch"));
    }
    if header.is_empty() {
        header = vec![1, 0];
    }
    let key = sevenz_rust2::Password::from(password(value));
    if header[0] == 0x17 {
        let mut parser = Parser::new(&header[1..], size);
        parser.streams(true)?;
        if parser.at != header.len() - 1 {
            return Err(error("Invalid encoded 7z header"));
        }
        // Reuse only the validated encoded-header streams. The synthetic index
        // declares exactly one file, so no archive-controlled file count is parsed.
        let mut synthetic = vec![1, 4];
        synthetic.extend_from_slice(&header[1..]);
        synthetic.extend_from_slice(&[5, 1, 0x11, 15, 0]);
        for unit in "header\0".encode_utf16() {
            synthetic.extend_from_slice(&unit.to_le_bytes());
        }
        synthetic.extend_from_slice(&[0, 0]);
        let retained = input.file.try_clone()?;
        let overlay = Overlay::new(input, size, synthetic);
        let mut reader =
            sevenz_rust2::ArchiveReader::new_with_memory_limit(overlay, key.clone(), 256 * 1024)
                .map_err(error)?;
        reader.set_thread_count(1);
        let mut decoded = Header {
            bytes: Vec::new(),
            cancel: cancel.clone(),
        };
        reader
            .for_each_entries(|_, source| {
                io::copy(source, &mut decoded)?;
                Ok(true)
            })
            .map_err(error)?;
        header = decoded.bytes;
        // The file is reopened by descriptor cloning, never by an untrusted path.
        // Metadata/source identity is rechecked by the catalog after indexing.
        drop(reader);
        input = Input::new(retained, cancel);
    }
    Parser::new(&header, size).plain()?;
    let overlay = Overlay::new(input, size, header);
    let mut reader = sevenz_rust2::ArchiveReader::new_with_memory_limit(overlay, key, 256 * 1024)
        .map_err(error)?;
    reader.set_thread_count(1);
    Ok(reader)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn counts_and_property_lengths_are_checked_before_library_allocation() {
        let mut bytes = vec![1, 5, 0xff];
        bytes.extend_from_slice(&u64::MAX.to_le_bytes());
        bytes.extend([0, 0]);
        assert!(Parser::new(&bytes, 32).plain().is_err());
        assert!(Parser::new(&[1, 5, 1, 0x11, 0x7f, 0], 32).plain().is_err());
        assert!(Parser::new(&[1, 5, 0, 0, 0], 32).plain().is_ok());
        assert!(Parser::new(&[1, 3, 0], 32).plain().is_err());
    }
}
