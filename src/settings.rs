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
    pub(crate) mask_cursors: [usize; 5],
    pub(crate) plugins: Vec<(crate::plugins::PluginId, &'static str)>,
    pub(crate) preview_time: chrono::DateTime<chrono::Local>,
}
impl Settings {
    pub fn new(kind: SettingsKind, config: &Config) -> Self {
        Self {
            kind,
            draft: config.clone(),
            cursor: 0,
            mask_cursors: std::array::from_fn(|i| config.highlight_groups.0[i].masks.len()),
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
        if self.kind == SettingsKind::Highlight {
            self.draft.highlight_groups.0.len() * 3 + 2
        } else if self.kind == SettingsKind::Plugins {
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
        if self.kind == SettingsKind::Highlight {
            self.cursor = index;
            if index == 0 {
                self.draft.highlight_files = !self.draft.highlight_files;
            } else if index == 16 {
                self.draft.highlight_groups = Default::default();
                self.mask_cursors =
                    std::array::from_fn(|i| self.draft.highlight_groups.0[i].masks.len());
            } else if (1..=15).contains(&index) {
                let g = &mut self.draft.highlight_groups.0[(index - 1) / 3];
                match (index - 1) % 3 {
                    0 => g.enabled = !g.enabled,
                    2 => g.color = (g.color + 1) % 16,
                    _ => {}
                }
            }
            return;
        }

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

pub fn paste(app: &mut App, text: &str) {
    if app.modals.settings_choice().is_some() {
        return;
    }
    if let Some(s) = app.modals.settings_mut().filter(|s| {
        s.kind == SettingsKind::Highlight && (2..=14).contains(&s.cursor) && s.cursor % 3 == 2
    }) {
        let i = (s.cursor - 2) / 3;
        let old = s.draft.highlight_groups.0[i].masks.clone();
        let cursor = s.mask_cursors[i];
        crate::dialog::paste_input(
            &mut s.draft.highlight_groups.0[i].masks,
            &mut s.mask_cursors[i],
            text,
        );
        if s.draft.highlight_groups.0[i].masks.len() > 512 {
            s.draft.highlight_groups.0[i].masks = old;
            s.mask_cursors[i] = cursor;
        }
    }
}
pub fn accept(app: &mut App) {
    if let Some(s) = app.modals.settings() {
        if let Err(e) = s.draft.highlight_groups.validate() {
            app.error(e.to_string());
            return;
        }
    }

    if let Some(settings) = app.modals.take_settings() {
        app.apply_config(settings.draft);
        app.notice = "Options applied · Saved on exit when enabled; Save desktop saves now".into();
    }
}
pub fn key(app: &mut App, code: KeyCode) {
    if code == KeyCode::F(1)
        && app
            .modals
            .settings()
            .is_some_and(|s| s.kind == SettingsKind::Highlight)
    {
        app.push_dialog(crate::dialog::Dialog {title:" Highlight groups help ",message:app.config.language.text("Enable a group, edit semicolon-separated * and ? masks, then choose color 0–15.\nMatching ignores case. The first matching enabled group wins.\nFolders, links, marks and the cursor keep their own colors.\nArrow keys or Space change color; Tab/Shift+Tab navigate.\nDefaults resets sample groups to disabled. OK applies; Esc cancels.").into()});
        return;
    }

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
