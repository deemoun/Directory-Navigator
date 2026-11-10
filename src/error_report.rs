//! Presentation only: callers supply facts; causes are never parsed or translated.
use crate::localization::Language;
use std::{io, path::Path};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    Unchanged,
    BufferRetained,
    Published,
    Partial,
    Cancelled,
    PreferencesRetained,
}
impl Outcome {
    fn text(self) -> &'static str {
        match self {
        Self::Unchanged=>"This request did not publish any changes.",
        Self::BufferRetained=>"The editor text is retained. The document remains open.",
        Self::Published=>"The file was published. Keep the document open until the warning is resolved.",
        Self::Partial=>"Completed items are retained. Some destinations or source removals may already be complete.",
        Self::Cancelled=>"The operation was cancelled. Completed items are retained.",
        Self::PreferencesRetained=>"Preferences remain available in this running instance.",
    }
    }
}
#[derive(Clone, Debug)]
pub(crate) struct Report {
    summary: &'static str,
    paths: Vec<(&'static str, String)>,
    outcome: Option<Outcome>,
    advice: &'static str,
    cause: String,
}
impl Report {
    pub(crate) fn new(
        summary: &'static str,
        cause: impl Into<String>,
        advice: &'static str,
    ) -> Self {
        Self {
            summary,
            paths: Vec::new(),
            outcome: None,
            advice,
            cause: cause.into(),
        }
    }
    pub(crate) fn path(mut self, label: &'static str, path: impl AsRef<Path>) -> Self {
        if self.paths.len() < 3 {
            self.paths
                .push((label, path.as_ref().to_string_lossy().into_owned()));
        }
        self
    }
    pub(crate) fn text(mut self, label: &'static str, value: &str) -> Self {
        if self.paths.len() < 3 {
            self.paths.push((label, value.to_owned()));
        }
        self
    }
    pub(crate) fn outcome(mut self, outcome: Outcome) -> Self {
        self.outcome = Some(outcome);
        self
    }
    pub(crate) fn io(mut self, kind: io::ErrorKind) -> Self {
        self.advice = match kind {
            io::ErrorKind::NotFound => {
                "Refresh the directory and check that the path still exists."
            }
            io::ErrorKind::PermissionDenied => "Check access permissions for this path.",
            io::ErrorKind::StorageFull => "Free space on the destination filesystem, then retry.",
            io::ErrorKind::AlreadyExists => {
                "Choose another name, or confirm replacement where it is supported."
            }
            io::ErrorKind::InvalidInput => "Check the path and selected options, then retry.",
            _ => self.advice,
        };
        self
    }
    pub(crate) fn render(&self, language: Language) -> String {
        let mut text = language.text(self.summary).to_owned();
        for (label, path) in &self.paths {
            text.push_str(&format!(
                "\n{}: {}",
                language.text(label),
                safe(path, 65536, false)
            ));
        }
        if let Some(outcome) = self.outcome {
            text.push_str("\n\n");
            text.push_str(language.text(outcome.text()));
        }
        if !self.advice.is_empty() {
            text.push_str("\n\n");
            text.push_str(language.text(self.advice));
        }
        if !self.cause.is_empty() {
            text.push_str(&format!(
                "\n\n{}:\n{}",
                language.text("Cause"),
                safe(&self.cause, 8192, true)
            ));
        }
        text
    }
    pub(crate) fn inline(&self, language: Language, width: u16) -> String {
        // Keep the explanation first. Full causes remain in the owned Help/details view.
        format!(
            "{}\n{}: {}\n{}",
            language.text(self.summary),
            language.text("Cause"),
            inline_tail(&safe(&self.cause, 8192, false), width.saturating_sub(8)),
            language.text(self.advice)
        )
    }
}
fn inline_tail(text: &str, width: u16) -> String {
    use crate::presentation::text::Span;
    use unicode_segmentation::UnicodeSegmentation;
    if Span::raw(text).width() <= usize::from(width) {
        return text.into();
    }
    if width == 0 {
        return String::new();
    }
    let mut used = 1;
    let mut suffix = Vec::new();
    for g in text.graphemes(true).rev() {
        let w = Span::raw(g).width();
        if used + w > usize::from(width) {
            break;
        }
        used += w;
        suffix.push(g);
    }
    format!("…{}", suffix.into_iter().rev().collect::<String>())
}
fn safe(value: &str, limit: usize, multiline: bool) -> String {
    use unicode_segmentation::UnicodeSegmentation;
    let mut text = String::new();
    for grapheme in value.graphemes(true) {
        if text.len().saturating_add(grapheme.len()) > limit {
            text.push('…');
            break;
        }
        for c in grapheme.chars() {
            text.push(if c.is_control() && !(multiline && c == '\n') {
                '\u{fffd}'
            } else {
                c
            });
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn localized_context_keeps_native_causes_paths_and_truthful_outcomes() {
        let cause = "Source";
        let path = Path::new("Ёжик e\u{301} name.txt");
        let report=Report::new("Cannot save the file.",cause,"Keep the document open. Use Save As for another destination or encoding; UTF-8 can represent all text characters.").path("File",path).outcome(Outcome::BufferRetained);
        let ru = report.render(Language::Russian);
        assert!(ru.starts_with("Не удалось сохранить файл."));
        assert!(ru.contains(&path.to_string_lossy().to_string()));
        assert!(ru.ends_with("\nSource"));
        assert!(ru.contains("Причина"));
        assert!(ru.contains("UTF-8"));
        assert_eq!(ru, report.render(Language::Russian));
        for (outcome, phrase) in [
            (Outcome::Unchanged, "did not publish"),
            (Outcome::Partial, "Some destinations"),
            (Outcome::Published, "file was published"),
            (Outcome::Cancelled, "was cancelled"),
        ] {
            let text = Report::new("File operation did not finish.", "native cause", "")
                .outcome(outcome)
                .render(Language::English);
            assert!(text.contains(phrase));
            if outcome != Outcome::Unchanged {
                assert!(!text.contains("did not publish"));
            }
        }
    }
    #[test]
    fn io_advice_uses_typed_kinds_and_never_guesses_from_cause_text() {
        let base = Report::new(
            "Cannot save the file.",
            "no space left; Permission denied",
            "fallback",
        );
        assert!(base.render(Language::English).contains("fallback"));
        assert!(base
            .clone()
            .io(io::ErrorKind::StorageFull)
            .render(Language::Russian)
            .contains("Освободите место"));
        assert!(base
            .clone()
            .io(io::ErrorKind::PermissionDenied)
            .render(Language::Russian)
            .contains("права доступа"));
        assert!(base
            .io(io::ErrorKind::NotFound)
            .render(Language::English)
            .contains("path still exists"));
    }
    #[test]
    fn controls_and_long_graphemes_are_bounded_without_mutating_native_inputs() {
        let native = std::path::PathBuf::from("file\n\x1b[31mЁ.txt");
        let original = native.clone();
        let cause = format!("{}\x1b[0m\nend", "👩‍💻e\u{301}".repeat(10000));
        let report = Report::new("Cannot open the file.", cause, "").path("File", &native);
        let text = report.render(Language::English);
        assert!(!text.contains('\x1b'));
        assert!(text.contains("file�"));
        assert!(text.len() < 26000);
        assert_eq!(native, original);
        assert!(safe("👩‍💻", 4, false).ends_with('…'));
        assert_eq!(safe("e\u{301}", 2, false), "…");
        assert_eq!(report.inline(Language::English, 70).lines().count(), 2);
    }
    #[test]
    fn help_claims_match_shared_limits_and_keep_unrelated_bounds() {
        assert_eq!(crate::limits::EDITOR_BYTES, 64 * 1024 * 1024);
        assert_eq!(crate::limits::EDITOR_LINES, 1_000_000);
        assert_eq!(crate::limits::VIEWER_BYTES, 64 * 1024 * 1024);
        for text in [
            crate::commands::help::EDITOR_HELP,
            crate::commands::help::CLASSIC_EDITOR,
            crate::help_text::SAVE_FORMAT,
        ] {
            assert!(text.contains("64 MiB"));
            assert!(text.contains("1,000,000"));
            assert!(!text.contains("250k"));
        }
        assert!(crate::help_text::COPY.contains("Ask before replacing is the default"));
        assert!(crate::help_text::COPY.contains("without merging"));
        assert!(crate::help_text::MOVE.contains("Skipped sources remain"));
        assert!(crate::help_text::VIEWING.contains("above 64 MiB"));
        assert!(include_str!("file_tools/clipboard.rs").contains("2 * 1024 * 1024"));
    }
}
