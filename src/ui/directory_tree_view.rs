use crate::painting::{Frame, Paragraph};
use crate::presentation::layout::Rect;
use crate::turbo::dialog_spec::{centered, scope, Control, ControlKind, DialogSpec};
use crate::{
    app::App,
    directory_tree::{geometry, Action},
    filesystem::display_name,
    formatting::tail,
    mouse::Target,
};
pub(super) fn draw(frame: &mut impl Frame, app: &App, area: Rect) {
    if let Some(s) = tree_spec(app, area) {
        s.paint(frame, app);
    }
}
pub(crate) fn tree_spec(app: &App, area: Rect) -> Option<DialogSpec<'_>> {
    let profile = app.config.palette;
    let dos = profile.styles();
    let tree = app.modals.directory_tree()?;
    let g = geometry(area);
    let rect = centered(area, g.width, g.height);
    let mut spec = DialogSpec::new(
        rect,
        scope(app, crate::focus::Focus::Tree)?,
        tree.focus,
        app.config.language.text(" Directory tree "),
        Target::Tree(Action::Open),
        Target::Tree(Action::Close),
    );
    let x = rect.x + 2;
    let w = rect.width - 5;
    let top = rect.y + 3;
    let root = tree.root.as_ref().unwrap_or(&tree.original);
    spec.caption(
        Paragraph::new(tail(&display_name(&root.to_string_lossy()), w)).style(dos(0x70)),
        Rect::new(x, rect.y + 1, w, 1),
    );
    spec.native(
        crate::turbo::widgets::outline(tree),
        Rect::new(x, top, w, tree.page_rows as u16),
    );
    let mut outline = Control::new(
        0,
        Rect::new(x, top, w, tree.page_rows as u16),
        ControlKind::Outline,
        Target::Tree(Action::Open),
    );
    outline.paint = false;
    outline.hits.clear();
    spec.control(outline);
    for (row, index) in (tree.offset..tree.rows.len())
        .take(tree.page_rows)
        .enumerate()
    {
        let entry = &tree.rows[index];
        let r = Rect::new(x, top + row as u16, w, 1);
        spec.hit(r, Target::Tree(Action::Row(index)));
        let marker = (entry.depth * 2 + if entry.depth > 0 { 2 } else { 0 }) as u16;
        let branch = tree.marker(entry) == "+"
            || tree
                .rows
                .get(index + 1)
                .is_some_and(|next| next.depth > entry.depth);
        if branch && !entry.link && marker < w {
            spec.hit(
                Rect::new(x + marker, r.y, 3.min(w - marker), 1),
                Target::Tree(Action::Toggle(index)),
            );
        }
    }
    let rows = tree.page_rows as u16;
    if rows >= 3 {
        let track = rows - 2;
        let thumb = if tree.rows.len() > 1 {
            tree.cursor * (usize::from(track) - 1) / (tree.rows.len() - 1)
        } else {
            0
        };
        spec.native(
            crate::turbo::widgets::scrollbar(
                tree.cursor,
                tree.rows.len().saturating_sub(1),
                tree.page_rows,
            ),
            Rect::new(rect.right() - 2, top, 1, rows),
        );
        for row in 0..rows {
            let (glyph, command) = if row == 0 {
                ("▲", Action::Up)
            } else if row + 1 == rows {
                ("▼", Action::Down)
            } else if usize::from(row - 1) == thumb {
                ("■", Action::Thumb)
            } else if usize::from(row - 1) < thumb {
                ("░", Action::PageUp)
            } else {
                ("░", Action::PageDown)
            };
            let r = Rect::new(rect.right() - 2, top + row, 1, 1);
            let _ = glyph;
            spec.hit(r, Target::Tree(command));
        }
    }
    let path = tree
        .selected()
        .map(|r| r.path.as_path())
        .unwrap_or(tree.original.as_path());
    for (y, text, color) in [
        (
            rect.bottom() - 6,
            tail(&display_name(&path.to_string_lossy()), w),
            0x71,
        ),
        (
            rect.bottom() - 5,
            tail(&display_name(&tree.status()), w),
            0x70,
        ),
        (
            rect.bottom() - 4,
            "←/→ fold · Space toggle · Ctrl+R refresh · Ctrl+H hidden".into(),
            0x78,
        ),
    ] {
        spec.caption(
            Paragraph::new(text).style(dos(color)),
            Rect::new(x, y, w, 1),
        );
    }
    let bw = (rect.width - 4) / 4;
    for (index, label, command) in [
        (
            1,
            if tree.destination_prompt.is_some() {
                "Select"
            } else {
                "Open"
            }
            .to_string(),
            Action::Open,
        ),
        (2, "Refresh".to_string(), Action::Refresh),
        (
            3,
            format!("Hidden: {}", if tree.hidden { "on" } else { "off" }),
            Action::Hidden,
        ),
        (4, "Close".to_string(), Action::Close),
    ] {
        let r = Rect::new(x + (index as u16 - 1) * bw, rect.bottom() - 2, bw - 1, 1);
        spec.control(Control::new(
            index,
            r,
            ControlKind::Button(app.config.language.text(&label).into()),
            Target::Tree(command),
        ));
    }
    Some(spec)
}
