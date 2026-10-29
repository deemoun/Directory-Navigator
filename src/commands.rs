//! Shared command bindings, shortcut presentation and unchanged help prose.
//! Controllers retain modal precedence, text editing, validation and execution.
use crate::{
    app::App,
    editor::EditorAction as E,
    focus::{self, Screen},
    menu::Action,
    shortcuts::Profile,
    viewer::Action as V,
};
use crossterm::event::{KeyCode as K, KeyEvent, KeyModifiers as M};
use std::borrow::Cow;

pub fn label(profile: Profile, screen: Screen, classic: &str) -> Cow<'_, str> {
    if !profile.is_mac() {
        return Cow::Borrowed(classic);
    }
    if classic == "Alt+0" {
        return Cow::Borrowed("^Ou");
    }
    if let Some(n) = classic
        .strip_prefix('F')
        .and_then(|s| s.parse::<u8>().ok())
        .filter(|n| (1..=10).contains(n))
    {
        return Cow::Owned(format!("^O{}", n % 10));
    }
    let value = match screen {
        Screen::Manager => {
            if classic == "Ctrl+O" {
                "^O^O"
            } else {
                binding_label(
                    MANAGER_ALT_KEYS.iter().chain(MANAGER_CONTROL_KEYS.iter()),
                    classic,
                )
            }
        }
        Screen::Editor => binding_label(EDITOR_KEYS.iter().chain(EDITOR_ALT_KEYS.iter()), classic),
        Screen::Viewer => binding_label(
            VIEWER_KEYS.iter(),
            match classic {
                "S-F7" => "Shift+F7",
                "C-F7" => "Ctrl+F7",
                _ => classic,
            },
        ),
        Screen::Plugin(_) => classic,
    };
    Cow::Borrowed(value)
}

