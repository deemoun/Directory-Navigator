//! The only bridge between App and plugin callbacks.
use super::*;
use crate::{app::App, desktop::WindowKind, focus::Focus, painting::Frame};
pub(crate) fn active(app: &App) -> Option<Binding> {
    if app.output_visible || app.screensaver.active {
        return None;
    }
    if let Some(m) = app.modal_top() {
        return app
            .modals
            .plugin_dialog()
            .filter(|id| app.config.plugin_enabled(*id))
            .and_then(|id| app.desktop.plugins.binding(id))
            .filter(|b| b.owner == Owner::Modal(m.id));
    }
    if let WindowKind::Plugin(id) = app.desktop.active().kind {
        if !app.config.plugin_enabled(id) {
            return None;
        }
        return app
            .desktop
            .plugins
            .binding(id)
            .filter(|b| b.owner == Owner::Document(app.desktop.active().id));
    }
    None
}
fn alive(app: &App, b: Binding) -> bool {
    app.config.plugin_enabled(b.plugin)
        && app.desktop.plugins.binding(b.plugin) == Some(b)
        && match b.owner {
            Owner::Modal(id) => {
                app.modals.plugin_dialog() == Some(b.plugin)
                    && app.modal_infos().iter().any(|m| m.id == id)
            }
            Owner::Document(id) => app.desktop.contains(id),
        }
}
pub(crate) fn context(app: &App, b: Binding, area: Rect, now: Instant) -> Option<Context> {
    if !alive(app, b) {
        return None;
    }
    let scope = match b.owner {
        Owner::Modal(id) => ControlScope::Modal(*app.modal_infos().iter().find(|m| m.id == id)?),
        Owner::Document(window) => ControlScope::Document {
            window,
            revision: app.layout_revision,
        },
    };
    Some(Context {
        binding: b,
        scope,
        area,
        language: app.config.language,
        palette: app.config.palette,
        active: active(app) == Some(b)
            && !app.output_visible
            && !app.screensaver.active
            && app.utilities.size.0 >= 80
            && app.utilities.size.1 >= 25,
        now,
        storage: app.config_store.path().ok().map(PathBuf::from),
    })
}
pub(crate) fn open(app: &mut App, id: PluginId) {
    if !app.config.plugin_enabled(id) {
        app.notice = "Plugin is disabled. Enable it in Options > Configuration > Plugins.".into();
        return;
    }

    let Some(e) = app.desktop.plugins.entries.get(&id) else {
        return;
    };
    let definition = e.tool.definition();
    let opening = OpenContext {
        job: app.job.is_some(),
        blocking: app.modal_top().is_some_and(|m| {
            !matches!(
                m.focus,
                Focus::Menu | Focus::QuickSearch | Focus::EditorMenu
            )
        }),
        viewer: app.desktop.active().kind == WindowKind::Viewer,
    };
    // A tool cannot bypass the workspace's blocking-modal rule.
    if opening.blocking || !e.tool.can_open(&opening) {
        return;
    }
    crate::menu::close(app);
    app.modals.set_quick_search(None);
    if let Some(b) = app.desktop.plugins.binding(id).filter(|b| alive(app, *b)) {
        if let Owner::Document(w) = b.owner {
            app.desktop.activate(w);
        }
        return;
    }
    let owner = match definition.presentation {
        Presentation::Dialog => {
            app.modals.set_plugin_dialog(Some(id));
            let m = app
                .modals
                .infos()
                .filter(|m| m.focus == Focus::Plugin)
                .last()
                .unwrap();
            Owner::Modal(m.id)
        }
        Presentation::Document => match app.desktop.open_plugin(id) {
            Ok(w) => Owner::Document(w),
            Err(e) => {
                app.error(e.to_string());
                return;
            }
        },
    };
    let b = Binding { plugin: id, owner };
    app.desktop.plugins.bind(b);
    if let Some(ctx) = context(
        app,
        b,
        crate::turbo::dialog_spec::input_area(app),
        Instant::now(),
    ) {
        app.desktop
            .plugins
            .entries
            .get_mut(&id)
            .unwrap()
            .tool
            .open(&ctx);
    }
}
pub(crate) fn dispatch(app: &mut App, b: Binding, event: Event<'_>) {
    if active(app) != Some(b) {
        return;
    }
    let Some(ctx) = context(
        app,
        b,
        crate::turbo::dialog_spec::input_area(app),
        Instant::now(),
    ) else {
        return;
    };
    if let Event::Key(key) = event {
        let tool = &app.desktop.plugins.entries[&b.plugin].tool;
        if tool.uses_controls(key) {
            let proposal = tool.controls(&ctx).key(key);
            if let Some(p) = proposal {
                if !p.current(app) {
                    return;
                }
                let tool = &mut app.desktop.plugins.entries.get_mut(&b.plugin).unwrap().tool;
                let mut effects = tool.event(Event::Focus(p.focus), &ctx);
                if let Some(Target::Plugin(id, a)) = p
                    .action
                    .filter(|t| matches!(t,Target::Plugin(id,_) if *id==b.plugin))
                {
                    let _ = id;
                    effects.extend(tool.event(Event::Action(a), &ctx));
                }
                apply(app, b, effects);
                return;
            }
        }
    }
    let effects = app
        .desktop
        .plugins
        .entries
        .get_mut(&b.plugin)
        .unwrap()
        .tool
        .event(event, &ctx);
    apply(app, b, effects);
}
fn apply(app: &mut App, b: Binding, effects: Vec<Effect>) {
    for effect in effects {
        if !alive(app, b) {
            break;
        }
        // Background tools may retire themselves, but interactive services belong
        // to the foreground presentation and cannot steal its focus/clipboard.
        if !matches!(&effect, Effect::Close) && active(app) != Some(b) {
            continue;
        }
        match effect {
            Effect::Close => match b.owner {
                Owner::Modal(_) => {
                    app.modals.set_plugin_dialog(None);
                    app.desktop.plugins.retire(b);
                }
                Owner::Document(w) => app.desktop.close(w),
            },
            Effect::CopyText(text) => app.editor_clipboard = text,
            Effect::Help { title, text } => app.push_dialog(crate::dialog::Dialog {
                title,
                message: text,
            }),
        }
    }
}
pub(crate) fn controls(app: &App, area: Rect) -> Option<DialogSpec<'_>> {
    let b = active(app)?;
    let ctx = context(app, b, area, app.desktop.plugins.now)?;
    Some(
        app.desktop
            .plugins
            .entries
            .get(&b.plugin)?
            .tool
            .controls(&ctx),
    )
}
pub(crate) fn draw(frame: &mut impl Frame, app: &App, area: Rect, b: Binding) {
    let Some(ctx) = context(app, b, area, app.desktop.plugins.now) else {
        return;
    };
    let Some(e) = app.desktop.plugins.entries.get(&b.plugin) else {
        return;
    };
    match e.tool.view(&ctx) {
        View::Dialog(s) => s.paint(frame, app),
        View::Surface(scene, s) => {
            frame.compose(scene);
            s.append(frame);
        }
    }
}
pub(crate) fn poll(app: &mut App, now: Instant) -> bool {
    let mut changed = update_activity(app, now);
    for b in app.desktop.plugins.bound() {
        let Some(ctx) = context(app, b, crate::turbo::dialog_spec::input_area(app), now) else {
            app.desktop.plugins.retire(b);
            continue;
        };
        let tool = &mut app.desktop.plugins.entries.get_mut(&b.plugin).unwrap().tool;
        if tool.next_wake(now) == Some(Duration::ZERO) {
            let effects = tool.event(Event::Tick, &ctx);
            apply(app, b, effects);
            changed = true;
        }
    }
    changed
}
/// Preparation delivers activity transitions before the immutable frame is built.
pub(crate) fn update_activity(app: &mut App, now: Instant) -> bool {
    app.desktop.plugins.now = now;
    let mut changed = false;
    for b in app.desktop.plugins.bound() {
        let Some(ctx) = context(app, b, crate::turbo::dialog_spec::input_area(app), now) else {
            continue;
        };
        let entry = app.desktop.plugins.entries.get_mut(&b.plugin).unwrap();
        if entry.active != Some(ctx.active) {
            entry.active = Some(ctx.active);
            let effects = entry.tool.event(Event::Activity(ctx.active), &ctx);
            apply(app, b, effects);
            changed = true;
        }
    }
    changed
}
pub(crate) fn next_wake(app: &App, now: Instant) -> Option<Duration> {
    app.desktop
        .plugins
        .entries
        .values()
        .filter(|e| e.binding.is_some())
        .filter_map(|e| e.tool.next_wake(now))
        .min()
}
pub(crate) fn key(app: &mut App, key: KeyEvent) {
    if let Some(b) = active(app) {
        dispatch(app, b, Event::Key(key));
    }
}
pub(crate) fn paste(app: &mut App, text: &str) {
    if let Some(b) = active(app) {
        dispatch(app, b, Event::Paste(text));
    }
}
pub(crate) fn mouse(app: &mut App, event: MouseEvent, target: Option<Target>) {
    if let Some(b) = active(app) {
        let a = match target {
            Some(Target::Plugin(id, a)) if id == b.plugin => Some(a),
            _ => None,
        };
        dispatch(app, b, Event::Mouse(event, a));
    }
}

pub(crate) fn reconcile(app: &mut App) {
    for b in app.desktop.plugins.bound() {
        if !alive(app, b) {
            app.desktop.plugins.retire(b);
        }
    }
}

/// Apply only committed preferences; disabling closes the presentation but retains
/// the module's session state (including pending score records).
pub(crate) fn apply_enabled(app: &mut App) {
    for b in app.desktop.plugins.bound() {
        if !app.config.plugin_enabled(b.plugin) {
            match b.owner {
                Owner::Document(id) => app.desktop.close(id),
                Owner::Modal(_) => {
                    if app.modals.plugin_dialog() == Some(b.plugin) {
                        app.modals.set_plugin_dialog(None);
                    }
                    app.desktop.plugins.retire(b);
                }
            }
        }
    }
    app.sync_modals();
}
