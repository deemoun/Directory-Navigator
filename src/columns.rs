use crate::app::App;
use crossterm::event::KeyCode;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ColumnPreset {
    #[default]
    Name,
    NameSize,
    NameSizeModified,
}
impl ColumnPreset {
    pub const ALL: [Self; 3] = [Self::Name, Self::NameSize, Self::NameSizeModified];
    pub fn key(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::NameSize => "name_size",
            Self::NameSizeModified => "name_size_modified",
        }
    }
    pub fn parse(value: &str) -> Self {
        match value {
            "name" => Self::Name,
            "name_size_modified" => Self::NameSizeModified,
            "name_size" => Self::NameSize,
            _ => Self::Name,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Name => "Name only",
            Self::NameSize => "Name + Size",
            Self::NameSizeModified => "Name + Size + Modified",
        }
    }
    pub fn index(self) -> usize {
        match self {
            Self::Name => 0,
            Self::NameSize => 1,
            Self::NameSizeModified => 2,
        }
    }
}

pub struct ColumnDialog {
    pub original: [ColumnPreset; 2],
    /// Left panel, right panel, OK, Cancel.
    pub cursor: usize,
}
impl ColumnDialog {
    pub fn new(original: [ColumnPreset; 2], active: usize) -> Self {
        Self {
            original,
            cursor: active,
        }
    }
}

pub fn choose(app: &mut App, panel: usize, preset: ColumnPreset) {
    if panel < 2 {
        if let Some(dialog) = app.modals.columns_dialog_mut() {
            dialog.cursor = panel;
            app.config.columns[panel] = preset;
        }
    }
}
pub fn cancel(app: &mut App) {
    if let Some(dialog) = app.modals.take_columns_dialog() {
        app.config.columns = dialog.original;
    }
}
pub fn accept(app: &mut App) {
    app.modals.set_columns_dialog(None);
    app.notice = "Columns applied · Saved on exit when enabled; Save desktop saves now".into();
}
pub fn key(app: &mut App, code: KeyCode) {
    if crate::dialog_controls::key(
        app,
        crossterm::event::KeyEvent::new(code, crossterm::event::KeyModifiers::NONE),
        crate::focus::Focus::Columns,
    ) {
        return;
    }
    let Some(dialog) = app.modals.columns_dialog_mut() else {
        return;
    };
    match code {
        KeyCode::Left | KeyCode::Right | KeyCode::Up | KeyCode::Down | KeyCode::Char(' ')
            if dialog.cursor < 2 =>
        {
            let panel = dialog.cursor;
            let delta = if matches!(code, KeyCode::Left | KeyCode::Up) {
                2
            } else {
                1
            };
            app.config.columns[panel] =
                ColumnPreset::ALL[(app.config.columns[panel].index() + delta) % 3];
        }
        _ => {}
    }
}
