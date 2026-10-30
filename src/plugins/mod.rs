//! Built-in tools. Internal Rust API: no binary ABI, loader or third-party SDK.
pub(crate) mod calculator;
pub(crate) mod host;
pub(crate) mod mp3_player;
pub(crate) mod tetris;

use crate::{
    desktop::WindowId,
    modals::ModalId,
    mouse::Target,
    presentation::layout::Rect,
    turbo::{
        dialog_spec::{ControlScope, DialogSpec},
        scene::Scene,
    },
};
use crossterm::event::{KeyEvent, MouseEvent};
use std::{
    any::Any,
    collections::BTreeMap,
    path::PathBuf,
    time::{Duration, Instant},
};
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PluginId(pub &'static str);
pub const CALCULATOR: PluginId = PluginId("calculator");
pub const TETRIS: PluginId = PluginId("tetris");
pub const MP3_PLAYER: PluginId = PluginId("mp3_player");
/// Compiled-in defaults are separate from explicit user overrides. Unknown tools
/// retain the original enabled default; experimental built-ins opt out here.
pub fn enabled_by_default(id: PluginId) -> bool {
    const DEFAULT_DISABLED: &[PluginId] = &[MP3_PLAYER];
    !DEFAULT_DISABLED.contains(&id)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Presentation {
    #[allow(dead_code)] // Supported for internal plugins which need a blocking presentation.
    Dialog,
    Document,
}
#[derive(Clone, Copy)]
pub(crate) struct Definition {
    pub id: PluginId,
    pub title: &'static str,
    pub menu_label: &'static str,
    pub shortcut: &'static str,
    pub menu: Option<crate::menu::RootId>,
    pub presentation: Presentation,
}
impl Definition {
    fn builtin(id: PluginId, title: &'static str, presentation: Presentation) -> Self {
        let placement = crate::menu::plugin_placement(id);
        Self {
            id,
            title,
            menu_label: placement.map_or("", |(_, item)| item.label),
            shortcut: placement.map_or("", |(_, item)| item.shortcut),
            menu: placement.map(|(root, _)| root),
            presentation,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Owner {
    Modal(ModalId),
    Document(WindowId),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Binding {
    pub plugin: PluginId,
    pub owner: Owner,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActionId(pub usize);
pub(crate) struct OpenContext {
    pub job: bool,
    pub blocking: bool,
    pub viewer: bool,
}
pub(crate) struct Context {
    pub binding: Binding,
    pub scope: ControlScope,
    pub area: Rect,
    pub language: crate::localization::Language,
    pub palette: crate::palette::PaletteProfile,
    pub active: bool,
    pub now: Instant,
    pub storage: Option<PathBuf>,
}
impl Context {
    pub fn target(&self, id: usize) -> Target {
        Target::Plugin(self.binding.plugin, ActionId(id))
    }
    pub fn dialog(
        &self,
        r: Rect,
        focus: usize,
        title: &str,
        default: usize,
        cancel: usize,
    ) -> DialogSpec<'static> {
        DialogSpec::owned(
            r,
            self.scope,
            focus,
            title,
            self.target(default),
            self.target(cancel),
        )
    }
}
pub(crate) enum Event<'a> {
    Key(KeyEvent),
    Paste(&'a str),
    Action(ActionId),
    Mouse(MouseEvent, Option<ActionId>),
    Activity(bool),
    Focus(usize),
    Tick,
}
pub(crate) enum Effect {
    Close,
    CopyText(String),
    Help { title: &'static str, text: String },
}
pub(crate) struct LocalWindow {
    pub id: u64,
    pub parent: Option<u64>,
    pub bounds: Rect,
    pub caps: crate::windowing::geometry::Capabilities,
}
pub(crate) struct Layer<'a> {
    pub interactive: bool,
    pub id: u64,
    pub scene: Scene,
    pub controls: Option<DialogSpec<'a>>,
}
pub(crate) enum View<'a> {
    Layers(Vec<Layer<'a>>),
    Dialog(Box<DialogSpec<'a>>),
}
pub(crate) trait ToolPlugin: Any {
    fn definition(&self) -> Definition;
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn can_open(&self, _: &OpenContext) -> bool {
        true
    }
    fn open(&mut self, ctx: &Context);
    fn uses_controls(&self, _: KeyEvent) -> bool {
        true
    }
    fn event(&mut self, event: Event<'_>, ctx: &Context) -> Vec<Effect>;
    fn windows(&self, _ctx: &Context) -> Vec<LocalWindow> {
        Vec::new()
    }
    /// Optional presentation toggle in place of native maximize/restore.
    fn zoom_action(&self) -> Option<ActionId> {
        None
    }
    fn controls(&self, ctx: &Context) -> DialogSpec<'_>;
    fn view(&self, ctx: &Context) -> View<'_> {
        View::Dialog(Box::new(self.controls(ctx)))
    }
    fn next_wake(&self, _: Instant) -> Option<Duration> {
        None
    }
    fn close(&mut self) {}
}
struct Entry {
    tool: Box<dyn ToolPlugin>,
    binding: Option<Binding>,
    active: Option<bool>,
}
pub struct Runtime {
    entries: BTreeMap<PluginId, Entry>,
    now: Instant,
}
impl Default for Runtime {
    fn default() -> Self {
        let mut s = Self {
            entries: BTreeMap::new(),
            now: Instant::now(),
        };
        s.register(Box::<calculator::CalculatorPlugin>::default())
            .expect("calculator ID");
        s.register(Box::<mp3_player::Mp3Player>::default())
            .expect("MP3 player ID");
        s.register(Box::<tetris::TetrisPlugin>::default())
            .expect("tetris ID");
        s
    }
}
impl Runtime {
    pub(crate) fn register(&mut self, tool: Box<dyn ToolPlugin>) -> Result<(), &'static str> {
        let id = tool.definition().id;
        if self.entries.contains_key(&id) {
            return Err("Duplicate plugin ID");
        }
        self.entries.insert(
            id,
            Entry {
                tool,
                binding: None,
                active: None,
            },
        );
        Ok(())
    }
    pub(crate) fn definitions(&self) -> impl Iterator<Item = Definition> + '_ {
        self.entries.values().map(|e| e.tool.definition())
    }
    pub(crate) fn definition(&self, id: PluginId) -> Option<Definition> {
        self.entries.get(&id).map(|e| e.tool.definition())
    }
    pub(crate) fn binding(&self, id: PluginId) -> Option<Binding> {
        self.entries.get(&id)?.binding
    }
    pub(crate) fn bound(&self) -> Vec<Binding> {
        self.entries.values().filter_map(|e| e.binding).collect()
    }
    pub(crate) fn bind(&mut self, b: Binding) {
        let e = self.entries.get_mut(&b.plugin).expect("registered plugin");
        e.binding = Some(b);
        e.active = None;
    }
    pub(crate) fn retire(&mut self, b: Binding) {
        if let Some(e) = self
            .entries
            .get_mut(&b.plugin)
            .filter(|e| e.binding == Some(b))
        {
            e.binding = None;
            e.active = None;
            e.tool.close();
        }
    }
    fn typed<T: ToolPlugin>(&self, id: PluginId) -> &T {
        self.entries[&id]
            .tool
            .as_any()
            .downcast_ref()
            .expect("builtin type")
    }
    fn typed_mut<T: ToolPlugin>(&mut self, id: PluginId) -> &mut T {
        self.entries
            .get_mut(&id)
            .unwrap()
            .tool
            .as_any_mut()
            .downcast_mut()
            .expect("builtin type")
    }
    pub fn calculator(&self) -> &crate::utilities::calculator::Calculator {
        &self.typed::<calculator::CalculatorPlugin>(CALCULATOR).state
    }
    pub fn calculator_mut(&mut self) -> &mut crate::utilities::calculator::Calculator {
        &mut self
            .typed_mut::<calculator::CalculatorPlugin>(CALCULATOR)
            .state
    }
    pub fn calculator_notice(&self) -> &str {
        &self
            .typed::<calculator::CalculatorPlugin>(CALCULATOR)
            .notice
    }
    pub fn scores(&self) -> &crate::tetris::scores::Store {
        &self.typed::<tetris::TetrisPlugin>(TETRIS).scores
    }
    pub fn scores_mut(&mut self) -> &mut crate::tetris::scores::Store {
        &mut self.typed_mut::<tetris::TetrisPlugin>(TETRIS).scores
    }
    pub(crate) fn game(&self) -> Option<&crate::tetris::Game> {
        self.typed::<tetris::TetrisPlugin>(TETRIS).game.as_ref()
    }
    pub(crate) fn game_mut(&mut self) -> Option<&mut crate::tetris::Game> {
        self.typed_mut::<tetris::TetrisPlugin>(TETRIS).game.as_mut()
    }
    pub(crate) fn install_game(&mut self, g: crate::tetris::Game) {
        self.typed_mut::<tetris::TetrisPlugin>(TETRIS).game = Some(g);
    }
}

#[cfg(test)]
mod tests;
