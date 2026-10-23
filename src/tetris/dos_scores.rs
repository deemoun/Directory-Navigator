//! Read-only DN score interchange, derived from the TETRIS.PAS record layout.
//! Twenty DOS records: ten Tetris followed by ten Pentix; only Tetris is imported.
use super::{scores, Record};
use std::{
    io::{self, Read},
    path::Path,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    Cp437,
    Cp866,
}
impl Encoding {
    pub fn key(self) -> &'static str {
        match self {
            Self::Cp437 => "cp437",
            Self::Cp866 => "cp866",
        }
    }
    pub fn parse(value: &str) -> io::Result<Self> {
        match value {
            "cp437" => Ok(Self::Cp437),
            "cp866" => Ok(Self::Cp866),
            _ => Err(io::Error::other("Unsupported DOS score encoding")),
        }
    }
}

pub fn decode(bytes: &[u8], encoding: Encoding) -> io::Result<Vec<Record>> {
    // Borland records are byte packed. Also accept the word-aligned layout used
    // by compatible builds; no host-memory structs or native-endian casts.
    let width = match bytes.len() {
        740 => 37,
        760 => 38,
        _ => {
            return Err(io::Error::other(
                "Invalid dn.tet length (expected 740 or 760 bytes)",
            ))
        }
    };
    let decoded: Vec<_> = bytes
        .iter()
        .enumerate()
        .map(|(i, b)| b ^ ((bytes.len() - i) as u8 ^ 0xaa))
        .collect();
    match parse(&decoded, width, encoding) {
        Ok(records) => Ok(records),
        Err(error) => {
            // DN's initialization path can write a plain, empty table. Never
            // interpret a nonempty unencoded file as a scored original table.
            match parse(bytes, width, encoding) {
                Ok(records)
                    if records.is_empty()
                        && bytes.chunks_exact(width).all(|r| r[width - 4..] == [0; 4]) =>
                {
                    Ok(records)
                }
                _ => Err(error),
            }
        }
    }
}
fn parse(bytes: &[u8], width: usize, encoding: Encoding) -> io::Result<Vec<Record>> {
    let mut records = Vec::new();
    for (index, raw) in bytes.chunks_exact(width).enumerate() {
        let length = usize::from(raw[0]);
        let start_level = raw[31];
        let level = raw[32];
        let score = i32::from_le_bytes(
            raw[width - 4..]
                .try_into()
                .map_err(|_| io::Error::other("Invalid DOS score"))?,
        );
        if length > 30
            || !(1..=10).contains(&start_level)
            || !(start_level..=10).contains(&level)
            || score < 0
        {
            return Err(io::Error::other("Invalid dn.tet record"));
        }
        if index >= 10 || score == 0 {
            continue;
        }
        let name: String = raw[1..=length]
            .iter()
            .map(|b| character(*b, encoding))
            .collect();
        let name = if name.is_empty() {
            scores::ANONYMOUS.to_owned()
        } else {
            name
        };
        if !scores::validate_name(&name) {
            return Err(io::Error::other("Invalid name in dn.tet"));
        }
        records.push(Record {
            name,
            start_level,
            level,
            score: score as u64,
            lines: None,
        });
    }
    records.sort_by_key(|r| std::cmp::Reverse(r.score));
    Ok(records)
}
fn character(byte: u8, encoding: Encoding) -> char {
    if matches!(encoding, Encoding::Cp866) && byte >= 128 {
        CP866_HIGH
            .chars()
            .nth(usize::from(byte - 128))
            .unwrap_or('�')
    } else {
        crate::utilities::characters::cp437(byte)
    }
}

/// Read only the settings directory, preferring lowercase on case-sensitive FSs.
/// Original files are never renamed, rewritten or removed.
pub(super) fn read(modern_path: &Path, encoding: Encoding) -> io::Result<Vec<Record>> {
    for name in ["dn.tet", "DN.TET"] {
        let path = modern_path.with_file_name(name);
        let file = match crate::filesystem::open_regular(&path) {
            Ok(file) => file,
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e),
        };
        let mut bytes = Vec::new();
        file.take(761).read_to_end(&mut bytes)?;
        return decode(&bytes, encoding);
    }
    Ok(Vec::new())
}

// Unicode mapping of the DOS Russian/OEM code page; no locale-dependent decoding.
const CP866_HIGH: &str = "АБВГДЕЖЗИЙКЛМНОПРСТУФХЦЧШЩЪЫЬЭЮЯабвгдежзийклмноп░▒▓│┤╡╢╖╕╣║╗╝╜╛┐└┴┬├─┼╞╟╚╔╩╦╠═╬╧╨╤╥╙╘╒╓╫╪┘┌█▄▌▐▀рстуфхцчшщъыьэюяЁёЄєЇїЎў°∙·√№¤■ ";