pub(crate) fn mac_key(app: &mut App, c: char, key: KeyEvent) -> Option<KeyEvent> {
    let mapped = |code, modifiers| Some(KeyEvent::new(code, modifiers));
    match focus::screen(app) {
        Screen::Manager => {
            let action = match c {
                'f' => Some(Action::Find),
                's' => Some(Action::QuickSearch),
                'h' => Some(Action::CommandHistory),
                'i' => Some(Action::FileInfo),
                'b' => Some(Action::Bookmarks),
                'l' => Some(Action::FilenameFilter),
                'g' => Some(Action::GoTo),
                't' => Some(Action::DirectoryTree),
                'q' => Some(Action::QuickView),
                'r' => Some(Action::Refresh),
                'e' => Some(Action::Columns),
                'p' => Some(Action::ExportList),
                'o' => Some(Action::OpenExternal),
                'y' => Some(Action::Plugin(crate::plugins::TETRIS)),
                'j' => Some(Action::History(
                    crate::file_tools::history::HistoryKind::Directory,
                )),
                'a' => Some(Action::History(
                    crate::file_tools::history::HistoryKind::Edit,
                )),
                'v' => Some(Action::History(
                    crate::file_tools::history::HistoryKind::View,
                )),
                'k' => Some(Action::Settings(crate::settings::SettingsKind::Keyboard)),
                _ => None,
            };
            if let Some(action) = action {
                crate::menu::execute(app, action);
                return None;
            }
            match c {
                'm' => mapped(K::F(9), M::NONE),
                'x' => mapped(K::F(10), M::NONE),
                'z' => {
                    crate::menu::open_submenu(
                        app,
                        crate::menu::RootId::Panel,
                        crate::menu::MenuId::Sort,
                    );
                    None
                }
                _ => None,
            }
        }
        Screen::Editor => {
            if let Some(event) = prefix_binding(EDITOR_KEYS.iter().chain(EDITOR_ALT_KEYS.iter()), c)
            {
                return Some(event);
            }
            match (c, key.code) {
                ('o', _) => mapped(K::F(3), M::NONE),
                ('f', _) => mapped(K::Char('f'), M::CONTROL),
                ('g', _) => mapped(K::Char('g'), M::CONTROL),
                ('d', _) => mapped(K::F(6), M::NONE),
                ('s', _) => mapped(K::Char('s'), M::CONTROL),
                ('c', _) => mapped(K::Delete, M::CONTROL),
                ('h', _) => mapped(K::Home, M::CONTROL),
                ('e', _) => mapped(K::End, M::CONTROL),
                ('m', _) => mapped(K::F(10), M::NONE),
                (_, K::Left | K::Right) => {
                    mapped(key.code, M::CONTROL | (key.modifiers & M::SHIFT))
                }
                (_, K::Char('<')) => mapped(K::Home, M::CONTROL),
                (_, K::Char('>')) => mapped(K::End, M::CONTROL),
                _ => None,
            }
        }
        Screen::Viewer => {
            if let Some(event) = prefix_binding(VIEWER_KEYS.iter(), c) {
                return Some(event);
            }
            match c {
                'w' => mapped(K::Esc, M::NONE),
                'f' => mapped(K::F(7), M::NONE),
                'x' => mapped(K::F(10), M::NONE),
                _ => match key.code {
                    K::Left => mapped(K::Home, M::NONE),
                    K::Right => mapped(K::End, M::NONE),
                    _ => None,
                },
            }
        }
        Screen::Plugin(_) => None,
    }
}
pub mod help {
    pub const MANAGER_HELP: &str="Mac-friendly: press Ctrl+O, release, then choose\n1 Help · 2 User · 3 View · 4 Edit · 5 Copy\n6 Move · 7 MkDir · 8 Trash/delete · 9 Menu · 0 Quit\ns Quick search · f Recursive Find · i Info · z Sort\nb Bookmarks · l Filter · e Columns · r Refresh\nh Commands · j Directories · a Edits · v Views\no OS default app · p List file · y Tetris\nk Keyboard settings · u Windows · m Menu\nUp/Down Page · Left/Right First/last entry\nCtrl+A all · Ctrl+U clear · Ctrl+O then Space marks\nCtrl+O twice: terminal output; Esc returns\nEsc or click cancels the prefix; nothing is typed.\nClassic DN keys remain available.\nCommand shortcuts belong to your terminal.\nTrash/delete still requires confirmation.";
    pub const EDITOR_HELP: &str="Mac-friendly: Ctrl+O, release, then digit or letter\n1 Help · 2 Save · 3 Open · 6 Duplicate · 7 Find · 0 Menu\na Save As · w Close · x Quit DN · m Menu · u Windows\nn Next match · p Reverse · r Replace · g Go to line\nCtrl+S Save · Ctrl+F Find · Ctrl+G Go to line\nCtrl+Z/Y Undo/Redo · Ctrl+C/X/V internal clipboard\nCtrl+O then Left/Right: word movement\nAdd Shift to the arrow to extend selection\nUp/Down Page · , / . Line start/end\nh / e Document start/end · c Clear selection\nEsc or click cancels the prefix. Classic keys still work.\nOpen/Close/Quit protect unsaved changes.\nUTF-8, 2 MiB / 100k lines. Line endings preserved.\nSave As never overwrites an existing destination.";
    pub const VIEWER_HELP: &str="Mac-friendly: press Ctrl+O, release, then choose\n2 Wrap · 3 Close · 7 Find · 0 Quit DN\nf Find · n Next match · p Reverse · w Close · u Windows\nUp/Down Page · Left/Right First/last row\nEsc or click cancels the prefix.\nClassic DN keys remain available.";
    pub const USER_MENU_HELP: &str="Ctrl+O: choose a user-menu command\n1 Help · 2 Global/Local · 4 Edit · other digits: F-keys\nr Reload · w Back/Cancel · Up/Down Page\nLeft/Right First/last item\nEsc or click cancels the prefix.\nCommands still require explicit Run confirmation.";

