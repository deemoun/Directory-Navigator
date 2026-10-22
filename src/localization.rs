//! UTF-8 language catalogs, separate from UI logic. English message IDs are fallback text.
use std::{collections::BTreeMap, io, sync::OnceLock};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Language {
    #[default]
    English,
    Russian,
}
impl Language {
    pub fn key(self) -> &'static str {
        match self {
            Self::English => "en",
            Self::Russian => "ru",
        }
    }
    pub fn parse(s: &str) -> Self {
        if s == "ru" {
            Self::Russian
        } else {
            Self::English
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::English => "English",
            Self::Russian => "Русский",
        }
    }
    pub fn text(self, source: &str) -> &str {
        if self == Self::English {
            return source;
        }
        static RUSSIAN: OnceLock<Catalog> = OnceLock::new();
        let catalog = RUSSIAN
            .get_or_init(|| Catalog::parse(include_str!("../locales/ru.lang")).unwrap_or_default());
        catalog
            .strings
            .get(source)
            .or_else(|| catalog.strings.get(source.trim()))
            .map_or(source, String::as_str)
    }
    /// Translate an exact resource message, never filenames or arbitrary user buffers.
    pub fn message(self, source: &str) -> String {
        self.text(source).to_owned()
    }
    pub fn field(self, source: &str, format: crate::regional::DateTimeFormat) -> String {
        if let Some((key, value)) = source.split_once(": ") {
            let translated = self.text(key);
            let value = if matches!(key, "Modified" | "Modified (local)") {
                format.information(value)
            } else if matches!(key, "Entry type" | "Read-only attribute") {
                self.text(value).to_owned()
            } else {
                value.to_owned()
            };
            return format!("{translated}: {value}");
        }
        self.text(source).to_owned()
    }
    pub fn matches(self, label: &str, character: char) -> bool {
        let c = character.to_lowercase().next();
        crate::menu::mnemonic(label) == c || crate::menu::mnemonic(self.text(label)) == c
    }
    pub fn files(self, n: usize) -> &'static str {
        match self {
            Self::English => {
                if n == 1 {
                    "file"
                } else {
                    "files"
                }
            }
            Self::Russian => match (n % 10, n % 100) {
                (1, n) if n != 11 => "файл",
                (2..=4, n) if !(12..=14).contains(&n) => "файла",
                _ => "файлов",
            },
        }
    }
}
#[derive(Default)]
pub struct Catalog {
    pub strings: BTreeMap<String, String>,
}
impl Catalog {
    /// Tab-separated message ID / translation. Backslash, newline and tab use escapes.
    pub fn parse(text: &str) -> io::Result<Self> {
        if text.len() > 1024 * 1024 {
            return Err(io::Error::other("Language catalog exceeds 1 MiB"));
        }
        let mut strings = BTreeMap::new();
        for (i, line) in text.lines().enumerate() {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (key, value) = line.split_once('\t').ok_or_else(|| {
                io::Error::other(format!("Invalid language record at line {}", i + 1))
            })?;
            let key = unescape(key)?;
            let value = unescape(value)?;
            if key.is_empty() || value.is_empty() || strings.insert(key, value).is_some() {
                return Err(io::Error::other("Empty or duplicate language resource"));
            }
            if strings.len() > 4096 {
                return Err(io::Error::other("Too many language resources"));
            }
        }
        Ok(Self { strings })
    }
}
fn unescape(s: &str) -> io::Result<String> {
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            out.push(match chars.next() {
                Some('n') => '\n',
                Some('t') => '\t',
                Some('\\') => '\\',
                _ => return Err(io::Error::other("Invalid language escape")),
            });
        } else if c.is_control() {
            return Err(io::Error::other("Control character in language resource"));
        } else {
            out.push(c);
        }
    }
    Ok(out)
}
