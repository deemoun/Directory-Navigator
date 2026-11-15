//! Native RFCOMM adapters. No LAN sockets, relays or external transfer programs.
use super::{
    obex::{Cancel, Link},
    Device,
};
use std::io;
#[cfg(target_os = "macos")]
#[path = "macos.rs"]
mod native;
#[cfg(target_os = "linux")]
#[path = "linux.rs"]
mod native;
#[cfg(target_os = "windows")]
#[path = "windows.rs"]
mod native;
pub use native::{connect, devices, Listener};
pub fn failure(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(e.to_string())
}
pub fn address(value: &str) -> io::Result<String> {
    let bytes = value.split([':', '-']).collect::<Vec<_>>();
    if bytes.len() != 6
        || bytes
            .iter()
            .any(|b| b.len() != 2 || !b.bytes().all(|c| c.is_ascii_hexdigit()))
    {
        return Err(failure("Invalid Bluetooth device address"));
    }
    Ok(bytes.join(":").to_ascii_uppercase())
}

// A private discovery UUID prevents accidentally delivering to the OS file receiver.
#[cfg(target_os = "linux")]
pub const SERVICE: &str = "8d43f509-fd45-4e59-a428-410203d5ad71";
#[cfg(any(target_os = "windows", target_os = "macos"))]
pub const SERVICE_BYTES: [u8; 16] = [
    141, 67, 245, 9, 253, 69, 78, 89, 164, 40, 65, 2, 3, 213, 173, 113,
];