    pub const CLASSIC_MANAGER: &str = "Tab panels · Arrows/PgUp/PgDn/Home/End navigate\nAlt+F/U/P/M/O menus · Alt+0 windows · Alt+X exits\nEnter app/dir/archive · Ctrl+Enter OS app · F3 view · F4 edit · BS parent\nIns / Space marks · Ctrl+A all · Ctrl+U clear\n+ / - group · Alt+E info · Alt+K columns\nAlt+F7 quick search · Ctrl+F7 recursive Find\nCtrl+Q quick view · Ctrl+T tree · Alt+F9 Tetris\nCtrl+1–9 bookmarks set · Alt+1–9 go · Alt+Del filter\nCtrl+K calculator · Ctrl+B ASCII · Alt+Y memory\nCtrl+C compare · Alt+L list file\nAlt+BS directories · Alt+PgUp edits / PgDn views\nF2 user menu · Shift+F2 sort · Ctrl+F3 name / F4 ext / F5 date / F6 size\nF5 copy · F6 move · Shift+F6 rename here\nF7 mkdir · F8 Trash/delete · F9 menu · F10 exit\nCtrl+H hidden · Ctrl+L go to · Ctrl+R refresh\nClick row · Ctrl+click mark · Double-click open\nWheel scrolls · Esc cancels dialogs/operations\nType commands · Enter runs · Esc clears · Ctrl+E/X history · Alt+F8 list\nFile menu: Copy names / paths uses internal clipboard\nOptions: File associations configures Enter actions\nOptions: Import DOS settings previews DN.CFG preferences\nColors: white dirs · green exec · cyan source\nMagenta archives · blue links · yellow marks\nTrash by default; File Manager Setup changes mode.\nExisting destinations are refused; no overwrites.";
    pub const CLASSIC_EDITOR: &str = "F2 / Ctrl+S Save · Shift+F2 Save As · F3 Open\nF6 Duplicate line · F7 / Ctrl+F Find\nShift+F7 Search again · Alt+F7 Reverse search\nCtrl+F7 Replace · Alt+G / Ctrl+G Go to line\nCtrl+←/→ words · Ctrl+Shift+←/→ select words\nShift+arrows select · Ctrl+A select all\nAlt+Backspace / Ctrl+Z Undo · Ctrl+Y Redo\nCtrl+Ins/C Copy · Shift+Del / Ctrl+X Cut\nShift+Ins / Ctrl+V Paste · terminal paste supported\nAlt+B Block: selection/clipboard · Options: indent/colors/tabs\nF10 Menu · Alt+0 Windows · Ctrl+F4 / Esc Close · Alt+X Quit DN\nOpen/Close/Quit protect unsaved changes.\nUTF-8, 2 MiB / 100k lines. Line endings preserved.\nSave As never overwrites an existing destination.";
}

