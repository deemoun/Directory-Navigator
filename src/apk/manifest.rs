//! Bounded AXML inspection and narrow edits. Unknown chunks remain read-only.
//! Android resource chunk layouts: AOSP androidfw/ResourceTypes.h.
use super::binary::*;
use crate::task::Cancellation;
use std::io;
const NONE: u32 = u32::MAX;
const ANDROID: &str = "http://schemas.android.com/apk/res/android";
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Changes {
    pub debuggable: Option<bool>,
    pub version_code: Option<u32>,
    pub version_name: Option<String>,
    pub permissions: Option<Vec<String>>,
}
#[derive(Clone, Debug)]
struct Chunk {
    kind: u16,
    data: Vec<u8>,
}
#[derive(Clone, Debug)]
pub struct Manifest {
    chunks: Vec<Chunk>,
    strings: Vec<String>,
    resources: Vec<u32>,
    pub package: String,
    pub split: bool,
    pub version_code: String,
    pub version_name: String,
    pub min_sdk: String,
    pub target_sdk: String,
    pub debuggable: bool,
    pub permissions: Vec<String>,
    pub components: Vec<String>,
    pub launcher: Option<String>,
    pub xml: String,
    editable: bool,
}
impl Manifest {
    pub fn parse(data: &[u8], cancel: &Cancellation) -> io::Result<Self> {
        if data.len() > 8 * 1024 * 1024
            || u16_at(data, 0)? != 3
            || u16_at(data, 2)? != 8
            || u32_at(data, 4)? as usize != data.len()
        {
            return Err(error("Unsupported Android manifest"));
        }
        let mut result = Self {
            chunks: vec![],
            strings: vec![],
            resources: vec![],
            package: String::new(),
            split: false,
            version_code: String::new(),
            version_name: String::new(),
            min_sdk: String::new(),
            target_sdk: String::new(),
            debuggable: false,
            permissions: vec![],
            components: vec![],
            launcher: None,
            xml: String::new(),
            editable: true,
        };
        let mut offset = 8;
        while offset < data.len() {
            cancel.check()?;
            let kind = u16_at(data, offset)?;
            let head = u16_at(data, offset + 2)? as usize;
            let size = u32_at(data, offset + 4)? as usize;
            if head < 8 || size < head {
                return Err(error("Invalid manifest chunk"));
            }
            let raw = bytes(data, offset, size)?.to_vec();
            offset += size;
            match kind {
                1 => {
                    if !result.strings.is_empty() {
                        return Err(error("Duplicate manifest string pool"));
                    }
                    result.strings = pool(&raw)?;
                    result.editable &= u32_at(&raw, 12)? == 0;
                }
                0x180 => {
                    if head != 8 || !(size - 8).is_multiple_of(4) || !result.resources.is_empty() {
                        return Err(error("Invalid resource map"));
                    }
                    result.resources = (8..size)
                        .step_by(4)
                        .map(|p| u32_at(&raw, p))
                        .collect::<io::Result<_>>()?;
                }
                0x100..=0x104 => {}
                _ => result.editable = false,
            }
            result.chunks.push(Chunk { kind, data: raw });
            if result.chunks.len() > 100_000 {
                return Err(error("Too many manifest chunks"));
            }
        }
        if result.strings.is_empty() {
            return Err(error("Missing manifest strings"));
        }
        result.editable &= result.chunks.iter().all(|c| {
            c.kind != 0x102
                || (u16_at(&c.data, 24).ok() == Some(20)
                    && u16_at(&c.data, 26).ok() == Some(20)
                    && u16_at(&c.data, 28).is_ok_and(|n| c.data.len() == 36 + usize::from(n) * 20))
        });
        result.describe(cancel)?;
        Ok(result)
    }
    fn string(&self, index: u32) -> io::Result<&str> {
        self.strings
            .get(index as usize)
            .map(String::as_str)
            .ok_or_else(|| error("Invalid manifest string reference"))
    }
    fn value(&self, a: &[u8]) -> io::Result<String> {
        let value = u32_at(a, 16)?;
        Ok(match a[15] {
            3 => self.string(value)?.into(),
            0x12 => (value != 0).to_string(),
            0x10 => value.to_string(),
            0x11 => format!("0x{value:x}"),
            1 => format!("@0x{value:08x}"),
            _ => format!("[type 0x{:02x}: 0x{value:x}]", a[15]),
        })
    }
    fn attrs<'a>(&self, raw: &'a [u8]) -> io::Result<Vec<&'a [u8]>> {
        if u16_at(raw, 2)? != 16 || u16_at(raw, 26)? != 20 {
            return Err(error("Unsupported manifest attributes"));
        }
        let start = 16 + u16_at(raw, 24)? as usize;
        let count = u16_at(raw, 28)? as usize;
        if start < 36 || count > 256 {
            return Err(error("Invalid manifest attributes"));
        }
        let data = bytes(raw, start, count * 20)?;
        data.as_chunks::<20>()
            .0
            .iter()
            .map(|a| {
                if u16_at(a, 12)? != 8 {
                    Err(error("Invalid typed attribute"))
                } else {
                    Ok(a.as_slice())
                }
            })
            .collect()
    }
    fn describe(&mut self, cancel: &Cancellation) -> io::Result<()> {
        let mut stack = Vec::<String>::new();
        let mut activity = None;
        let mut main = false;
        let mut category = false;
        let mut roots = 0usize;
        for c in &self.chunks {
            cancel.check()?;
            match c.kind {
                0x102 => {
                    let tag = self.string(u32_at(&c.data, 20)?)?.to_owned();
                    if stack.is_empty() {
                        if tag != "manifest" || roots != 0 || u32_at(&c.data, 16)? != NONE {
                            return Err(error("Invalid manifest root"));
                        }
                        roots += 1;
                    }
                    let attrs = self.attrs(&c.data)?;
                    if stack.len() >= 64 {
                        return Err(error("Manifest nesting exceeds 64"));
                    }
                    self.xml
                        .push_str(&format!("{}<{tag}", "  ".repeat(stack.len())));
                    let mut name = None;
                    let mut declared = std::collections::BTreeSet::new();
                    for a in attrs {
                        let key = self.string(u32_at(a, 4)?)?.to_owned();
                        let ns = u32_at(a, 0)?;
                        let val = self.value(a)?;
                        let prefix = if ns == NONE {
                            String::new()
                        } else {
                            let uri = self.string(ns)?.to_owned();
                            let prefix = if uri == ANDROID {
                                "android".into()
                            } else {
                                format!("ns{ns}")
                            };
                            if declared.insert(ns) {
                                self.xml
                                    .push_str(&format!(" xmlns:{prefix}=\"{}\"", escape(&uri)));
                            }
                            format!("{prefix}:")
                        };
                        self.xml
                            .push_str(&format!(" {prefix}{key}=\"{}\"", escape(&val)));
                        if prefix.is_empty() && tag == "manifest" && key == "split" {
                            self.split = !val.is_empty();
                        }
                        if prefix.is_empty() && tag == "manifest" && key == "package" {
                            self.package = val.clone();
                        }
                        if prefix == "android:" {
                            match (tag.as_str(), key.as_str()) {
                                ("manifest", "versionCode") => self.version_code = val.clone(),
                                ("manifest", "versionName") => self.version_name = val.clone(),
                                ("uses-sdk", "minSdkVersion") => self.min_sdk = val.clone(),
                                ("uses-sdk", "targetSdkVersion") => self.target_sdk = val.clone(),
                                ("application", "debuggable") => self.debuggable = val == "true",
                                (_, "name") => name = Some(val.clone()),
                                _ => {}
                            }
                        }
                    }
                    self.xml.push_str(">\n");
                    if let Some(name) = name {
                        if tag == "uses-permission" {
                            self.permissions.push(name.clone());
                        }
                        if matches!(
                            tag.as_str(),
                            "activity" | "activity-alias" | "service" | "receiver" | "provider"
                        ) {
                            self.components.push(format!("{tag}: {name}"));
                            if tag.starts_with("activity") {
                                activity = Some(name);
                            }
                        } else if tag == "action" && name == "android.intent.action.MAIN" {
                            main = true;
                        } else if tag == "category" && name == "android.intent.category.LAUNCHER" {
                            category = true;
                        }
                    }
                    if tag == "intent-filter" {
                        main = false;
                        category = false;
                    }
                    stack.push(tag);
                }
                0x103 => {
                    let tag = self.string(u32_at(&c.data, 20)?)?.to_owned();
                    if stack.pop().as_deref() != Some(tag.as_str()) {
                        return Err(error("Unbalanced manifest elements"));
                    }
                    self.xml
                        .push_str(&format!("{}</{tag}>\n", "  ".repeat(stack.len())));
                    if tag == "intent-filter" && main && category {
                        self.launcher = activity.clone();
                    }
                    if tag.starts_with("activity") {
                        activity = None;
                    }
                }
                _ => {}
            }
            if self.xml.len() > 8 * 1024 * 1024 {
                return Err(error("Decoded manifest exceeds limit"));
            }
        }
        if !stack.is_empty() || self.package.is_empty() {
            return Err(error("Incomplete manifest"));
        }
        Ok(())
    }
    fn intern(&mut self, value: &str) -> u32 {
        if let Some(i) = self.strings.iter().position(|s| s == value) {
            i as u32
        } else {
            self.strings.push(value.into());
            (self.strings.len() - 1) as u32
        }
    }
    fn attr(&mut self, name: &str, id: u32, kind: u8, value: u32) -> Vec<u8> {
        let ns = self.intern(ANDROID);
        let key = self.intern(name);
        self.resources
            .resize(self.resources.len().max(key as usize + 1), 0);
        self.resources[key as usize] = id;
        let mut a = vec![0; 20];
        put32(&mut a, 0, ns);
        put32(&mut a, 4, key);
        put32(&mut a, 8, if kind == 3 { value } else { NONE });
        put16(&mut a, 12, 8);
        a[15] = kind;
        put32(&mut a, 16, value);
        a
    }
    pub fn patch(&self, changes: &Changes, cancel: &Cancellation) -> io::Result<Vec<u8>> {
        if !self.editable {
            return Err(error(
                "This manifest contains unsupported editable structures",
            ));
        }
        let mut edited = self.clone();
        let mut chunks = Vec::new();
        let mut skip = 0usize;
        for c in &self.chunks {
            cancel.check()?;
            if skip > 0 {
                if c.kind == 0x102 {
                    skip += 1;
                }
                if c.kind == 0x103 {
                    skip -= 1;
                }
                continue;
            }
            if c.kind != 0x102 {
                chunks.push(c.clone());
                continue;
            }
            let tag = self.string(u32_at(&c.data, 20)?)?;
            if tag == "uses-permission" {
                if let Some(permissions) = &changes.permissions {
                    let keep = self.attrs(&c.data)?.iter().any(|a| {
                        u32_at(a, 4).ok().and_then(|i| self.string(i).ok()) == Some("name")
                            && self.value(a).is_ok_and(|name| permissions.contains(&name))
                    });
                    if !keep {
                        skip = 1;
                        continue;
                    }
                }
            }
            if tag == "application" {
                if let Some(permissions) = &changes.permissions {
                    if permissions.len() > 1024 {
                        return Err(error("Too many permissions"));
                    }
                    let mut added = std::collections::BTreeSet::new();
                    for p in permissions {
                        if p.is_empty()
                            || p.len() > 512
                            || !p
                                .chars()
                                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '$'))
                        {
                            return Err(error("Invalid permission name"));
                        }
                        if self.permissions.contains(p) || !added.insert(p) {
                            continue;
                        }
                        let name = edited.intern("uses-permission");
                        let val = edited.intern(p);
                        let a = edited.attr("name", 0x01010003, 3, val);
                        let mut start = vec![0; 36];
                        put16(&mut start, 0, 0x102);
                        put16(&mut start, 2, 16);
                        put32(&mut start, 4, 56);
                        put32(&mut start, 12, NONE);
                        put32(&mut start, 16, NONE);
                        put32(&mut start, 20, name);
                        put16(&mut start, 24, 20);
                        put16(&mut start, 26, 20);
                        put16(&mut start, 28, 1);
                        start.extend(a);
                        let mut end = vec![0; 24];
                        put16(&mut end, 0, 0x103);
                        put16(&mut end, 2, 16);
                        put32(&mut end, 4, 24);
                        put32(&mut end, 12, NONE);
                        put32(&mut end, 16, NONE);
                        put32(&mut end, 20, name);
                        chunks.push(Chunk {
                            kind: 0x102,
                            data: start,
                        });
                        chunks.push(Chunk {
                            kind: 0x103,
                            data: end,
                        });
                    }
                }
            }
            let mut attrs = self
                .attrs(&c.data)?
                .iter()
                .map(|a| a.to_vec())
                .collect::<Vec<_>>();
            let mut replace = |name: &str, id: u32, kind: u8, value: u32| -> io::Result<()> {
                let a = edited.attr(name, id, kind, value);
                if let Some(index) = attrs.iter().position(|a| {
                    u32_at(a, 0).ok().and_then(|n| edited.string(n).ok()) == Some(ANDROID)
                        && u32_at(a, 4).ok().and_then(|n| edited.string(n).ok()) == Some(name)
                }) {
                    attrs[index] = a;
                } else {
                    attrs.push(a);
                }
                Ok(())
            };
            if tag == "manifest" {
                if let Some(v) = changes.version_code {
                    replace("versionCode", 0x0101021b, 0x10, v)?;
                }
                if let Some(v) = &changes.version_name {
                    if v.len() > 1024 {
                        return Err(error("Version name exceeds limit"));
                    }
                    // End the closure borrow before interning another string.
                    let value = if let Some(i) = self.strings.iter().position(|s| s == v) {
                        i as u32
                    } else {
                        edited.intern(v)
                    };
                    let a = edited.attr("versionName", 0x0101021c, 3, value);
                    if let Some(i) = attrs.iter().position(|a| {
                        u32_at(a, 0).ok().and_then(|n| edited.string(n).ok()) == Some(ANDROID)
                            && u32_at(a, 4).ok().and_then(|n| edited.string(n).ok())
                                == Some("versionName")
                    }) {
                        attrs[i] = a;
                    } else {
                        attrs.push(a);
                    }
                }
            } else if tag == "application" {
                if let Some(v) = changes.debuggable {
                    replace("debuggable", 0x0101000f, 0x12, u32::from(v))?;
                }
            }
            let mut raw = c.data[..36].to_vec();
            put16(&mut raw, 24, 20);
            put16(&mut raw, 26, 20);
            put16(&mut raw, 28, attrs.len() as u16);
            // Reordering attributes would invalidate id/class/style indices. Keep their order.
            for a in attrs {
                raw.extend(a);
            }
            let len = raw.len() as u32;
            put32(&mut raw, 4, len);
            chunks.push(Chunk {
                kind: 0x102,
                data: raw,
            });
        }
        let mut output = vec![3, 0, 8, 0, 0, 0, 0, 0];
        let mut map_written = false;
        for c in chunks {
            match c.kind {
                1 => {
                    output.extend(make_pool(&edited.strings)?);
                    if !self.chunks.iter().any(|c| c.kind == 0x180) {
                        output.extend(make_map(&edited.resources));
                        map_written = true;
                    }
                }
                0x180 => {
                    output.extend(make_map(&edited.resources));
                    map_written = true;
                }
                _ => output.extend(c.data),
            }
        }
        if !map_written {
            return Err(error("Missing resource map"));
        }
        let len = output.len() as u32;
        put32(&mut output, 4, len);
        let check = Self::parse(&output, cancel)?;
        if changes.debuggable.is_some_and(|v| v != check.debuggable)
            || changes
                .version_code
                .is_some_and(|v| v.to_string() != check.version_code)
            || changes
                .version_name
                .as_ref()
                .is_some_and(|v| *v != check.version_name)
            || changes.permissions.as_ref().is_some_and(|v| {
                v.iter().collect::<std::collections::BTreeSet<_>>()
                    != check.permissions.iter().collect()
            })
        {
            return Err(error("This manifest cannot represent the requested edit"));
        }
        Ok(output)
    }
}
fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('"', "&quot;")
}
fn length8(data: &[u8], p: &mut usize) -> io::Result<usize> {
    let a = bytes(data, *p, 1)?[0];
    *p += 1;
    if a & 128 != 0 {
        let b = bytes(data, *p, 1)?[0];
        *p += 1;
        Ok(((a as usize & 127) << 8) | b as usize)
    } else {
        Ok(a as usize)
    }
}
fn length16(data: &[u8], p: &mut usize) -> io::Result<usize> {
    let a = u16_at(data, *p)?;
    *p += 2;
    if a & 0x8000 != 0 {
        let b = u16_at(data, *p)?;
        *p += 2;
        Ok(((a as usize & 0x7fff) << 16) | b as usize)
    } else {
        Ok(a as usize)
    }
}
pub(super) fn pool(data: &[u8]) -> io::Result<Vec<String>> {
    if u16_at(data, 2)? < 28 {
        return Err(error("Invalid string pool"));
    }
    let count = u32_at(data, 8)? as usize;
    let styles = u32_at(data, 12)? as usize;
    let start = u32_at(data, 20)? as usize;
    let head = u16_at(data, 2)? as usize;
    let utf8 = u32_at(data, 16)? & 0x100 != 0;
    if count > 100_000 || start < head + count * 4 + styles * 4 || start > data.len() {
        return Err(error("Invalid string pool bounds"));
    }
    let mut result = Vec::new();
    let mut total = 0;
    for i in 0..count {
        let mut p = start
            .checked_add(u32_at(data, head + i * 4)? as usize)
            .ok_or_else(|| error("String offset overflow"))?;
        let s = if utf8 {
            let chars = length8(data, &mut p)?;
            let len = length8(data, &mut p)?;
            let s = std::str::from_utf8(bytes(data, p, len)?)
                .map_err(|_| error("Invalid UTF-8 manifest string"))?
                .to_owned();
            if s.encode_utf16().count() != chars || bytes(data, p + len, 1)?[0] != 0 {
                return Err(error("Invalid manifest string length"));
            }
            s
        } else {
            let len = length16(data, &mut p)?;
            if len > 1_000_000 {
                return Err(error("Oversized manifest string"));
            }
            let units = bytes(data, p, len * 2)?
                .as_chunks::<2>()
                .0
                .iter()
                .map(|v| u16::from_le_bytes([v[0], v[1]]))
                .collect::<Vec<_>>();
            if u16_at(data, p + len * 2)? != 0 {
                return Err(error("Unterminated manifest string"));
            }
            String::from_utf16(&units).map_err(|_| error("Invalid UTF-16 manifest string"))?
        };
        total += s.len();
        if total > 8 * 1024 * 1024 {
            return Err(error("Oversized manifest strings"));
        }
        result.push(s);
    }
    Ok(result)
}
fn make_pool(strings: &[String]) -> io::Result<Vec<u8>> {
    let mut offsets = Vec::new();
    let mut text = Vec::new();
    for s in strings {
        offsets.push(text.len() as u32);
        let units = s.encode_utf16().collect::<Vec<_>>();
        let len = units.len() as u32;
        if len >= 0x8000 {
            text.extend(((len >> 16) as u16 | 0x8000).to_le_bytes());
        }
        text.extend((len as u16).to_le_bytes());
        for u in units {
            text.extend(u.to_le_bytes());
        }
        text.extend([0, 0]);
    }
    while text.len() % 4 != 0 {
        text.push(0);
    }
    let start = 28 + strings.len() * 4;
    let size = start + text.len();
    if size > 8 * 1024 * 1024 {
        return Err(error("Manifest string pool exceeds limit"));
    }
    let mut output = vec![0; 28];
    put16(&mut output, 0, 1);
    put16(&mut output, 2, 28);
    put32(&mut output, 4, size as u32);
    put32(&mut output, 8, strings.len() as u32);
    put32(&mut output, 20, start as u32);
    for offset in offsets {
        output.extend(offset.to_le_bytes());
    }
    output.extend(text);
    Ok(output)
}
fn make_map(map: &[u32]) -> Vec<u8> {
    let mut result = vec![0; 8];
    put16(&mut result, 0, 0x180);
    put16(&mut result, 2, 8);
    put32(&mut result, 4, (8 + map.len() * 4) as u32);
    for id in map {
        result.extend(id.to_le_bytes());
    }
    result
}
