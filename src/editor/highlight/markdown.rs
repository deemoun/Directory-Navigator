//! Markdown source spans are derived from the same bounded parser as preview.
use super::*;
impl Cache {
    pub(crate) fn schedule_markdown(
        &mut self,
        buffer: &Buffer,
        requests: &crate::requests::RequestScope,
    ) {
        if self.language != Language::Markdown || self.md_requested == Some(buffer.revision()) {
            return;
        }
        self.lines.clear();
        let revision = buffer.revision();
        self.md_requested = Some(revision);
        let text = buffer.text.clone();
        match requests.spawn("markdown-highlight", move |cancel| {
            crate::markdown::parse(&text, &cancel)
        }) {
            Ok(task) => self.md_pending = Some((revision, task)),
            Err(_) => {
                self.md_pending = None;
            }
        }
    }
    pub(crate) fn markdown_pending(&self) -> bool {
        self.md_pending.is_some()
    }
    pub(crate) fn poll_markdown(&mut self, buffer: &Buffer) -> bool {
        let Some((revision, task)) = &self.md_pending else {
            return false;
        };
        let reply = match task.try_recv() {
            Ok(value) => Some(Ok(value)),
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
            Err(e) => Some(Err(e)),
        };
        let Some(reply) = reply else {
            return false;
        };
        let revision = *revision;
        self.md_pending = None;
        if revision != buffer.revision() || self.language != Language::Markdown {
            return false;
        }
        match reply {
            Ok(Ok(doc)) => self.apply_markdown(buffer, &doc),
            Ok(Err(e)) => self.md_notice = Some(e.to_string()),
            Err(_) => self.md_notice = Some("Markdown highlighting failed; showing source".into()),
        }
        true
    }
    fn apply_markdown(&mut self, buffer: &Buffer, doc: &crate::markdown::Document) {
        let mut colors = vec![0x1f; buffer.text.len()];
        let mut code = vec![false; buffer.text.len()];
        for span in &doc.styles {
            if span.range.end > colors.len() {
                continue;
            }
            colors[span.range.clone()].fill(span.role.color());
            if let Some(name) = &span.code_language {
                let language = match name.as_str() {
                    "rust" => Language::Rust,
                    "pascal" | "pas" => Language::Pascal,
                    "c" | "cpp" | "c++" => Language::C,
                    "python" | "py" => Language::Python,
                    "json" => Language::Json,
                    "toml" => Language::Toml,
                    "yaml" | "yml" => Language::Yaml,
                    "sh" | "shell" | "bash" => Language::Shell,
                    _ => Language::Plain,
                };
                let mut state = State::default();
                let mut offset = span.range.start;
                for line in buffer.text[span.range.clone()].split_inclusive('\n') {
                    let text = line.trim_end_matches(['\r', '\n']);
                    if language == Language::Plain {
                        code[offset..offset + text.len()].fill(true);
                    } else {
                        let (tokens, next) = lex(text, language, state);
                        state = next;
                        for token in tokens {
                            let range = offset + token.start..offset + token.end;
                            colors[range.clone()].fill(token.color);
                            code[range].fill(token.code);
                        }
                    }
                    offset += line.len();
                }
            }
        }
        self.lines = buffer
            .lines
            .iter()
            .map(|line| {
                let mut tokens = Vec::new();
                let mut i = line.start;
                while i < line.end {
                    let start = i;
                    let color = colors[i];
                    let is_code = code[i];
                    i += 1;
                    while i < line.end && colors[i] == color && code[i] == is_code {
                        i += 1;
                    }
                    tokens.push(Token {
                        start: start - line.start,
                        end: i - line.start,
                        color,
                        code: is_code,
                    });
                }
                Highlighted {
                    hash: 0,
                    end: State::default(),
                    tokens,
                }
            })
            .collect();
    }
}