#[derive(Clone, Copy)]
pub(crate) struct Binding<A> {
    pub key: K,
    pub required: M,
    pub forbidden: M,
    pub action: A,
    pub classic: &'static str,
    pub mac: Option<&'static str>,
}
const fn bind<A>(
    key: K,
    required: M,
    forbidden: M,
    classic: &'static str,
    mac: Option<&'static str>,
    action: A,
) -> Binding<A> {
    Binding {
        key,
        required,
        forbidden,
        action,
        classic,
        mac,
    }
}
impl<A: Copy> Binding<A> {
    fn resolve(bindings: &[Self], key: KeyEvent) -> Option<A> {
        bindings
            .iter()
            .find(|b| {
                b.key == key.code
                    && key.modifiers.contains(b.required)
                    && !key.modifiers.intersects(b.forbidden)
            })
            .map(|b| b.action)
    }
}
const EDITOR_KEYS: &[Binding<E>] = &[
    bind(
        K::F(4),
        M::CONTROL,
        M::empty(),
        "Ctrl+F4",
        Some("^Ow"),
        E::Close,
    ),
    bind(
        K::F(7),
        M::CONTROL,
        M::empty(),
        "Ctrl+F7",
        Some("^Or"),
        E::Replace,
    ),
    bind(
        K::Insert,
        M::CONTROL,
        M::empty(),
        "Ctrl+Ins",
        Some("Ctrl+C"),
        E::Copy,
    ),
    bind(
        K::Delete,
        M::CONTROL,
        M::empty(),
        "Ctrl+Del",
        Some("^Oc"),
        E::Clear,
    ),
    bind(
        K::Char('s'),
        M::CONTROL,
        M::empty(),
        "Ctrl+S",
        None,
        E::Save,
    ),
    bind(
        K::Char('f'),
        M::CONTROL,
        M::empty(),
        "Ctrl+F",
        None,
        E::Find,
    ),
    bind(
        K::Char('g'),
        M::CONTROL,
        M::empty(),
        "Ctrl+G",
        None,
        E::GoTo,
    ),
    bind(
        K::Char('z'),
        M::CONTROL,
        M::empty(),
        "Ctrl+Z",
        None,
        E::Undo,
    ),
    bind(
        K::Char('y'),
        M::CONTROL,
        M::empty(),
        "Ctrl+Y",
        None,
        E::Redo,
    ),
    bind(
        K::Char('c'),
        M::CONTROL,
        M::empty(),
        "Ctrl+C",
        None,
        E::Copy,
    ),
    bind(K::Char('x'), M::CONTROL, M::empty(), "Ctrl+X", None, E::Cut),
    bind(
        K::Char('v'),
        M::CONTROL,
        M::empty(),
        "Ctrl+V",
        None,
        E::Paste,
    ),
    bind(
        K::Char('a'),
        M::CONTROL,
        M::empty(),
        "Ctrl+A",
        None,
        E::SelectAll,
    ),
    bind(K::F(1), M::empty(), M::CONTROL, "F1", None, E::Help),
    bind(
        K::F(2),
        M::SHIFT,
        M::CONTROL,
        "Shift+F2",
        Some("^Oa"),
        E::SaveAs,
    ),
    bind(K::F(2), M::empty(), M::CONTROL, "F2", None, E::Save),
    bind(
        K::F(3),
        M::empty(),
        M::CONTROL.union(M::SHIFT),
        "F3",
        None,
        E::Open,
    ),
    bind(
        K::F(6),
        M::empty(),
        M::CONTROL.union(M::SHIFT),
        "F6",
        None,
        E::Duplicate,
    ),
    bind(
        K::F(7),
        M::SHIFT,
        M::CONTROL,
        "Shift+F7",
        Some("^On"),
        E::FindNext,
    ),
    bind(K::F(7), M::empty(), M::CONTROL, "F7", None, E::Find),
    bind(K::F(10), M::empty(), M::CONTROL, "F10", None, E::Menu),
    bind(
        K::Insert,
        M::SHIFT,
        M::CONTROL,
        "Shift+Ins",
        Some("Ctrl+V"),
        E::Paste,
    ),
    bind(
        K::Delete,
        M::SHIFT,
        M::CONTROL,
        "Shift+Del",
        Some("Ctrl+X"),
        E::Cut,
    ),
    bind(K::Esc, M::empty(), M::CONTROL, "Esc", None, E::Close),
];
pub(crate) fn editor_key(key: KeyEvent) -> Option<E> {
    Binding::resolve(EDITOR_KEYS, key)
}
const EDITOR_ALT_KEYS: &[Binding<E>] = &[
    bind(
        K::Backspace,
        M::ALT,
        M::empty(),
        "Alt+Backspace",
        Some("Ctrl+Z"),
        E::Undo,
    ),
    bind(
        K::F(7),
        M::ALT,
        M::empty(),
        "Alt+F7",
        Some("^Op"),
        E::FindPrevious,
    ),
    bind(
        K::Char('g'),
        M::ALT,
        M::empty(),
        "Alt+G",
        Some("Ctrl+G"),
        E::GoTo,
    ),
    bind(
        K::Char('x'),
        M::ALT,
        M::empty(),
        "Alt+X",
        Some("^Ox"),
        E::Quit,
    ),
];
pub(crate) fn editor_alt(key: K) -> Option<E> {
    Binding::resolve(EDITOR_ALT_KEYS, KeyEvent::new(key, M::ALT))
}
const VIEWER_KEYS: &[Binding<V>] = &[
    bind(K::Esc, M::empty(), M::empty(), "Esc", None, V::Close),
    bind(K::F(3), M::empty(), M::empty(), "F3", None, V::Close),
    bind(K::F(2), M::empty(), M::empty(), "F2", None, V::Wrap),
    bind(
        K::F(7),
        M::CONTROL,
        M::empty(),
        "Ctrl+F7",
        Some("^Op"),
        V::Reverse,
    ),
    bind(
        K::F(7),
        M::SHIFT,
        M::empty(),
        "Shift+F7",
        Some("^On"),
        V::Next,
    ),
    bind(K::F(7), M::empty(), M::empty(), "F7", None, V::Find),
];
pub(crate) fn viewer_key(key: KeyEvent) -> Option<V> {
    Binding::resolve(VIEWER_KEYS, key)
}
/// Existing availability policy, shared by menu and function-bar presentation.
pub(crate) fn manager_available(app: &App, action: Action) -> bool {
    match action {
        Action::Plugin(id) => app.config.plugin_enabled(id),
        Action::WindowMove => crate::windowing::workspace::foreground(app).is_some(),
        Action::WindowZoom => {
            crate::windowing::workspace::foreground(app).is_some_and(|s| s.caps.zoom)
        }
        Action::Disabled | Action::Separator => false,
        _ => true,
    }
}
pub(crate) fn viewer_available(viewer: &crate::viewer::Viewer, action: V) -> bool {
    !matches!(action, V::Wrap | V::Find | V::Next | V::Reverse) || viewer.text_ready()
}

