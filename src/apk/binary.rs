//! Checked little-endian reads shared by Android's binary formats.
use std::io;
pub(super) fn error(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
pub(super) fn bytes(data: &[u8], offset: usize, size: usize) -> io::Result<&[u8]> {
    data.get(
        offset
            ..offset
                .checked_add(size)
                .ok_or_else(|| error("Android offset overflow"))?,
    )
    .ok_or_else(|| error("Truncated Android structure"))
}
pub(super) fn u16_at(data: &[u8], offset: usize) -> io::Result<u16> {
    Ok(u16::from_le_bytes(
        bytes(data, offset, 2)?.try_into().unwrap(),
    ))
}
pub(super) fn u32_at(data: &[u8], offset: usize) -> io::Result<u32> {
    Ok(u32::from_le_bytes(
        bytes(data, offset, 4)?.try_into().unwrap(),
    ))
}
pub(super) fn put16(data: &mut [u8], offset: usize, value: u16) {
    data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}
pub(super) fn put32(data: &mut [u8], offset: usize, value: u32) {
    data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}
pub(super) fn uleb(data: &[u8], offset: &mut usize) -> io::Result<u32> {
    let mut result = 0;
    for shift in (0..35).step_by(7) {
        let b = bytes(data, *offset, 1)?[0];
        *offset += 1;
        if shift == 28 && b > 15 {
            return Err(error("Invalid Android variable integer"));
        }
        result |= u32::from(b & 127) << shift;
        if b & 128 == 0 {
            return Ok(result);
        }
    }
    Err(error("Invalid Android variable integer"))
}
