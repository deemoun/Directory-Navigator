use super::*;
use std::io;
use toml::{Table, Value as V};
fn bad() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "Invalid saved session")
}
fn table(v: &V) -> io::Result<&Table> {
    v.as_table().ok_or_else(bad)
}
fn num(t: &Table, k: &str) -> io::Result<usize> {
    t.get(k)
        .and_then(V::as_integer)
        .and_then(|n| usize::try_from(n).ok())
        .filter(|n| *n <= i32::MAX as usize)
        .ok_or_else(bad)
}
fn string<'a>(t: &'a Table, k: &str) -> io::Result<&'a str> {
    t.get(k).and_then(V::as_str).ok_or_else(bad)
}
fn path(t: &Table, k: &str) -> io::Result<PathBuf> {
    crate::bookmarks::decode(string(t, k)?)
        .filter(|p| p.is_absolute())
        .ok_or_else(bad)
}
fn boolean(t: &Table, k: &str) -> io::Result<bool> {
    t.get(k).and_then(V::as_bool).ok_or_else(bad)
}
fn format(t: &Table) -> io::Result<Option<TextFormat>> {
    t.get("encoding")
        .map(|v| {
            let encoding = v
                .as_str()
                .and_then(crate::text_encoding::TextEncoding::named)
                .ok_or_else(bad)?;
            let bom = boolean(t, "bom")?;
            if bom && !encoding.unicode() {
                return Err(bad());
            }
            Ok(TextFormat { encoding, bom })
        })
        .transpose()
}
pub(crate) fn decode(value: &V) -> io::Result<Snapshot> {
    let t = table(value)?;
    let p = t
        .get("panels")
        .and_then(V::as_array)
        .filter(|p| p.len() == 2)
        .ok_or_else(bad)?;
    let panels = p
        .iter()
        .map(|v| {
            let t = table(v)?;
            let cursor = t
                .get("cursor")
                .map(|v| {
                    v.as_str()
                        .and_then(crate::bookmarks::decode)
                        .ok_or_else(bad)
                })
                .transpose()?;
            if cursor
                .as_ref()
                .is_some_and(|p| p.components().count() != 1 || p.file_name().is_none())
            {
                return Err(bad());
            }
            let filter = string(t, "filter")?.to_owned();
            if filter.len() > 4096 || filter.chars().any(char::is_control) {
                return Err(bad());
            }
            Ok(PanelRecord {
                path: path(t, "path")?,
                cursor,
                filter,
                offset: num(t, "offset")?,
            })
        })
        .collect::<io::Result<Vec<_>>>()?
        .try_into()
        .map_err(|_| bad())?;
    let active_panel = num(t, "active_panel")?;
    if active_panel > 1 {
        return Err(bad());
    }
    let docs = t.get("documents").and_then(V::as_array).ok_or_else(bad)?;
    if docs.len() > MAX_DOCUMENTS {
        return Err(bad());
    }
    let documents = docs
        .iter()
        .map(|v| {
            let t = table(v)?;
            let kind = match string(t, "kind")? {
                "editor" => Kind::Editor,
                "viewer" => Kind::Viewer,
                _ => return Err(bad()),
            };
            let geometry = t
                .get("geometry")
                .map(|v| {
                    let g = table(v)?;
                    let u = |k| u16::try_from(num(g, k)?).map_err(|_| bad());
                    let bounds = Rect::new(u("x")?, u("y")?, u("width")?, u("height")?);
                    if bounds.width == 0 || bounds.height == 0 {
                        return Err(bad());
                    }
                    Ok(Geometry {
                        bounds,
                        maximized: boolean(g, "maximized")?,
                    })
                })
                .transpose()?;
            let mode = match string(t, "mode")? {
                "auto" => crate::viewer::ViewMode::Auto,
                "text" => crate::viewer::ViewMode::Text,
                "hex" => crate::viewer::ViewMode::Hex,
                _ => return Err(bad()),
            };
            Ok(DocumentRecord {
                path: path(t, "path")?,
                kind,
                geometry,
                format: format(t)?,
                position: num(t, "position")?,
                column: num(t, "column")?,
                scroll: num(t, "scroll")?,
                horizontal: u16::try_from(num(t, "horizontal")?).map_err(|_| bad())?,
                mode,
                markdown: boolean(t, "markdown")?,
                wrap: boolean(t, "wrap")?,
            })
        })
        .collect::<io::Result<Vec<_>>>()?;
    let active_document = t
        .get("active_document")
        .map(|_| num(t, "active_document"))
        .transpose()?;
    if active_document.is_some_and(|n| n >= documents.len()) {
        return Err(bad());
    }
    Ok(Snapshot {
        panels,
        active_panel,
        documents,
        active_document,
    })
}
fn n(n: usize) -> V {
    V::Integer(i64::try_from(n).unwrap_or(i64::MAX))
}
fn s(s: &str) -> V {
    V::String(s.into())
}
fn p(p: &std::path::Path) -> V {
    s(&crate::bookmarks::encode(p))
}
pub(crate) fn encode(snapshot: &Snapshot) -> io::Result<V> {
    if snapshot.documents.len() > MAX_DOCUMENTS {
        return Err(bad());
    }
    let panels = snapshot
        .panels
        .iter()
        .map(|r| {
            let mut t = Table::new();
            t.insert("path".into(), p(&r.path));
            if let Some(cursor) = &r.cursor {
                t.insert("cursor".into(), p(cursor));
            }
            t.insert("filter".into(), s(&r.filter));
            t.insert("offset".into(), n(r.offset));
            V::Table(t)
        })
        .collect();
    let documents = snapshot
        .documents
        .iter()
        .map(|r| {
            let mut t = Table::new();
            t.insert("path".into(), p(&r.path));
            t.insert(
                "kind".into(),
                s(if r.kind == Kind::Editor {
                    "editor"
                } else {
                    "viewer"
                }),
            );
            t.insert(
                "mode".into(),
                s(match r.mode {
                    crate::viewer::ViewMode::Auto => "auto",
                    crate::viewer::ViewMode::Text => "text",
                    crate::viewer::ViewMode::Hex => "hex",
                }),
            );
            for (key, value) in [
                ("position", r.position),
                ("column", r.column),
                ("scroll", r.scroll),
                ("horizontal", r.horizontal as usize),
            ] {
                t.insert(key.into(), n(value));
            }
            t.insert("markdown".into(), V::Boolean(r.markdown));
            t.insert("wrap".into(), V::Boolean(r.wrap));
            if let Some(f) = r.format {
                t.insert("encoding".into(), s(f.encoding.name()));
                t.insert("bom".into(), V::Boolean(f.bom));
            }
            if let Some(g) = r.geometry {
                let mut v = Table::new();
                for (key, value) in [
                    ("x", g.bounds.x),
                    ("y", g.bounds.y),
                    ("width", g.bounds.width),
                    ("height", g.bounds.height),
                ] {
                    v.insert(key.into(), n(value as usize));
                }
                v.insert("maximized".into(), V::Boolean(g.maximized));
                t.insert("geometry".into(), V::Table(v));
            }
            V::Table(t)
        })
        .collect();
    let mut t = Table::new();
    t.insert("panels".into(), V::Array(panels));
    t.insert("documents".into(), V::Array(documents));
    t.insert("active_panel".into(), n(snapshot.active_panel));
    if let Some(a) = snapshot.active_document {
        t.insert("active_document".into(), n(a));
    }
    let value = V::Table(t);
    decode(&value)?;
    Ok(value)
}
