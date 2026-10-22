use crate::{app::App, dialog, panel::Panel};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::{ffi::OsStr, path::PathBuf};

/// Whole-name, case-sensitive Unicode wildcard matching; no path expansion.
pub struct Wildcard {
    tokens: Vec<char>,
}
impl Wildcard {
    pub fn new(pattern: &str) -> Self {
        Self {
            tokens: pattern.chars().collect(),
        }
    }
    pub fn matches(&self, name: &OsStr) -> bool {
        if !self.tokens.is_empty() && self.tokens.iter().all(|c| *c == '*') {
            return true;
        }
        let Some(name) = name.to_str() else {
            return false;
        };
        let mut chars = name.chars();
        let mut index = 0;
        let mut star = None;
        loop {
            if self.tokens.get(index) == Some(&'*') {
                index += 1;
                star = Some((index, chars.clone()));
                continue;
            }
            match chars.clone().next() {
                None => return self.tokens[index..].iter().all(|c| *c == '*'),
                Some(c)
                    if self
                        .tokens
                        .get(index)
                        .is_some_and(|token| *token == '?' || *token == c) =>
                {
                    chars.next();
                    index += 1;
                }
                _ => match &mut star {
                    Some((resume, previous)) => {
                        if previous.next().is_none() {
                            return false;
                        }
                        chars = previous.clone();
                        index = *resume;
                    }
                    _ => return false,
                },
            }
        }
    }
}

pub fn apply(panel: &mut Panel, pattern: &str, selecting: bool) -> usize {
    let matcher = Wildcard::new(pattern);
    let mut changed = 0;
    for entry in &panel.entries {
        if !entry.parent && matcher.matches(&entry.name) {
            let updated = if selecting {
                panel.selected.insert(entry.name.clone())
            } else {
                panel.selected.remove(&entry.name)
            };
            changed += usize::from(updated);
        }
    }
    changed
}

pub struct PatternDialog {
    pub selecting: bool,
    pub panel: usize,
    pub path: PathBuf,
    pub field: String,
    pub cursor: usize,
    pub replace_on_type: bool,
}
impl PatternDialog {
    pub fn new(panel: usize, path: PathBuf, selecting: bool) -> Self {
        Self {
            selecting,
            panel,
            path,
            field: "*".into(),
            cursor: 1,
            replace_on_type: true,
        }
    }
    pub fn title(&self) -> &'static str {
        if self.selecting {
            " Select group "
        } else {
            " Unselect group "
        }
    }
    pub fn paste(&mut self, text: &str) {
        if self.replace_on_type {
            self.field.clear();
            self.cursor = 0;
            self.replace_on_type = false;
        }
        dialog::paste_input(&mut self.field, &mut self.cursor, text);
    }
    pub fn edit(&mut self, code: KeyCode) {
        if matches!(
            code,
            KeyCode::Char(_) | KeyCode::Backspace | KeyCode::Delete
        ) && self.replace_on_type
        {
            self.field.clear();
            self.cursor = 0;
        }
        self.replace_on_type = false;
        dialog::edit_input(&mut self.field, &mut self.cursor, code);
    }
}

pub fn accept(app: &mut App) {
    if let Some(prompt) = app.pattern.take() {
        let panel = &mut app.panels[prompt.panel];
        if panel.loading || panel.path != prompt.path {
            app.error("Directory changed; reopen group selection.");
            return;
        }
        let count = apply(panel, &prompt.field, prompt.selecting);
        app.notice = format!(
            "{} {count} entries · case-sensitive pattern",
            if prompt.selecting {
                "Marked"
            } else {
                "Unmarked"
            }
        );
    }
}

pub fn key(app: &mut App, key: KeyEvent) {
    let Some(prompt) = &mut app.pattern else {
        return;
    };
    match key.code {
        KeyCode::Esc => app.pattern = None,
        KeyCode::Enter => accept(app),
        KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            prompt.replace_on_type = true
        }
        code if !key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            prompt.edit(code)
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Independent, non-greedy dynamic-programming oracle for exhaustive small inputs.
    fn reference(pattern: &str, name: &str) -> bool {
        let name = name.chars().collect::<Vec<_>>();
        let mut previous = vec![false; name.len() + 1];
        previous[0] = true;
        for token in pattern.chars() {
            let mut next = vec![false; name.len() + 1];
            next[0] = token == '*' && previous[0];
            for j in 1..=name.len() {
                next[j] = if token == '*' {
                    previous[j] || next[j - 1]
                } else {
                    previous[j - 1] && (token == '?' || token == name[j - 1])
                };
            }
            previous = next;
        }
        previous[name.len()]
    }
    fn words(alphabet: &[char], depth: usize) -> Vec<String> {
        let mut all = vec![String::new()];
        let mut level = all.clone();
        for _ in 0..depth {
            level = level
                .iter()
                .flat_map(|prefix| alphabet.iter().map(move |c| format!("{prefix}{c}")))
                .collect();
            all.extend(level.iter().cloned());
        }
        all
    }
    #[test]
    fn greedy_matching_agrees_with_independent_oracle() {
        let names = words(&['a', '界'], 5);
        for pattern in words(&['a', '界', '*', '?'], 5) {
            let matcher = Wildcard::new(&pattern);
            for name in &names {
                assert_eq!(
                    matcher.matches(OsStr::new(name)),
                    reference(&pattern, name),
                    "pattern={pattern:?}, name={name:?}"
                );
            }
        }
    }
    #[test]
    fn matching_is_scalar_based_case_sensitive_and_not_normalized() {
        for (pattern, name, expected) in [
            ("?", "界", true),
            ("?", "é", false),
            ("??", "é", true),
            ("é", "é", false),
            ("*.RS", "main.rs", false),
            ("**a*b?", "aaab界", true),
            ("*a*a*b", "aaaaac", false),
        ] {
            assert_eq!(Wildcard::new(pattern).matches(OsStr::new(name)), expected);
        }
    }
    #[cfg(unix)]
    #[test]
    fn undecodable_names_match_only_universal_star() {
        use std::os::unix::ffi::OsStrExt;
        let name = OsStr::from_bytes(b"a\xff");
        for pattern in ["*", "**", "***"] {
            assert!(Wildcard::new(pattern).matches(name));
        }
        for pattern in ["?*", "a*", "??", ""] {
            assert!(!Wildcard::new(pattern).matches(name));
        }
    }
}
