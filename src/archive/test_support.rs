//! Generated private fixtures; never commit archive payloads.
use std::{
    fs,
    io::{self, Write},
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
pub struct Temp(pub PathBuf);
impl Temp {
    pub fn new() -> io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "dn-archive-upgrade-{}-{}",
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
pub fn zip(
    path: &std::path::Path,
    entries: &[(&str, &[u8])],
    password: Option<&str>,
) -> io::Result<()> {
    let mut writer = zip::ZipWriter::new(fs::File::create(path)?);
    writer.set_comment("fixture comment")?;
    for (name, bytes) in entries {
        let mut options = zip::write::SimpleFileOptions::default();
        if let Some(password) = password {
            options = options.with_aes_encryption(zip::AesMode::Aes256, password);
        }
        writer.start_file(*name, options)?;
        writer.write_all(bytes)?;
    }
    writer.finish()?;
    Ok(())
}
pub fn seven(path: &std::path::Path, name: &str, text: &[u8]) -> io::Result<()> {
    let mut writer =
        sevenz_rust2::ArchiveWriter::new(fs::File::create(path)?).map_err(io::Error::other)?;
    let entry = sevenz_rust2::ArchiveEntry {
        name: name.into(),
        has_stream: true,
        ..Default::default()
    };
    writer
        .push_archive_entry(entry, Some(text))
        .map_err(io::Error::other)?;
    writer.finish()?;
    Ok(())
}
fn crc(bytes: &[u8]) -> u32 {
    let mut value = !0u32;
    for byte in bytes {
        value ^= u32::from(*byte);
        for _ in 0..8 {
            value = (value >> 1) ^ (0xedb88320u32 & 0u32.wrapping_sub(value & 1));
        }
    }
    !value
}
pub fn rar(path: &std::path::Path, name: &str, data: &[u8]) -> io::Result<()> {
    fn header(kind: u8, flags: u16, body: &[u8]) -> Vec<u8> {
        let mut bytes = vec![kind];
        bytes.extend(flags.to_le_bytes());
        bytes.extend(((7 + body.len()) as u16).to_le_bytes());
        bytes.extend(body);
        let mut result = (crc(&bytes) as u16).to_le_bytes().to_vec();
        result.extend(bytes);
        result
    }
    let mut bytes = b"Rar!\x1a\x07\x00".to_vec();
    bytes.extend(header(0x73, 0, &[0; 6]));
    let mut body = (data.len() as u32).to_le_bytes().to_vec();
    body.extend((data.len() as u32).to_le_bytes());
    body.push(3);
    body.extend(crc(data).to_le_bytes());
    body.extend(0u32.to_le_bytes());
    body.extend([20, 0x30]);
    body.extend((name.len() as u16).to_le_bytes());
    body.extend(0o100644u32.to_le_bytes());
    body.extend(name.as_bytes());
    bytes.extend(header(0x74, 0x8000, &body));
    bytes.extend(data);
    bytes.extend(header(0x7b, 0, &[]));
    fs::write(path, bytes)
}
