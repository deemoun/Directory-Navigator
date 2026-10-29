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
mod controller;
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
    let Some(settings) = app.modals.settings_mut() else {
        return;
    };
    let navigation = match controller::key(settings, code) {
        controller::Key::Handled => return,
        controller::Key::Choice(row) => {
            open_choice(app, row);
            return;
        }
        controller::Key::Controls(code) => code,
    };
    // Build the same definition used by painting. Drop its immutable borrow
    // before applying any controller-owned draft or modal changes.
    let proposal = {
        let Some(spec) = crate::ui::settings_spec(app, crate::turbo::dialog_spec::input_area(app))
        else {
            return;
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
    dispatch(app, controller::Action::OpenChoice(row));
}
pub fn choice_key(app: &mut App, code: KeyCode) {
    let Some(choice) = app.modals.settings_choice_mut() else {
        return;
    };
    match controller::choice_key(choice, code) {
        controller::ChoiceKey::Handled => {}
        controller::ChoiceKey::Cancel => app.modals.set_settings_choice(None),
        controller::ChoiceKey::Accept => {
            let choice = app.modals.take_settings_choice().unwrap();
            if let Some(settings) = app.modals.settings_mut() {
                controller::apply_choice(settings, choice);
            }
        }
    }
}
/// The host alone commits configuration and owns the parent/child presentations.
fn dispatch(app: &mut App, action: controller::Action) {
    let Some(settings) = app.modals.settings_mut() else {
        return;
    };
    let effect = controller::reduce(settings, action, app.config.language);
    match effect {
        controller::Effect::None => {}
        controller::Effect::Choice(choice) => app.modals.set_settings_choice(Some(choice)),
        controller::Effect::Accept => accept(app),
        controller::Effect::Cancel => app.modals.set_settings(None),
    }
}
pub(crate) fn action(app: &mut App, target: crate::mouse::Target) {
    if let Some(action) = controller::command(target) {
        dispatch(app, action);
    }
}
