use crate::{app::App, config::Config};
use crossterm::event::KeyCode;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsKind {
    Keyboard,
    Regional,
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
            SettingsKind::Keyboard => " Keyboard shortcuts ",
            SettingsKind::Regional => " Language / date & time ",
            SettingsKind::System => " System Setup ",
            SettingsKind::Interface => " Interface Setup ",
            SettingsKind::FileManager => " File Manager Setup ",
            SettingsKind::Highlight => " Highlight Groups ",
        }
    }
    pub fn rows(&self) -> &'static [Setting] {
        match self.kind {
            SettingsKind::Keyboard => &[],
            SettingsKind::Regional => &[],
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
    pub fn control_count(&self) -> usize {
        if self.kind == SettingsKind::Keyboard {
            1
        } else if self.kind == SettingsKind::Regional {
            5
        } else {
            self.rows().len()
        }
    }
    pub fn keyboard_choice(&mut self, reverse: bool) {
        if self.kind == SettingsKind::Keyboard {
            self.cursor = 0;
            self.draft.shortcuts = self.draft.shortcuts.next(reverse);
        }
    }
    pub fn regional_choice(&mut self, index: usize, reverse: bool) {
        if self.kind != SettingsKind::Regional || index >= 5 {
            return;
        }
        self.cursor = index;
        match index {
            0 => {
                self.draft.language =
                    if self.draft.language == crate::localization::Language::English {
                        crate::localization::Language::Russian
                    } else {
                        crate::localization::Language::English
                    }
            }
            1 => {
                self.draft.datetime.date_order = self.draft.datetime.date_order.next();
                if reverse {
                    self.draft.datetime.date_order = self.draft.datetime.date_order.next();
                }
            }
            2 => {
                let choices = ['-', '.', '/'];
                let i = choices
                    .iter()
                    .position(|c| *c == self.draft.datetime.date_separator)
                    .unwrap_or(0);
                self.draft.datetime.date_separator = choices[(i + if reverse { 2 } else { 1 }) % 3];
            }
            3 => self.draft.datetime.twelve_hour = !self.draft.datetime.twelve_hour,
            4 => {
                self.draft.datetime.time_separator = if self.draft.datetime.time_separator == ':' {
                    '.'
                } else {
                    ':'
                }
            }
            _ => {}
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
        *value = crate::turbo::widgets::toggle(*value);
    }
}

pub fn accept(app: &mut App) {
    if let Some(settings) = app.modals.take_settings() {
        app.apply_config(settings.draft);
        app.notice = "Options applied · Saved on exit when enabled; Save desktop saves now".into();
    }
}
pub fn key(app: &mut App, code: KeyCode) {
    if app.modals.settings_choice().is_some() {
        choice_key(app, code);
        return;
    }
    if code == KeyCode::F(4) {
        if let Some(settings) = app.modals.settings() {
            open_choice(app, settings.cursor);
        }
        return;
    }
    let Some(settings) = app.modals.settings_mut() else {
        return;
    };
    let count = settings.control_count();
    if settings.kind == SettingsKind::Keyboard
        && settings.cursor == 0
        && matches!(
            code,
            KeyCode::Left | KeyCode::Right | KeyCode::Char(' ' | 'p' | 'P')
        )
    {
        settings.keyboard_choice(code == KeyCode::Left);
        return;
    }
    if settings.kind == SettingsKind::Regional {
        match code {
            KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') if settings.cursor < count => {
                settings.regional_choice(settings.cursor, code == KeyCode::Left);
                return;
            }
            _ => {}
        }
    }
    match code {
        KeyCode::Esc => app.modals.set_settings(None),
        KeyCode::Enter if settings.cursor == count + 1 => app.modals.set_settings(None),
        KeyCode::Enter => accept(app),
        KeyCode::Tab => {
            settings.cursor = crate::control_focus::cycle(settings.cursor, count + 2, false)
        }
        KeyCode::BackTab => {
            settings.cursor = crate::control_focus::cycle(settings.cursor, count + 2, true)
        }
        KeyCode::Up => {
            settings.cursor = crate::control_focus::cycle(settings.cursor, count + 2, true)
        }
        KeyCode::Down => {
            settings.cursor = crate::control_focus::cycle(settings.cursor, count + 2, false)
        }
        KeyCode::Char(' ') if settings.cursor < count => settings.toggle(settings.cursor),
        KeyCode::Char(' ') if settings.cursor == count => accept(app),
        KeyCode::Char(' ') => app.modals.set_settings(None),
        KeyCode::Char(c) => {
            if let Some(index) = settings
                .rows()
                .iter()
                .position(|setting| app.config.language.matches(setting.label(), c))
            {
                settings.toggle(index);
            }
        }
        _ => {}
    }
}

