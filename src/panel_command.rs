//! Toolkit-neutral pane commands. Filesystem actions retain application guards.
use crate::{app::App, panel_layout::PanelLayout};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Focus,
    Step(isize),
    Column(bool),
    Page(bool),
    First,
    Last,
    Open,
    Parent,
    Mark,
}
pub fn apply(app: &mut App, index: usize, command: Command) {
    apply_with_selection(app, index, command, false);
}
pub(crate) fn apply_with_selection(app: &mut App, index: usize, command: Command, mark: bool) {
    if index >= app.panels.len() {
        return;
    }
    app.active = index;
    let brief = app.config.layouts[index] == PanelLayout::Brief;
    let panel = &mut app.panels[index];
    let previous = panel.cursor;
    match command {
        Command::Focus => app.quick_view = None,
        Command::Step(amount) => panel.step(amount),
        Command::Column(right) if brief => panel.move_column(right),
        Command::Page(down) => {
            let capacity = panel.viewport_rows * if brief { 3 } else { 1 };
            panel.step(if down {
                capacity as isize
            } else {
                -(capacity as isize)
            });
        }
        Command::First => panel.cursor = 0,
        Command::Last => panel.cursor = panel.entries.len().saturating_sub(1),
        Command::Open => app.open_current(),
        Command::Parent if app.config.backspace_parent => panel.parent(app.config.show_hidden),
        Command::Mark => {
            panel.toggle_selection();
            panel.step(1);
        }
        _ => {}
    }
    if mark {
        let panel = &mut app.panels[index];
        if !panel.loading {
            let start = previous.min(panel.cursor);
            let end = previous.max(panel.cursor);
            let names: Vec<_> = panel
                .entries
                .get(start..=end)
                .unwrap_or_default()
                .iter()
                .filter(|entry| !entry.parent)
                .map(|entry| entry.name.clone())
                .collect();
            for name in names {
                panel.mark_name(name, true);
            }
        }
    }
}
