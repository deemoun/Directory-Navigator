//! Read-only standard DEX catalog and instruction references. No code execution.
use super::binary::*;
use crate::task::Cancellation;
use std::{collections::BTreeMap, io};
#[derive(Debug, Clone)]
pub struct Method {
    pub name: String,
    pub code: usize,
}
#[derive(Debug, Clone)]
pub struct Dex {
    pub strings: Vec<String>,
    pub classes: Vec<String>,
    pub methods: Vec<Method>,
    data: Vec<u8>,
}
impl Dex {
    pub fn parse(data: Vec<u8>, cancel: &Cancellation) -> io::Result<Self> {
        if data.len() > 64 * 1024 * 1024
            || bytes(&data, 0, 4)? != b"dex\n"
            || bytes(&data, 7, 1)? != [0]
        {
            return Err(error("Unsupported DEX header"));
        }
        let version = std::str::from_utf8(bytes(&data, 4, 3)?)
            .ok()
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(0);
        if !(35..=40).contains(&version)
            || u32_at(&data, 32)? as usize != data.len()
            || u32_at(&data, 36)? != 112
            || u32_at(&data, 40)? != 0x12345678
        {
            return Err(error("Unsupported DEX version or byte order"));
        }
        fn table(data: &[u8], pos: usize, width: usize) -> io::Result<(usize, usize)> {
            let count = u32_at(data, pos)? as usize;
            let offset = u32_at(data, pos + 4)? as usize;
            if count > 1_000_000 {
                return Err(error("DEX table exceeds limit"));
            }
            bytes(data, offset, count * width)?;
            Ok((count, offset))
        }
        let (count, offset) = table(&data, 56, 4)?;
        let mut strings = Vec::new();
        let mut total = 0;
        for i in 0..count {
            cancel.check()?;
            let mut p = u32_at(&data, offset + i * 4)? as usize;
            let expected = uleb(&data, &mut p)? as usize;
            if expected > 16 * 1024 {
                return Err(error("DEX string exceeds limit"));
            }
            let mut units = Vec::new();
            loop {
                let a = bytes(&data, p, 1)?[0];
                p += 1;
                if a == 0 {
                    break;
                }
                let u = if a < 128 {
                    u16::from(a)
                } else if a & 0xe0 == 0xc0 {
                    let b = bytes(&data, p, 1)?[0];
                    p += 1;
                    if b & 0xc0 != 0x80 {
                        return Err(error("Invalid DEX MUTF-8"));
                    }
                    let u = (u16::from(a & 31) << 6) | u16::from(b & 63);
                    if u < 128 && u != 0 {
                        return Err(error("Overlong DEX string"));
                    }
                    u
                } else if a & 0xf0 == 0xe0 {
                    let b = bytes(&data, p, 2)?;
                    p += 2;
                    if b.iter().any(|b| b & 0xc0 != 0x80) {
                        return Err(error("Invalid DEX MUTF-8"));
                    }
                    let u = (u16::from(a & 15) << 12)
                        | (u16::from(b[0] & 63) << 6)
                        | u16::from(b[1] & 63);
                    if u < 0x800 {
                        return Err(error("Overlong DEX string"));
                    }
                    u
                } else {
                    return Err(error("Invalid DEX MUTF-8"));
                };
                units.push(u);
                if units.len() > expected {
                    return Err(error("DEX string length mismatch"));
                }
            }
            if units.len() != expected {
                return Err(error("DEX string length mismatch"));
            }
            let text = String::from_utf16(&units).map_err(|_| error("Invalid DEX UTF-16"))?;
            total += text.len();
            if total > 16 * 1024 * 1024 {
                return Err(error("DEX strings exceed limit"));
            }
            strings.push(text);
        }
        let lookup = |index: u32| {
            strings
                .get(index as usize)
                .cloned()
                .ok_or_else(|| error("Invalid DEX string index"))
        };
        let (count, offset) = table(&data, 64, 4)?;
        let mut type_bytes = 0usize;
        let types = (0..count)
            .map(|i| {
                let name = lookup(u32_at(&data, offset + i * 4)?)?;
                type_bytes += name.len();
                if name.len() > 4096 || type_bytes > 16 * 1024 * 1024 {
                    return Err(error("DEX type catalog exceeds limit"));
                }
                Ok(name)
            })
            .collect::<io::Result<Vec<_>>>()?;
        let (count, offset) = table(&data, 72, 12)?;
        let mut protos = Vec::new();
        let mut proto_bytes = 0usize;
        for i in 0..count {
            cancel.check()?;
            let p = offset + i * 12;
            let ret = types
                .get(u32_at(&data, p + 4)? as usize)
                .ok_or_else(|| error("Invalid DEX return type"))?;
            let params = u32_at(&data, p + 8)? as usize;
            let mut signature = String::from("(");
            if params != 0 {
                let count = u32_at(&data, params)? as usize;
                if count > 65535 {
                    return Err(error("Too many DEX parameters"));
                }
                for n in 0..count {
                    if signature.len() > 8192 {
                        return Err(error("DEX signature exceeds limit"));
                    }
                    signature.push_str(
                        types
                            .get(u16_at(&data, params + 4 + n * 2)? as usize)
                            .ok_or_else(|| error("Invalid DEX parameter"))?,
                    );
                }
            }
            signature.push(')');
            signature.push_str(ret);
            proto_bytes += signature.len();
            if signature.len() > 8192 || proto_bytes > 16 * 1024 * 1024 {
                return Err(error("DEX signatures exceed limit"));
            }
            protos.push(signature);
        }
        let (count, offset) = table(&data, 88, 8)?;
        let mut methods = Vec::new();
        let mut method_bytes = 0usize;
        for i in 0..count {
            cancel.check()?;
            let p = offset + i * 8;
            let class = types
                .get(u16_at(&data, p)? as usize)
                .ok_or_else(|| error("Invalid DEX method owner"))?;
            let proto = protos
                .get(u16_at(&data, p + 2)? as usize)
                .ok_or_else(|| error("Invalid DEX prototype"))?;
            let name = format!("{class}->{}{proto}", lookup(u32_at(&data, p + 4)?)?);
            method_bytes += name.len();
            if method_bytes > 32 * 1024 * 1024 {
                return Err(error("DEX method catalog exceeds limit"));
            }
            methods.push(Method { name, code: 0 });
        }
        let (count, offset) = table(&data, 96, 32)?;
        let mut classes = Vec::new();
        let mut class_bytes = 0usize;
        for i in 0..count {
            cancel.check()?;
            let p = offset + i * 32;
            classes.push(
                types
                    .get(u32_at(&data, p)? as usize)
                    .ok_or_else(|| error("Invalid DEX class"))?
                    .clone(),
            );
            class_bytes += classes.last().unwrap().len();
            if class_bytes > 16 * 1024 * 1024 {
                return Err(error("DEX class catalog exceeds limit"));
            }
            let mut p = u32_at(&data, p + 24)? as usize;
            if p == 0 {
                continue;
            }
            let counts = (0..4)
                .map(|_| uleb(&data, &mut p))
                .collect::<io::Result<Vec<_>>>()?;
            if counts.iter().any(|&c| c > 1_000_000) {
                return Err(error("DEX class data exceeds limit"));
            }
            for _ in 0..counts[0] + counts[1] {
                uleb(&data, &mut p)?;
                uleb(&data, &mut p)?;
            }
            for count in &counts[2..] {
                let mut index = 0u32;
                for _ in 0..*count {
                    cancel.check()?;
                    index = index
                        .checked_add(uleb(&data, &mut p)?)
                        .ok_or_else(|| error("DEX index overflow"))?;
                    uleb(&data, &mut p)?;
                    let code = uleb(&data, &mut p)? as usize;
                    let method = methods
                        .get_mut(index as usize)
                        .ok_or_else(|| error("Invalid encoded method"))?;
                    method.code = code;
                    if code != 0 {
                        bytes(&data, code, 16)?;
                    }
                }
            }
        }
        Ok(Self {
            strings,
            classes,
            methods,
            data,
        })
    }
    pub fn instructions(&self, index: usize, cancel: &Cancellation) -> io::Result<Vec<String>> {
        let method = self
            .methods
            .get(index)
            .ok_or_else(|| error("Unknown method"))?;
        if method.code == 0 {
            return Ok(vec!["No code (abstract, native, or external method)".into()]);
        }
        let size = u32_at(&self.data, method.code + 12)? as usize;
        if size > 1_000_000 {
            return Err(error("Method exceeds instruction limit"));
        }
        let data = bytes(&self.data, method.code + 16, size * 2)?;
        let mut rows = vec![method.name.clone()];
        let mut pc = 0;
        while pc < size {
            cancel.check()?;
            let unit = u16_at(data, pc * 2)?;
            let op = unit as u8;
            let width = width(data, pc, unit)?;
            bytes(data, pc * 2, width * 2)?;
            let mut reference = String::new();
            if op == 0x1a || op == 0x1b {
                let idx = if op == 0x1a {
                    u16_at(data, (pc + 1) * 2)? as u32
                } else {
                    u32_at(data, (pc + 1) * 2)?
                };
                let text = self
                    .strings
                    .get(idx as usize)
                    .ok_or_else(|| error("Invalid const-string reference"))?;
                reference = format!(" {:?}", text);
            } else if matches!(op,0x6e..=0x72|0x74..=0x78|0xfa|0xfb) {
                let idx = u16_at(data, (pc + 1) * 2)? as usize;
                reference = format!(
                    " {}",
                    self.methods
                        .get(idx)
                        .ok_or_else(|| error("Invalid invoke reference"))?
                        .name
                );
            }
            let raw = (0..width.min(5))
                .map(|n| u16_at(data, (pc + n) * 2).map(|u| format!("{u:04x}")))
                .collect::<io::Result<Vec<_>>>()?
                .join(" ");
            rows.push(format!("{pc:06x}  {raw:<24} {}{reference}", mnemonic(op)));
            pc += width;
            if rows.len() > 100_000 {
                return Err(error("Disassembly exceeds row limit"));
            }
        }
        Ok(rows)
    }
    pub fn string_uses(&self, query: &str, cancel: &Cancellation) -> io::Result<Vec<String>> {
        let matches = self
            .strings
            .iter()
            .enumerate()
            .filter(|(_, s)| s.to_lowercase().contains(&query.to_lowercase()))
            .map(|(i, s)| (i as u32, s))
            .collect::<BTreeMap<_, _>>();
        let mut rows = Vec::new();
        for method in &self.methods {
            cancel.check()?;
            if method.code == 0 {
                continue;
            }
            let size = u32_at(&self.data, method.code + 12)? as usize;
            if size > 1_000_000 {
                return Err(error("Method exceeds limit"));
            }
            let data = bytes(&self.data, method.code + 16, size * 2)?;
            let mut pc = 0;
            while pc < size {
                cancel.check()?;
                let u = u16_at(data, pc * 2)?;
                let op = u as u8;
                let w = width(data, pc, u)?;
                bytes(data, pc * 2, w * 2)?;
                let id = match op {
                    0x1a => Some(u16_at(data, (pc + 1) * 2)? as u32),
                    0x1b => Some(u32_at(data, (pc + 1) * 2)?),
                    _ => None,
                };
                if let Some(s) = id.and_then(|id| matches.get(&id)) {
                    rows.push(format!("{} +0x{pc:x}: {s:?}", method.name));
                }
                pc += w;
                if rows.len() > 10_000 {
                    return Err(error("Too many string usages; narrow the query"));
                }
            }
        }
        Ok(rows)
    }
}
fn width(data: &[u8], pc: usize, u: u16) -> io::Result<usize> {
    Ok(if u as u8 == 0 && u != 0 {
        match u {
            0x100 => 4 + u16_at(data, (pc + 1) * 2)? as usize * 2,
            0x200 => 2 + u16_at(data, (pc + 1) * 2)? as usize * 4,
            0x300 => {
                let elem = u16_at(data, (pc + 1) * 2)? as usize;
                let n = u32_at(data, (pc + 2) * 2)? as usize;
                if ![1, 2, 4, 8].contains(&elem) {
                    return Err(error("Invalid array payload"));
                }
                4 + elem
                    .checked_mul(n)
                    .ok_or_else(|| error("Array payload overflow"))?
                    .div_ceil(2)
            }
            _ => return Err(error("Unknown DEX payload")),
        }
    } else {
        match u as u8 {
            0x00
            | 0x01
            | 0x04
            | 0x07
            | 0x0a..=0x12
            | 0x1d
            | 0x1e
            | 0x21
            | 0x27
            | 0x28
            | 0x7b..=0x8f
            | 0xb0..=0xcf => 1,
            0x02
            | 0x05
            | 0x08
            | 0x13
            | 0x15
            | 0x16
            | 0x19
            | 0x1a
            | 0x1c
            | 0x1f
            | 0x20
            | 0x22
            | 0x23
            | 0x29
            | 0x2d..=0x3d
            | 0x44..=0x6d
            | 0x90..=0xaf
            | 0xd0..=0xe2
            | 0xfe
            | 0xff => 2,
            0x03
            | 0x06
            | 0x09
            | 0x14
            | 0x17
            | 0x1b
            | 0x24..=0x26
            | 0x2a..=0x2c
            | 0x6e..=0x72
            | 0x74..=0x78
            | 0xfc
            | 0xfd => 3,
            0xfa | 0xfb => 4,
            0x18 => 5,
            _ => return Err(error("Unsupported DEX opcode")),
        }
    })
}
fn mnemonic(op: u8) -> String {
    match op {
        0 => "nop",
        0x0e => "return-void",
        0x0f => "return",
        0x10 => "return-wide",
        0x11 => "return-object",
        0x12 => "const/4",
        0x13 => "const/16",
        0x14 => "const",
        0x1a => "const-string",
        0x1b => "const-string/jumbo",
        0x22 => "new-instance",
        0x27 => "throw",
        0x28 => "goto",
        0x29 => "goto/16",
        0x2a => "goto/32",
        0x6e => "invoke-virtual",
        0x6f => "invoke-super",
        0x70 => "invoke-direct",
        0x71 => "invoke-static",
        0x72 => "invoke-interface",
        0x74 => "invoke-virtual/range",
        0x76 => "invoke-direct/range",
        0x77 => "invoke-static/range",
        _ => return format!("op_{op:02x}"),
    }
    .into()
}