/// A child choice list stays inside the parent's draft transaction. No setting
/// is applied to the application until the parent dialog accepts.
pub struct ChoiceDialog {
    pub kind: SettingsKind,
    pub row: usize,
    pub items: Vec<String>,
    pub cursor: usize,
}
pub fn choice_items(
    settings: &Settings,
    row: usize,
    language: crate::localization::Language,
) -> Vec<String> {
    fn label(settings: &Settings, row: usize, language: crate::localization::Language) -> String {
        if settings.kind == SettingsKind::Keyboard {
            return language.text(settings.draft.shortcuts.name()).into();
        }
        match row {
            0 => settings.draft.language.name().into(),
            1 => language
                .text(match settings.draft.datetime.date_order {
                    crate::regional::DateOrder::Ymd => "Year–month–day",
                    crate::regional::DateOrder::Dmy => "Day–month–year",
                    crate::regional::DateOrder::Mdy => "Month–day–year",
                })
                .into(),
            2 => settings.draft.datetime.date_separator.to_string(),
            3 => language
                .text(if settings.draft.datetime.twelve_hour {
                    "12 hour"
                } else {
                    "24 hour"
                })
                .into(),
            4 => settings.draft.datetime.time_separator.to_string(),
            _ => String::new(),
        }
    }
    let mut copy = Settings::new(settings.kind, &settings.draft);
    let mut items = Vec::new();
    for _ in 0..8 {
        let value = label(&copy, row, language);
        if items.contains(&value) {
            break;
        }
        items.push(value);
        if copy.kind == SettingsKind::Keyboard {
            copy.keyboard_choice(false);
        } else {
            copy.regional_choice(row, false);
        }
    }
    items
}
pub fn open_choice(app: &mut App, row: usize) {
    let Some(settings) = app.modals.settings() else {
        return;
    };
    if !matches!(
        settings.kind,
        SettingsKind::Keyboard | SettingsKind::Regional
    ) || row >= settings.control_count()
    {
        return;
    }
    let items = choice_items(settings, row, app.config.language);
    let kind = settings.kind;
    app.modals.set_settings_choice(Some(ChoiceDialog {
        kind,
        row,
        items,
        cursor: 0,
    }));
}
pub fn choice_key(app: &mut App, code: KeyCode) {
    let Some(choice) = app.modals.settings_choice_mut() else {
        return;
    };
    if let Some(row) =
        crate::turbo::widgets::list_key(choice.cursor, choice.items.len(), choice.items.len(), code)
    {
        choice.cursor = row;
        return;
    }
    match code {
        KeyCode::Esc | KeyCode::F(4) => app.modals.set_settings_choice(None),
        KeyCode::Enter | KeyCode::Char(' ') => {
            let choice = app.modals.take_settings_choice().unwrap();
            if let Some(settings) = app.modals.settings_mut().filter(|s| s.kind == choice.kind) {
                for _ in 0..choice.cursor {
                    if settings.kind == SettingsKind::Keyboard {
                        settings.keyboard_choice(false);
                    } else {
                        settings.regional_choice(choice.row, false);
                    }
                }
                settings.cursor = choice.row;
            }
        }
        _ => {}
    }
}
