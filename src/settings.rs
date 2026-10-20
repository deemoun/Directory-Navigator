use crate::{app::App, config::Config};
use crossterm::event::KeyCode;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsKind {
    System,
    Interface,
    FileManager,
    Highlight,
}
#[derive(Clone, Copy)]
pub enum Setting {
    SaveOnExit,
    Clock,
    Hidden,
    Highlight,
    Tags,
    Space,
    DeleteKey,
    Trash,
    Backspace,
}
impl Setting {
    pub fn label(self) -> &'static str {
        match self {
            Self::SaveOnExit => "Save settings on e~x~it",
            Self::Clock => "Show ~c~lock",
            Self::Hidden => "Show ~h~idden files",
            Self::Highlight => "~H~ighlight file types",
            Self::Tags => "~T~ag character",
            Self::Space => "~S~PACE toggles selection",
            Self::DeleteKey => "~D~EL erases file(s)",
            Self::Trash => "Move deleted files to T~r~ash",
            Self::Backspace => "BS - go to ~u~pper directory",
        }
    }
}

pub struct Settings {
    pub kind: SettingsKind,
    pub draft: Config,
    pub cursor: usize,
}
impl Settings {
    pub fn new(kind: SettingsKind, config: &Config) -> Self {
        Self {
            kind,
            draft: config.clone(),
            cursor: 0,
        }
    }
    pub fn title(&self) -> &'static str {
        match self.kind {
            SettingsKind::System => " System Setup ",
            SettingsKind::Interface => " Interface Setup ",
            SettingsKind::FileManager => " File Manager Setup ",
            SettingsKind::Highlight => " Highlight Groups ",
        }
    }
    pub fn rows(&self) -> &'static [Setting] {
        match self.kind {
            SettingsKind::System => &[Setting::Hidden, Setting::SaveOnExit],
            SettingsKind::Interface => &[Setting::Highlight, Setting::Tags, Setting::Clock],
            SettingsKind::FileManager => &[
                Setting::Space,
                Setting::DeleteKey,
                Setting::Backspace,
                Setting::Tags,
                Setting::Highlight,
                Setting::Trash,
            ],
            SettingsKind::Highlight => &[Setting::Highlight],
        }
    }
    pub fn value(&self, setting: Setting) -> bool {
        match setting {
            Setting::SaveOnExit => self.draft.save_on_exit,
            Setting::Clock => self.draft.show_clock,
            Setting::Hidden => self.draft.show_hidden,
            Setting::Highlight => self.draft.highlight_files,
            Setting::Tags => self.draft.show_tags,
            Setting::Space => self.draft.space_select,
            Setting::DeleteKey => self.draft.delete_key,
            Setting::Trash => self.draft.use_trash,
            Setting::Backspace => self.draft.backspace_parent,
        }
    }
    pub fn toggle(&mut self, index: usize) {
        let Some(setting) = self.rows().get(index) else {
            return;
        };
        self.cursor = index;
        let value = match setting {
            Setting::SaveOnExit => &mut self.draft.save_on_exit,
            Setting::Clock => &mut self.draft.show_clock,
            Setting::Hidden => &mut self.draft.show_hidden,
            Setting::Highlight => &mut self.draft.highlight_files,
            Setting::Tags => &mut self.draft.show_tags,
            Setting::Space => &mut self.draft.space_select,
            Setting::DeleteKey => &mut self.draft.delete_key,
            Setting::Trash => &mut self.draft.use_trash,
            Setting::Backspace => &mut self.draft.backspace_parent,
        };
        *value = !*value;
    }
}

pub fn accept(app: &mut App) {
    if let Some(settings) = app.settings.take() {
        app.apply_config(settings.draft);
        app.notice = "Options applied · Saved on exit when enabled; Save desktop saves now".into();
    }
}
pub fn key(app: &mut App, code: KeyCode) {
    let Some(settings) = &mut app.settings else {
        return;
    };
    let count = settings.rows().len();
    match code {
        KeyCode::Esc => app.settings = None,
        KeyCode::Enter if settings.cursor == count + 1 => app.settings = None,
        KeyCode::Enter => accept(app),
        KeyCode::Tab => settings.cursor = (settings.cursor + 1) % (count + 2),
        KeyCode::BackTab => settings.cursor = (settings.cursor + count + 1) % (count + 2),
        KeyCode::Up => settings.cursor = (settings.cursor + count + 1) % (count + 2),
        KeyCode::Down => settings.cursor = (settings.cursor + 1) % (count + 2),
        KeyCode::Char(' ') if settings.cursor < count => settings.toggle(settings.cursor),
        KeyCode::Char(' ') if settings.cursor == count => accept(app),
        KeyCode::Char(' ') => app.settings = None,
        KeyCode::Char(c) => {
            if let Some(index) = settings.rows().iter().position(|setting| {
                crate::menu::mnemonic(setting.label()) == Some(c.to_ascii_lowercase())
            }) {
                settings.toggle(index);
            }
        }
        _ => {}
    }
}