fn binding_label<'a, A: Copy + 'a>(
    mut bindings: impl Iterator<Item = &'a Binding<A>>,
    classic: &'a str,
) -> &'a str {
    bindings
        .find_map(|b| if b.classic == classic { b.mac } else { None })
        .unwrap_or(classic)
}

const MANAGER_ALT_KEYS: &[Binding<fn(&mut App)>] = &[
    bind(
        K::Delete,
        M::ALT,
        M::empty(),
        "Alt+Del",
        Some("^Ol"),
        |app| {
            crate::navigation_tools::open_filter(app);
        },
    ),
    bind(K::Char('y'), M::ALT, M::empty(), "Alt+Y", None, |app| {
        crate::utilities::open(app, crate::utilities::Kind::Memory);
    }),
    bind(
        K::Backspace,
        M::ALT,
        M::empty(),
        "Alt+Backspace",
        Some("^Oj"),
        |app| {
            crate::file_tools::open_history(
                app,
                crate::file_tools::history::HistoryKind::Directory,
            );
        },
    ),
    bind(
        K::PageUp,
        M::ALT,
        M::empty(),
        "Alt+PageUp",
        Some("^Oa"),
        |app| {
            crate::file_tools::open_history(app, crate::file_tools::history::HistoryKind::Edit);
        },
    ),
    bind(
        K::PageDown,
        M::ALT,
        M::empty(),
        "Alt+PageDown",
        Some("^Ov"),
        |app| {
            crate::file_tools::open_history(app, crate::file_tools::history::HistoryKind::View);
        },
    ),
    bind(K::F(8), M::ALT, M::empty(), "Alt+F8", Some("^Oh"), |app| {
        crate::command_line::history::open(app);
    }),
    bind(
        K::Char('l'),
        M::ALT,
        M::empty(),
        "Alt+L",
        Some("^Op"),
        |app| {
            crate::file_tools::open_export(app);
        },
    ),
    bind(K::F(9), M::ALT, M::empty(), "Alt+F9", Some("^Oy"), |app| {
        crate::plugins::host::open(app, crate::plugins::TETRIS);
    }),
    bind(K::F(7), M::ALT, M::empty(), "Alt+F7", Some("^Os"), |app| {
        crate::quick_search::open(app);
    }),
    bind(
        K::Char('x'),
        M::ALT,
        M::empty(),
        "Alt+X",
        Some("^O0"),
        |app| {
            app.request_exit();
        },
    ),
    bind(
        K::Char('e'),
        M::ALT,
        M::empty(),
        "Alt+E",
        Some("^Oi"),
        |app| {
            app.open_file_info();
        },
    ),
    bind(
        K::Char('k'),
        M::ALT,
        M::empty(),
        "Alt+K",
        Some("^Oe"),
        |app| {
            app.open_columns();
        },
    ),
    bind(K::Char('t'), M::ALT, M::empty(), "Alt+T", None, |app| {
        app.prompt_goto();
    }),
    bind(K::Char('r'), M::ALT, M::empty(), "Alt+R", None, |app| {
        app.panels[app.active].refresh(app.config.show_hidden);
    }),
    bind(K::Char('s'), M::ALT, M::empty(), "Alt+S", None, |app| {
        app.open_settings(crate::settings::SettingsKind::FileManager);
    }),
    bind(K::Char('b'), M::ALT, M::empty(), "Alt+B", None, |app| {
        crate::menu::open_submenu(app, crate::menu::RootId::Panel, crate::menu::MenuId::Sort);
    }),
];

