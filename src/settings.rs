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
    Plugins,
}
pub(crate) mod choices;
pub(crate) mod definition;
pub use definition::Setting;

pub struct Settings {
    pub kind: SettingsKind,
    pub draft: Config,
    pub cursor: usize,
    pub(crate) plugins: Vec<(crate::plugins::PluginId, &'static str)>,
    pub(crate) preview_time: chrono::DateTime<chrono::Local>,
}
impl Settings {
    pub fn new(kind: SettingsKind, config: &Config) -> Self {
        Self {
            kind,
            draft: config.clone(),
            cursor: 0,
            plugins: Vec::new(),
            preview_time: chrono::Local::now(),
        }
    }
    pub fn title(&self) -> &'static str {
        self.kind.definition().title
    }
    pub fn rows(&self) -> &'static [Setting] {
        self.kind.definition().rows
    }
    pub(crate) fn choice_fields(&self) -> &'static [choices::ChoiceId] {
        match self.kind {
            SettingsKind::Keyboard => choices::KEYBOARD,
            SettingsKind::Regional => choices::REGIONAL,
            _ => &[],
        }
    }
    pub fn control_count(&self) -> usize {
        if self.kind == SettingsKind::Plugins {
            self.plugins.len()
        } else if !self.choice_fields().is_empty() {
            self.choice_fields().len()
        } else {
            self.rows().len()
        }
    }
    pub fn keyboard_choice(&mut self, reverse: bool) {
        if self.kind == SettingsKind::Keyboard {
            self.cycle_choice(0, reverse);
        }
    }
    pub fn regional_choice(&mut self, index: usize, reverse: bool) {
        if self.kind == SettingsKind::Regional {
            self.cycle_choice(index, reverse);
        }
    }
    fn cycle_choice(&mut self, index: usize, reverse: bool) {
        if let Some(choice) = self.choice_fields().get(index) {
            self.cursor = index;
            choice.definition().cycle(&mut self.draft, reverse);
        }
    }
    pub fn value(&self, setting: Setting) -> bool {
        (setting.definition().get)(&self.draft)
    }
    pub fn toggle(&mut self, index: usize) {
        if self.kind == SettingsKind::Plugins {
            if let Some((id, _)) = self.plugins.get(index) {
                self.cursor = index;
                self.draft
                    .set_plugin_enabled(*id, !self.draft.plugin_enabled(*id));
            }
            return;
        }

        let Some(setting) = self.rows().get(index) else {
            return;
        };
        self.cursor = index;
        let field = setting.definition();
        let value = crate::turbo::widgets::toggle((field.get)(&self.draft));
        (field.set)(&mut self.draft, value);
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
    // Build the same definition used by painting. Drop its immutable borrow
    // before applying any controller-owned draft or modal changes.
    let proposal = {
        let Some(spec) = crate::ui::settings_spec(app, crate::turbo::dialog_spec::input_area(app))
        else {
            return;
        };
        let navigation = match code {
            KeyCode::Up => KeyCode::BackTab,
            KeyCode::Down => KeyCode::Tab,
            _ => code,
        };
        spec.key(crossterm::event::KeyEvent::new(
            navigation,
            crossterm::event::KeyModifiers::NONE,
        ))
        .or_else(|| {
            if let KeyCode::Char(c) = code {
                spec.mnemonic(c).map(|id| spec.activate(id))
            } else {
                None
            }
        })
    };
    if let Some(proposal) = proposal {
        crate::dialog_controls::apply(app, proposal, action);
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
    settings
        .choice_fields()
        .get(row)
        .map_or_else(Vec::new, |choice| {
            choice.definition().items(&settings.draft, language)
        })
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

/// Apply a semantic proposal through the existing settings draft transaction.
pub(crate) fn action(app: &mut App, target: crate::mouse::Target) {
    use crate::mouse::Target;
    match target {
        Target::OpenSettingsChoice(row) => open_choice(app, row),
        Target::ShortcutProfile => {
            if let Some(settings) = app.modals.settings_mut() {
                settings.keyboard_choice(false);
            }
        }
        Target::RegionalChoice(index) => {
            if let Some(settings) = app.modals.settings_mut() {
                settings.regional_choice(index, false);
            }
        }
        Target::Setting(index) => {
            if let Some(settings) = app.modals.settings_mut() {
                settings.toggle(index);
            }
        }
        Target::AcceptSettings => accept(app),
        Target::CancelSettings => app.modals.set_settings(None),
        _ => {}
    }
}
