//! Small lexical highlighter. No language server or code execution.
use super::buffer::Buffer;
use std::{
    hash::{Hash, Hasher},
    path::Path,
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    Plain,
    Rust,
    Pascal,
    C,
    Python,
    Json,
    Toml,
    Shell,
    Yaml,
}
impl Language {
    pub fn for_path(path: Option<&Path>) -> Self {
        match path
            .and_then(Path::extension)
            .and_then(|ext| ext.to_str())
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str()
        {
            "rs" => Self::Rust,
            "pas" | "pp" => Self::Pascal,
            "c" | "h" | "cpp" | "hpp" | "cc" => Self::C,
            "py" => Self::Python,
            "json" => Self::Json,
            "toml" => Self::Toml,
            "sh" | "bash" | "zsh" => Self::Shell,
            "yaml" | "yml" => Self::Yaml,
            _ => Self::Plain,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Plain => "Text",
            Self::Rust => "Rust",
            Self::Pascal => "Pascal",
            Self::C => "C/C++",
            Self::Python => "Python",
            Self::Json => "JSON",
            Self::Toml => "TOML",
            Self::Shell => "Shell",
            Self::Yaml => "YAML",
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct State {
    block: u8,
    depth: usize,
    string: Option<(char, bool)>,
    raw: Option<usize>,
}
#[derive(Clone, Debug)]
pub struct Token {
    pub start: usize,
    pub end: usize,
    pub color: u8,
    code: bool,
}
struct Highlighted {
    hash: u64,
    end: State,
    tokens: Vec<Token>,
}
pub struct Cache {
    pub language: Language,
    lines: Vec<Highlighted>,
}
impl Cache {
    pub fn new(language: Language) -> Self {
        Self {
            language,
            lines: Vec::new(),
        }
    }
    pub fn refresh(&mut self, buffer: &Buffer) {
        let start = buffer
            .changed_line
            .min(self.lines.len())
            .min(buffer.lines.len());
        let mut state = self
            .lines
            .get(start.wrapping_sub(1))
            .map(|line| line.end)
            .unwrap_or_default();
        let same_length = self.lines.len() == buffer.lines.len();
        for row in start..buffer.lines.len() {
            let text = buffer.line_text(row);
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            text.hash(&mut hasher);
            state.hash_state(&mut hasher);
            let hash = hasher.finish();
            if same_length
                && row > buffer.changed_through
                && self.lines.get(row).is_some_and(|line| line.hash == hash)
            {
                break;
            }
            let (tokens, end) = lex(text, self.language, state);
            let line = Highlighted { hash, end, tokens };
            if row < self.lines.len() {
                self.lines[row] = line;
            } else {
                self.lines.push(line);
            }
            state = end;
        }
        self.lines.truncate(buffer.lines.len());
    }
    pub fn is_code(&self, row: usize, column: usize) -> bool {
        self.lines
            .get(row)
            .and_then(|line| {
                line.tokens
                    .get(line.tokens.partition_point(|t| t.end <= column))
            })
            .is_none_or(|t| t.code)
    }
    pub fn color(&self, row: usize, column: usize) -> u8 {
        self.lines
            .get(row)
            .and_then(|line| {
                let index = line.tokens.partition_point(|token| token.end <= column);
                line.tokens.get(index).filter(|token| token.start <= column)
            })
            .map(|token| token.color)
            .unwrap_or(0x1f)
    }
}
impl State {
    fn hash_state(self, hasher: &mut impl Hasher) {
        self.block.hash(hasher);
        self.depth.hash(hasher);
        self.string.hash(hasher);
        self.raw.hash(hasher);
    }
}
fn keyword(word: &str, language: Language) -> bool {
    let words = match language {
        Language::Rust => "as async await break const continue crate dyn else enum extern false fn for if impl in let loop match mod move mut pub ref return self Self static struct super trait true type unsafe use where while",
        Language::Pascal => "and array asm begin case const constructor destructor div do downto else end file for function goto if implementation in inherited interface label mod nil not object of on operator or packed procedure program record repeat set shl shr string then to type unit until uses var while with xor",
        Language::C => "auto bool break case char class const constexpr continue default delete do double else enum extern false float for if inline int long namespace new nullptr private protected public return short signed sizeof static struct switch template this throw true try typedef typename union unsigned using virtual void volatile while",
        Language::Python => "and as assert async await break class continue def del elif else except False finally for from global if import in is lambda None nonlocal not or pass raise return True try while with yield",
        Language::Json | Language::Toml | Language::Yaml => "true false null yes no on off",
        Language::Shell => "case do done elif else esac fi for function if in select then until while export local readonly return",
        Language::Plain => "",
    };
    if language == Language::Pascal {
        words
            .split_whitespace()
            .any(|key| key.eq_ignore_ascii_case(word))
    } else {
        words.split_whitespace().any(|key| key == word)
    }
}
fn lex(text: &str, language: Language, mut state: State) -> (Vec<Token>, State) {
    let mut tokens = Vec::new();
    if language == Language::Plain {
        return (tokens, State::default());
    }
    let mut i = 0;
    while i < text.len() {
        let start = i;
        let tail = &text[i..];
        let Some(c) = tail.chars().next() else {
            break;
        };
        let mut color = 0x1f;
        if state.depth > 0 {
            color = 0x1a;
            let (open, close) = match state.block {
                1 => ("/*", "*/"),
                2 => ("(*", "*)"),
                _ => ("{", "}"),
            };
            if tail.starts_with(close) {
                i += close.len();
                state.depth -= 1;
            } else if language == Language::Rust && tail.starts_with(open) {
                i += open.len();
                state.depth += 1;
            } else {
                i += c.len_utf8();
            }
        } else if let Some(hashes) = state.raw {
            color = 0x1e;
            if c == '"'
                && tail
                    .as_bytes()
                    .get(1..1 + hashes)
                    .is_some_and(|bytes| bytes.iter().all(|b| *b == b'#'))
            {
                i += 1 + hashes;
                state.raw = None;
            } else {
                i += c.len_utf8();
            }
        } else if let Some((quote, triple)) = state.string {
            color = 0x1e;
            let delimiter = quote.to_string().repeat(if triple { 3 } else { 1 });
            if tail.starts_with(&delimiter) {
                i += delimiter.len();
                if language == Language::Pascal && text[i..].starts_with(quote) {
                    i += quote.len_utf8();
                } else {
                    state.string = None;
                }
            } else if c == '\\' && language != Language::Pascal {
                i += 1;
                i += text[i..].chars().next().map(char::len_utf8).unwrap_or(0);
            } else {
                i += c.len_utf8();
            }
        } else if ((language == Language::Rust || language == Language::C)
            && tail.starts_with("//"))
            || (matches!(
                language,
                Language::Python | Language::Toml | Language::Shell | Language::Yaml
            ) && c == '#')
            || (language == Language::Pascal && tail.starts_with("//"))
        {
            i = text.len();
            color = 0x1a;
        } else if (matches!(language, Language::Rust | Language::C) && tail.starts_with("/*"))
            || (language == Language::Pascal && (tail.starts_with("(*") || c == '{'))
        {
            state.block = if c == '/' {
                1
            } else if c == '(' {
                2
            } else {
                3
            };
            state.depth = 1;
            i += if c == '{' { 1 } else { 2 };
            color = 0x1a;
        } else if language == Language::Rust
            && c == 'r'
            && tail[1..].trim_start_matches('#').starts_with('"')
        {
            let hashes = tail[1..].bytes().take_while(|b| *b == b'#').count();
            state.raw = Some(hashes);
            i += 2 + hashes;
            color = 0x1e;
        } else if c == '"'
            || (c == '\'' && language != Language::Rust)
            || (c == '\'' && tail[1..].chars().take(6).any(|next| next == '\''))
        {
            let triple = matches!(language, Language::Python | Language::Toml)
                && tail.starts_with(&c.to_string().repeat(3));
            state.string = Some((c, triple));
            i += if triple { 3 } else { 1 };
            color = 0x1e;
        } else if c.is_ascii_digit() {
            i += tail
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_'))
                .map(char::len_utf8)
                .sum::<usize>();
            color = 0x1d;
        } else if c.is_alphabetic() || c == '_' {
            i += tail
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .map(char::len_utf8)
                .sum::<usize>();
            if keyword(&text[start..i], language) {
                color = 0x1b;
            }
        } else {
            i += c.len_utf8();
        }
        if let Some(previous) = tokens
            .last_mut()
            .filter(|token: &&mut Token| token.color == color && token.end == start)
        {
            previous.end = i;
        } else {
            tokens.push(Token {
                start,
                end: i,
                color,
                code: !matches!(color, 0x1a | 0x1e),
            });
        }
    }
    // Single-line strings do not color every subsequent line after an unfinished quote.
    if !matches!(language, Language::Rust | Language::C)
        && state.string.is_some_and(|(_, triple)| !triple)
    {
        state.string = None;
    }
    (tokens, state)
}