const MANAGER_CONTROL_KEYS: &[Binding<fn(&mut App)>] = &[
    bind(
        K::Enter,
        M::CONTROL,
        M::empty(),
        "Ctrl+Enter",
        Some("^Oo"),
        |app| {
            app.open_external_current();
        },
    ),
    bind(
        K::Char('k'),
        M::CONTROL,
        M::empty(),
        "Ctrl+K",
        None,
        |app| {
            crate::plugins::host::open(app, crate::plugins::CALCULATOR);
        },
    ),
    bind(
        K::Char('b'),
        M::CONTROL,
        M::empty(),
        "Ctrl+B",
        None,
        |app| {
            crate::utilities::open(app, crate::utilities::Kind::Characters);
        },
    ),
    bind(
        K::Char('c'),
        M::CONTROL,
        M::empty(),
        "Ctrl+C",
        None,
        |app| {
            crate::file_tools::open_compare(app);
        },
    ),
    bind(
        K::Char('t'),
        M::CONTROL,
        M::empty(),
        "Ctrl+T",
        None,
        |app| {
            crate::directory_tree::open(app);
        },
    ),
    bind(
        K::Char('q'),
        M::CONTROL,
        M::empty(),
        "Ctrl+Q",
        None,
        |app| {
            crate::quick_view::toggle(app);
        },
    ),
    bind(
        K::F(7),
        M::CONTROL,
        M::empty(),
        "Ctrl+F7",
        Some("^Of"),
        |app| {
            crate::discovery::open_find(app);
        },
    ),
    bind(
        K::F(3),
        M::CONTROL,
        M::empty(),
        "Ctrl+F3",
        Some("^Oz N"),
        |app| {
            app.panels[app.active].set_sort(crate::panel::SortMode::Name);
        },
    ),
    bind(
        K::F(4),
        M::CONTROL,
        M::empty(),
        "Ctrl+F4",
        Some("^Oz E"),
        |app| {
            app.panels[app.active].set_sort(crate::panel::SortMode::Extension);
        },
    ),
    bind(
        K::F(5),
        M::CONTROL,
        M::empty(),
        "Ctrl+F5",
        Some("^Oz D"),
        |app| {
            app.panels[app.active].set_sort(crate::panel::SortMode::Date);
        },
    ),
    bind(
        K::F(6),
        M::CONTROL,
        M::empty(),
        "Ctrl+F6",
        Some("^Oz S"),
        |app| {
            app.panels[app.active].set_sort(crate::panel::SortMode::Size);
        },
    ),
    bind(
        K::Char('h'),
        M::CONTROL,
        M::empty(),
        "Ctrl+H",
        None,
        |app| {
            app.toggle_hidden();
        },
    ),
    bind(
        K::Char('l'),
        M::CONTROL,
        M::empty(),
        "Ctrl+L",
        None,
        |app| {
            app.prompt_goto();
        },
    ),
    bind(
        K::Char('a'),
        M::CONTROL,
        M::empty(),
        "Ctrl+A",
        None,
        |app| {
            app.panels[app.active].select_visible();
        },
    ),
    bind(
        K::Char('u'),
        M::CONTROL,
        M::empty(),
        "Ctrl+U",
        None,
        |app| {
            app.panels[app.active].selected.clear();
        },
    ),
    bind(
        K::Char('r'),
        M::CONTROL,
        M::empty(),
        "Ctrl+R",
        None,
        |app| {
            app.panels[app.active].refresh(app.config.show_hidden);
        },
    ),
];

pub(crate) fn manager_modified(app: &mut App, key: KeyEvent) -> bool {
    let bindings = if key.modifiers.contains(M::ALT) {
        MANAGER_ALT_KEYS
    } else {
        MANAGER_CONTROL_KEYS
    };
    if let Some(action) = Binding::resolve(bindings, key) {
        action(app);
        true
    } else {
        false
    }
}

