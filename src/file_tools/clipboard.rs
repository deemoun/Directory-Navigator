//! All-or-nothing native filename/path conversion for the internal text clipboard.
use crate::app::App;
use std::{io, path::Path};
pub const MAX_BYTES: usize = 2 * 1024 * 1024;
pub fn encode<'a>(
    paths: impl IntoIterator<Item = &'a Path>,
    full_paths: bool,
) -> io::Result<(String, usize)> {
    let mut output = String::new();
    let mut count = 0;
    for path in paths {
        let value = if full_paths {
            path.as_os_str()
        } else {
            path.file_name()
                .ok_or_else(|| io::Error::other("Entry has no filename"))?
        };
        let text = value.to_str().ok_or_else(|| {
            io::Error::other(
                "Copy requires Unicode names/paths; an entry cannot be represented without loss.",
            )
        })?;
        if text.contains(['\r', '\n']) {
            return Err(io::Error::other(
                "Cannot copy a name/path containing a newline.",
            ));
        }
        if output
            .len()
            .saturating_add(text.len())
            .saturating_add(usize::from(count > 0))
            > MAX_BYTES
        {
            return Err(io::Error::other("Copied names/paths exceed 2 MiB."));
        }
        if count > 0 {
            output.push('\n');
        }
        output.push_str(text);
        count += 1;
    }
    if count == 0 {
        return Err(io::Error::other("No eligible entries to copy."));
    }
    Ok((output, count))
}
pub fn copy(app: &mut App, full_paths: bool) {
    let panel = &app.panels[app.active];
    let result = if panel.loading {
        Err(io::Error::other("Directory is still loading."))
    } else if panel.only_filtered_marks() {
        Err(io::Error::other(
            "Marked entries are outside the current filter.",
        ))
    } else {
        encode(
            panel
                .entries
                .iter()
                .enumerate()
                .filter(|(i, e)| {
                    !e.parent
                        && if panel.selected.is_empty() {
                            *i == panel.cursor
                        } else {
                            panel.selected.contains(&e.name)
                        }
                })
                .map(|(_, e)| e.path.as_path()),
            full_paths,
        )
    };
    match result {
        Ok((text, count)) => {
            app.editor_clipboard = text;
            app.notice = format!(
                "{} {count}",
                app.config
                    .language
                    .text("Copied entries to internal clipboard:")
            );
        }
        Err(e) => app.error(app.config.language.text(&e.to_string()).to_owned()),
    }
}
