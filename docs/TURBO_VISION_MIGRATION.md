# Turbo Vision migration status

The default runtime draws all application surfaces through native Turbo Vision
cells and routes positional mouse hits through a TV Group. This includes the
file panes, dropdown menus, shared dialogs, quick view, editor/viewer, utilities,
welcome/settings, auxiliary tools and all Tetris pages. The native runtime does
not call Ratatui widget renderers or compose these screens in a Ratatui buffer.

The macOS runtime conversion is complete for the current full-screen application
surfaces: production uses Turbo Vision's terminal directly, without a Ratatui
Terminal, Frame, Buffer or widget renderer. This does not mean the Ratatui package
has been removed. Its text/style/layout **data types and layout solver** remain
behind one compatibility module, alongside the isolated comparison renderer.
Native Intel Mac, Windows and Linux execution remains unvalidated; Windows/Linux
validation was explicitly excluded from this finishing pass.

## Native rendering and input

- Upstream TV 4.1.0 remains pinned to
  `0f2b396a1844c59e31c4714d68ffb6815662918e`, enabled by default. OS clipboard
  and screenshot features remain disabled.
- `painting::Frame` and the shared block/styled-text/clear/list primitives have
  independent native and reference implementations. Screen code shares layout
  and command descriptions; production `TurboBackend::draw_app` calls
  `ui::native_scene`/`NativeFrame` directly. `ui::draw` and the retained
  `ui::draw_native` compatibility entry point serve reference/headless tests.
- The native painter preserves inherited styles, dimmed menu shadows, centered
  titles, wrapped/scrolled Unicode text, editor highlighting/selection, search
  matches, focused controls and cursor requests. Clearing a menu over a wide
  glyph repairs its retired continuation cell. The old overlay-marker bridge
  has been removed.
- Native desktop/window identity, selection and z-order remain authoritative for
  documents. Foreground modal scopes gate delivery. Stable application WindowIds
  map to opaque native ViewIds without narrowing to u16.
- Persistent native pane controls handle Tab focus and focused navigation,
  paging, marking and guarded open/parent commands. Welcome and operation/path
  prompts retain native Dialog/Group focus and Button dispatch, with the custom
  Unicode field used instead of stock InputLine semantics.
- Control traversal in menus, settings, columns/colors, screensaver setup,
  viewer/editor search, calculator/ASCII utilities, navigation/file tools, archive,
  discovery, tree, user menu, window list, command history, config import and game
  setup now goes through `control_focus` and a TV Group. Disabled/conditional
  choices are excluded. Mouse/mnemonic model bindings are synchronized before
  each traversal, including when returning from a nested overlay. Existing native
  welcome/path-operation Dialog/Button dispatch remains in place.
- A cached native Group of hit views performs front-to-back positional dispatch
  for each current target layout. Overlapping modal surfaces consume hits before
  background views. The original mouse payload reaches domain controllers for
  wheel, drag, double-click and modifier handling. The default path no longer
  selects targets by reverse-searching the application's rectangle vector.
- Existing editor buffers, undo/search/storage, viewer navigation, file-operation
  guards, asynchronous workers and command controllers are reused. A native
  button or hit view cannot bypass save-before-close or filesystem safety checks.

## Transport and compatibility

One Session still owns raw mode, paste, mouse capture, delayed release draining,
exit/panic restoration and shell suspend/resume. There is one terminal input
reader. The native differential renderer transports complete graphemes, wide
cells, ZWJ sequences and literal private-use characters through the bounded codec.
ANSI/xterm indexed colors, RGB/reset colors and supported styles are preserved.
Cursor requests are committed after painting.

Ratatui is confined to these compatibility boundaries:

- `presentation.rs`: reexports only layout/style/text data. This isolates imports;
  it does **not** replace their implementations or erase their type identity.
- `reference_ui.rs`: reference widgets, reference Frame implementation and the
  legacy native-frame test entry point. Shared native primitives own their border
  and wrapping options; no Ratatui widget options leak into native painting.
- `turbo/reference.rs`: Ratatui Backend comparison adapter for comparison tests.
  Production never allocates its retained reference canvas/pixels; resize and
  clear/resume tests check this. The codec now uses an owned grapheme/color cell.
- `terminal.rs`: the old Terminal/CrosstermBackend is available only with
  `--no-default-features --features legacy-ui`. Ratatui has no enabled features
  in the default dependency graph; `legacy-ui` alone enables its Crossterm backend.

Formats, configuration and filesystem policy are unchanged. The native reflow
adaptation's MIT notice is in THIRD_PARTY_NOTICES.md, already included by release
packaging. No remote workflows were run.

## Validation

Local macOS ARM64 checks pass:

- 401 native Rust tests pass; one existing opt-in native Trash test stays ignored.
- Formatting, all-target Clippy and release build.
- 382 legacy comparison tests, including appearance/dialog/editor/utility/
  command/game/terminal checks; the same opt-in native Trash check stays ignored.
- 1,008 file-manager frame pairs, 420 Tetris frame pairs and 448 requested-surface
  comparisons through the **production direct host**: text and foreground/
  background colors are decoded independently
  from terminal output, with cursor and mouse target checks.
- The 56 requested-surface scenes cover both palettes/languages and two sizes:
  menus/submenus, prompts/settings, quick-view text/wrapping, viewer text/hex/wrap/
  search, editor highlighting/selection/scrolling/search/replace/dirty guards,
  calculator results/errors/history/help, ASCII/CP437, loaded system/memory
  information, auxiliary dialogs and screensaver. Production frames have no
  reference canvas; allocation checks also cover resize and clear/resume.
- Dedicated reflow comparisons cover indentation, whitespace, long words,
  explicit blank lines, NBSP/zero-width spaces and multi-scalar glyphs at narrow
  widths. Native mouse tests cover overlapping targets, modal surfaces, replaced
  layouts, outside hits and coordinate bounds. Existing mouse/controller tests
  execute through native positional routing in the default build.
- Interactive terminal checks cover viewer search/wrap, quick view, calculator
  history/help, Unicode editor paste, unsaved-close cancel, Save As to a disposable
  file, mouse close actions, resizing and terminal restoration with subsequent
  shell input. Main-pane and Tetris terminal workflows also remain covered.

These checks establish the local native painting/dispatch increment. They do not
establish native cross-platform execution or equivalence of every stock TV control.

## Remaining work

1. Replace `presentation`'s remaining text/style/layout implementations with owned
   equivalents, then retire the reference renderer/tests and remove Ratatui from
   Cargo.toml. Terminal/frame/buffer rendering, mouse routing and control traversal
   need no further migration for this step. The codec's remaining color/style
   values use the same `presentation` boundary.
2. Native validation on Intel Mac, Windows and Linux, including Unicode,
   mouse/paste, configuration, idle workers, shell handoff and terminal cleanup.
   Windows/Linux validation is outside the requested finishing scope.

Movable/resizable/cascaded document windows remain a future product feature;
the migrated views preserve the existing full-screen layouts. Domain controllers
continue to own editing, filesystem commands and save-before-close guards. Native
traversal and custom Unicode fields deliberately retain those established rules
rather than adopting every stock toolkit control's editing semantics.
