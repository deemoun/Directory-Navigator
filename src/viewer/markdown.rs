//! Per-viewer Markdown state. Parsing owns a generation; painting reads snapshots.
use super::*;
use crate::{markdown::Document, task::Task};
use std::sync::Arc;
#[derive(Default)]
pub struct State {
    pub(crate) hint: bool,
    pub(crate) requested: bool,
    pub rendered: bool,
    pub document: Option<Arc<Document>>,
    source: Option<Contents>,
    pending: Option<Task<Result<Document, String>>>,
    started: bool,
    source_wrap: bool,
    rendered_wrap: Option<bool>,
}
impl State {
    pub fn pending(&self) -> bool {
        self.pending.is_some()
    }
}
impl Viewer {
    pub(crate) fn session_ready(&self) -> bool {
        !self.pending_work()
            && (!self.markdown.hint
                || self.markdown.started
                || self.contents.as_ref().is_some_and(|c| c.binary))
    }
    pub fn markdown_available(&self) -> bool {
        self.markdown.document.is_some()
    }
    fn source_anchor(&self) -> (usize, usize) {
        let (line, byte) = self.anchor();
        if !self.markdown.rendered {
            return (line, byte);
        }
        let Some(doc) = &self.markdown.document else {
            return (line, byte);
        };
        let Some(row) = doc.rows.get(line) else {
            return (0, 0);
        };
        if row.fixed {
            return (row.source_line, byte);
        }
        if let Some(run) = row
            .runs
            .iter()
            .find(|r| r.range.contains(&byte) || r.range.end == byte)
        {
            let offset =
                run.source.start + (byte.saturating_sub(run.range.start)).min(run.source.len());
            let line = doc
                .source_starts
                .partition_point(|start| *start <= offset)
                .saturating_sub(1);
            (line, offset.saturating_sub(doc.source_starts[line]))
        } else {
            (row.source_line, 0)
        }
    }
    pub fn source_line(&self) -> usize {
        self.source_anchor().0
            + self
                .contents
                .as_ref()
                .and_then(|c| c.stream.as_ref())
                .map_or(0, |w| w.first)
    }
    fn rendered_anchor(&self, line: usize, byte: usize) -> (usize, usize) {
        let Some(doc) = &self.markdown.document else {
            return (line, byte);
        };
        if let Some((i, _)) = doc
            .rows
            .iter()
            .enumerate()
            .find(|(_, r)| r.fixed && r.source_line == line)
        {
            return (i, byte);
        }
        let offset = doc.source_starts.get(line).copied().unwrap_or(0) + byte;
        doc.rows
            .iter()
            .enumerate()
            .flat_map(|(i, row)| row.runs.iter().map(move |r| (i, r)))
            .filter(|(_, r)| r.source.contains(&offset) || r.source.end == offset)
            .min_by_key(|(_, r)| r.source.len())
            .map(|(i, r)| {
                (
                    i,
                    r.range.start + offset.saturating_sub(r.source.start).min(r.range.len()),
                )
            })
            .unwrap_or_else(|| (self.display_line_for_source(line), 0))
    }
    pub fn source_line_count(&self) -> usize {
        self.markdown.document.as_ref().map_or_else(
            || {
                self.contents.as_ref().map_or(0, |c| {
                    c.stream
                        .as_ref()
                        .and_then(|w| w.source.total_lines())
                        .unwrap_or(c.lines.len())
                })
            },
            |d| d.source_lines,
        )
    }
    pub(super) fn display_line_for_source(&self, line: usize) -> usize {
        if self.markdown.rendered {
            self.markdown
                .document
                .as_ref()
                .and_then(|d| {
                    d.rows
                        .iter()
                        .enumerate()
                        .filter(|(_, r)| (!r.text.is_empty() || r.fixed) && r.source_line <= line)
                        .max_by_key(|(index, r)| (r.source_line, std::cmp::Reverse(*index)))
                        .map(|(index, _)| index)
                })
                .unwrap_or(0)
        } else {
            line
        }
    }
    pub fn fixed_row(&self, line: usize) -> bool {
        self.markdown.rendered
            && self
                .markdown
                .document
                .as_ref()
                .and_then(|d| d.rows.get(line))
                .is_some_and(|r| r.fixed)
    }
    pub fn horizontal_allowed(&self) -> bool {
        !self.wrap || self.markdown.rendered
    }
    pub fn toggle_markdown(&mut self) {
        if !self.markdown_available() {
            return;
        }
        let was_fixed = self.fixed_row(self.anchor().0);
        let anchor = self
            .reveal_line
            .map(|line| (line, 0))
            .unwrap_or_else(|| self.source_anchor());
        let Some(source) = self.markdown.source.clone() else {
            return;
        };
        if self.markdown.rendered {
            self.markdown.rendered_wrap = Some(self.wrap);
            self.contents = Some(source);
            self.wrap = self.markdown.source_wrap;
        } else {
            self.markdown.source_wrap = self.wrap;
            let document = self.markdown.document.as_ref().unwrap();
            self.contents = Some(Contents {
                stream: None,
                pdf: None,
                raw: source.raw,
                format: source.format,
                encoding_needed: false,
                raw_truncated: source.raw_truncated,
                source: source.source,
                loaded_bytes: source.loaded_bytes,
                binary: false,
                truncated: source.truncated,
                lines: document.rows.iter().map(|r| r.text.clone()).collect(),
            });
            self.wrap = self.markdown.rendered_wrap.unwrap_or(true);
        }
        self.markdown.rendered = !self.markdown.rendered;
        let intended = self.markdown.rendered;
        self.current = None;
        let width = if self.layout_width == 0 {
            80
        } else {
            self.layout_width
        };
        self.horizontal = 0;
        self.rows.clear();
        self.offset = 0;
        self.layout_width = 0;
        self.reveal_line = None;
        self.layout(width as u16);
        if self.markdown.rendered != intended {
            return;
        }
        let target = if self.markdown.rendered {
            self.rendered_anchor(anchor.0, anchor.1)
        } else {
            anchor
        };
        self.offset = self.row_for(target.0, target.1);
        if !self.wrap || self.fixed_row(target.0) {
            if let Some(text) = self.contents.as_ref().and_then(|c| c.lines.get(target.0)) {
                let mut byte = target.1.min(text.len());
                while !text.is_char_boundary(byte) {
                    byte -= 1;
                }
                let column = crate::presentation::text::Span::raw(&text[..byte]).width();
                // A heading's hidden marker is not a reason to pan every source
                // line. Pan only for fixed blocks or an anchor outside the view.
                if was_fixed || self.fixed_row(target.0) || column >= width {
                    self.horizontal = column.min(16384) as u16;
                }
            }
        }
        self.notice.clear();
    }
    pub(super) fn poll_markdown(&mut self) -> Option<Result<(), String>> {
        if !self.markdown.hint || self.contents.as_ref().is_none_or(|c| c.binary) {
            return None;
        }
        if !self.markdown.started {
            self.markdown.started = true;
            let source = self.contents.as_ref()?.source.clone()?;
            self.markdown.source = self.contents.clone();
            match self.requests.spawn("markdown-render", move |cancel| {
                crate::markdown::parse(&source, &cancel).map_err(|e| e.to_string())
            }) {
                Ok(task) => {
                    self.markdown.pending = Some(task);
                    self.notice = "Rendering Markdown…".into();
                }
                Err(e) => self.notice = format!("Markdown preview unavailable: {e}"),
            }
            return Some(Ok(()));
        }
        let reply = crate::task::take_reply(&mut self.markdown.pending)?;
        match reply {
            Ok(Ok(doc)) => {
                self.markdown.document = Some(Arc::new(doc));
                self.notice.clear();
                if self.markdown.requested {
                    self.toggle_markdown();
                }
            }
            Ok(Err(e)) => self.notice = e,
            Err(_) => self.notice = "Markdown rendering failed; showing source".into(),
        }
        Some(Ok(()))
    }
    pub(crate) fn editor_preview(
        owner: crate::desktop::WindowId,
        label: PathBuf,
        text: &str,
        line: usize,
    ) -> Self {
        let mut viewer = Self::from_archive_contents(label, text_contents(text), true);
        viewer.preview_editor = Some(owner);
        viewer.reveal_line = Some(line);
        viewer
    }
}
pub(crate) fn preview_editor(app: &mut crate::app::App) {
    let id = app.desktop.target();
    let Some(e) = app.desktop.editor(id).filter(|e| !e.busy()) else {
        return;
    };
    let Some(buffer) = &e.buffer else {
        return;
    };
    let label = e
        .path
        .clone()
        .unwrap_or_else(|| "[Untitled Markdown]".into());
    let viewer = Viewer::editor_preview(id, label, &buffer.text, buffer.row());
    if let Some(existing) = app
        .desktop
        .document_ids(crate::desktop::WindowKind::Viewer)
        .into_iter()
        .find(|other| {
            app.desktop
                .viewer(*other)
                .is_some_and(|v| v.preview_editor == Some(id))
        })
    {
        let mut viewer = viewer;
        viewer
            .requests
            .bind(crate::requests::Owner::Document(existing));
        viewer.modals.bind(existing);
        *app.desktop.viewer_mut(existing).unwrap() = viewer;
        crate::windowing::activate(app, existing);
    } else {
        app.show_viewer(viewer);
    }
}