const MANAGER_KEYS: &[Binding<fn(&mut App)>] = &[
    bind(K::Char('+'), M::empty(), M::empty(), "+", None, |app| {
        app.open_pattern(true);
    }),
    bind(K::Char('-'), M::empty(), M::empty(), "-", None, |app| {
        app.open_pattern(false);
    }),
    bind(K::Char('*'), M::empty(), M::empty(), "*", None, |app| {
        crate::menu::execute(app, Action::InvertSelection);
    }),
    bind(K::F(2), M::SHIFT, M::empty(), "Shift+F2", None, |app| {
        cycle_sort(app);
    }),
    bind(K::F(2), M::empty(), M::empty(), "F2", None, |app| {
        crate::user_menu::open(app, crate::user_menu::Source::Auto);
    }),
    bind(K::F(4), M::SHIFT, M::empty(), "Shift+F4", None, |app| {
        crate::editor::open(app, true);
    }),
    bind(K::F(4), M::empty(), M::empty(), "F4", None, |app| {
        crate::editor::open(app, false);
    }),
    bind(K::F(5), M::empty(), M::empty(), "F5", None, |app| {
        app.prompt_operation(crate::operations::Kind::Copy, false);
    }),
    bind(K::F(3), M::empty(), M::empty(), "F3", None, |app| {
        app.open_viewer();
    }),
    bind(K::F(6), M::SHIFT, M::empty(), "Shift+F6", None, |app| {
        app.prompt_operation(crate::operations::Kind::Move, true);
    }),
    bind(K::F(6), M::empty(), M::empty(), "F6", None, |app| {
        app.prompt_operation(crate::operations::Kind::Move, false);
    }),
    bind(K::F(7), M::empty(), M::empty(), "F7", None, |app| {
        app.prompt_operation(crate::operations::Kind::Mkdir, false);
    }),
    bind(K::F(8), M::empty(), M::empty(), "F8", None, |app| {
        app.prompt_operation(crate::operations::Kind::Delete, false);
    }),
    bind(K::Delete, M::empty(), M::empty(), "Del", None, |app| {
        if app.config.delete_key {
            app.prompt_operation(crate::operations::Kind::Delete, false);
        };
    }),
    bind(K::F(9), M::empty(), M::empty(), "F9", None, |app| {
        crate::menu::open_root(app, crate::menu::RootId::File);
    }),
    bind(K::F(1), M::empty(), M::empty(), "F1", None, |app| {
        crate::dialog::keyboard_help(app);
    }),
];
fn cycle_sort(app: &mut App) {
    use crate::panel::SortMode;
    let panel = &mut app.panels[app.active];
    panel.set_sort(match panel.sort {
        SortMode::Name => SortMode::Extension,
        SortMode::Extension => SortMode::Size,
        SortMode::Size => SortMode::Date,
        SortMode::Date => SortMode::Name,
    });
}
pub(crate) fn manager_key(app: &mut App, key: KeyEvent) {
    if let Some(action) = Binding::resolve(MANAGER_KEYS, key) {
        action(app);
    }
}

fn prefix_binding<'a, A: Copy + 'a>(
    mut bindings: impl Iterator<Item = &'a Binding<A>>,
    c: char,
) -> Option<KeyEvent> {
    bindings
        .find(|b| {
            b.mac.is_some_and(|mac| {
                mac.strip_prefix("^O")
                    .is_some_and(|suffix| suffix.len() == 1 && suffix.starts_with(c))
            })
        })
        .map(|b| KeyEvent::new(b.key, b.required))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn modifiers_keep_existing_command_precedence() {
        assert_eq!(
            editor_key(KeyEvent::new(K::F(7), M::CONTROL | M::SHIFT)),
            Some(E::Replace)
        );
        assert_eq!(
            viewer_key(KeyEvent::new(K::F(7), M::CONTROL | M::SHIFT)),
            Some(V::Reverse)
        );
        assert_eq!(editor_key(KeyEvent::new(K::F(3), M::SHIFT)), None);
        assert_eq!(
            editor_key(KeyEvent::new(K::Insert, M::CONTROL | M::SHIFT)),
            Some(E::Copy)
        );
        assert_eq!(editor_alt(K::Char('x')), Some(E::Quit));
    }
    #[test]
    fn presentation_reads_the_binding_metadata_and_keeps_legacy_spellings() {
        for binding in EDITOR_KEYS.iter().chain(EDITOR_ALT_KEYS) {
            if let Some(mac) = binding.mac {
                assert_eq!(label(Profile::Mac, Screen::Editor, binding.classic), mac);
            }
            assert_eq!(
                label(Profile::ClassicDn, Screen::Editor, binding.classic),
                binding.classic
            );
        }
        assert_eq!(label(Profile::Mac, Screen::Viewer, "S-F7"), "^On");
        assert_eq!(label(Profile::Mac, Screen::Viewer, "C-F7"), "^Op");
        // Menu spellings which historically stayed literal still do.
        assert_eq!(label(Profile::Mac, Screen::Manager, "Alt+PgUp"), "Alt+PgUp");
    }
}
