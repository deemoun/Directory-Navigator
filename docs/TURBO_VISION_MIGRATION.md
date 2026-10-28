# Turbo Vision migration status

Turbo Vision 4.1.0 is the sole application UI toolkit. Ratatui is absent from
Cargo.toml, Cargo.lock, source/test imports and the resolved dependency graph.
The legacy renderer, transport adapter, feature gates and duplicate pane/game
painters have been deleted. Windows and Linux validation remains out of scope
for this finishing pass. Local runtime validation targets macOS ARM64.

## Native architecture

- `TurboBackend` owns the native terminal. Application frames are prepared as
  native scenes and painted by the selected TV document. Session remains the
  sole owner of terminal input, raw mode, paste/mouse capture, panic cleanup and
  shell suspend/resume. Complete graphemes, ZWJ sequences, literal private-use
  characters, indexed/RGB/reset colors and supported modifiers are preserved.
- Native Desktop/Window identity, selection and z-order are authoritative.
  Foreground modal scopes gate input. TV Groups handle pane navigation, control
  traversal and positional mouse dispatch. Mouse/mnemonic bindings are synced
  before traversal; conditional/disabled controls stay excluded.
- Models retain filesystem policy, asynchronous workers, editing/undo/search,
  viewer navigation, game rules and save-before-close guards. Native input cannot
  bypass those guards. Existing full-screen document layouts are preserved.
- Shared dialog frames, buttons (including disabled states), input fields,
  checkboxes, radio buttons, choice fields, lists, read-only text, group boxes,
  scrollbars and activity indicators now use actual toolkit widgets through
  `turbo/widgets.rs`. This covers messages/confirmations, file-operation prompts,
  settings, editor/viewer search and numeric prompts, histories/bookmarks/windows,
  discovery/archive results, utilities and game pages. Dropdown menus and function
  bars use `MenuBox`, `MenuBar` and `StatusLine`; directory trees use `OutlineViewer`
  over DN's cancellable scan model. No toolkit directory control changes the
  process working directory.
- Controllers keep semantic state and guarded actions. Native controls are hydrated
  from that state; input editing, list/menu navigation and focus traversal use
  toolkit events. Ephemeral drawing controls do not mutate application models or
  start nested loops. The adapter preserves complete graphemes, literal private-use
  characters, clipping and cursor placement. Dialog composition reserves the native
  two-row button shadow and bottom border.
  Native button faces and menu check marks replace their hand-drawn counterparts.
- Settings choice fields open an owned native dialog/list child with F4 or the
  arrow control. Child Cancel restores the parent draft; accepting a choice still
  requires accepting the parent to apply settings. Numeric fields retain existing
  validation rather than silently
  clamping invalid entries. Unknown operation totals use an activity bar, not a
  fabricated percentage.
- The ASCII/CP437 grid reuses the library's `Table` control for cell drawing,
  keyboard navigation and mouse selection. DN supplies the byte mapping,
  single-cell highlight, details and clipboard behavior. The upstream showcase's
  `AsciiTable` is a display-only example, not an exported widget.
- Editor Open and Save As now use the library's `FileDialog`, including its
  filename input, file list, directory/wildcard handling, hidden-file toggle and
  buttons. The owned dialog is driven by the existing event loop; it never starts
  a second terminal session or nested modal loop. Preparation captures the native
  widget into an immutable scene, including its cursor and mouse scope.
  DN retains document reuse, destination reservations and safe background saves.
  The upstream picker enumerates directories synchronously and displays filenames
  as UTF-8; document loading/saving still uses the existing workers. Wide and
  combining filename input uses the native input field's UTF-8 paste path, with
  the toolkit's internal clipboard restored afterward.
- The upstream TV revision remains pinned to
  `0f2b396a1844c59e31c4714d68ffb6815662918e`. OS clipboard and screenshot
  features remain disabled. Turbo Vision is now mandatory; no Cargo feature
  selects a renderer. `--no-default-features` also builds the native application.

## Composed dialogs, panels, previews and help

Shared modal controls are actual children of one `Dialog`/`Group`, positioned
relative to its interior. DN-specific captions are immutable image views. A single
native draw paints the frame, children and shadows over the real underlying cells.
The last published frame retains the native group for button hit/command proposals;
DN controllers retain field state, focus policy, validation and destructive-action
guards. Caption images capture an opaque dialog background, so sparse text never
copies the obscured document. Selected menu rows include shortcut backgrounds.
All push buttons share one DN-style presentation: dark-gray faces, white normal
text, yellow mnemonics, and black offset shadows. Focus/default buttons have yellow
captions and cyan inward-pointing markers; disabled captions remain dim. This uses
`DIALOGS.PAS`/`TETRIS.PAS` as the behavior reference. Composed dialogs, isolated game
controls and native file-picker buttons use the same mapping and title centering.
Shadow cells use solid backgrounds to avoid font-dependent half-block seams.
Colors marks Apply as the Enter default while a palette choice has focus, then
moves the markers to Cancel when Cancel receives focus.
Embedded checkbox/radio rows fill
their entire reserved width
with the dialog background, preserving focus, mnemonics and disabled states.
Calculator history uses a one-row native ComboBox arrow rather than a two-row
Button; it cannot expand into the expression frame. Both composed and isolated
controls share one color-mapping function. Native buttons have their full two-row
bounds. Root/menu/file-picker/help
shadows use the toolkit shadow routine. Wide-cell overlay intersections are resolved
before publishing the frame and terminal diff.

File columns use native `ListBox`/`ListViewer` rendering while preserving DN's
column-major Brief layout, metadata, marks and per-entry colours. `ScrollBar`
provides tracks, fixed thumbs and arrow/page steps. DN retains its wide-integer,
endpoint-exact drag adapter rather than adopting the toolkit's coarse drag mapping.
Native `TextViewer` paints bounded, prepared text for the viewer, quick view and
information previews. Loading, wrap/search state and limits remain in DN; these
controls never load a file themselves.

Welcome reuses the Help browser as a landing page with Start, Keyboard settings
and Help action links. It retains its own owned modal and first-run/menu lifecycle;
configuration existence rules and settings saving are unchanged. Both Help and
Welcome use the original `HELPFILE.PAS` centered 72×20 geometry and a native
context status line with highlighted keys. The original English help index
(`RESOURCE/ENGLISH/DNHELP.HTX`, topic Index) likewise welcomes users through Help.
Regional settings capture one sample timestamp when opened, so repeated painting
cannot change the date/time preview or disagree with native terminal output.

Context help uses a native `HelpWindow` and topic file, with Contents, manager,
editor, viewer and operation topics. Tab/Shift+Tab select links; Enter or double
click follows them; F1 opens Contents and Alt+F1/Backspace goes back. The help
browser belongs to the existing modal instance, retains scroll history and returns
to its live parent. Unicode and literal arithmetic/markup characters are encoded
around a byte-oriented parser defect in the pinned toolkit. HelpWindow's viewer is
kept interior-relative around its upstream bounds override.

Behavior references: `original-dn/HELPFILE.PAS` (cross-references/history),
`FLPANEL.PAS`/`FLPANELX.PAS` (specialized file rows), `FVIEWER.PAS` (quick view),
and `RESOURCE/ENGLISH/DN.DNR` (dialog/button spacing). Modern safety checks,
Unicode, asynchronous ownership and full-screen documents remain in place.

## Shared control definitions

Common prompts (mkdir, go-to/import and delete/Trash confirmation) and generic
configuration settings now use one `DialogSpec` for native control factories,
ordered focus IDs, default/cancel proposals and mouse bounds. Keyboard traversal
constructs the same actual input/checkbox/choice/button types as painting, rather
than a separate group of placeholder buttons. Values, Unicode editing and limits,
configuration drafts, validation and file operations remain in their controllers.
Proposals identify their modal and owner; nested, replaced or retired owners cannot
receive stale actions. Native controls are hydrated from model state without
creating a second editable buffer or starting a nested event loop.

The pilot includes keyboard/regional settings while preserving owned choice-list
children. Editor Find/Replace, replacement confirmation, unsaved-change guards,
Go To/tab-width prompts and the legacy editor-help page now use the same definitions;
viewer search does too. Their ordinary Tab/Enter/Esc handling follows the shared
controls, while text editing, option shortcuts, replacement sessions and saving
stay in their existing document controllers. Disabled search options are excluded
from traversal and mouse targets. Invalid focus retains the declared safe default
in unsaved/replacement guards. Native FileDialog and HelpWindow integrations remain.
Advanced Copy/Move and destination history now use these definitions, including
conditional verification/source-removal controls, native history arrows and the
original focus order. Localized mnemonics come from the same control captions.
Archive/discovery, compare/export/file histories, bookmarks/filters, window and
command lists, calculator/history, character chart, information tools,
colors/screensaver, import preview, pattern/columns/file info, messages and file
progress share the structure too. Lists, menus, memos, radios, outlines and the
character table participate in native traversal as their actual control types.
Decorative/read-only content stays separate from focusable controls.

Tetris uses document-owned control definitions for Play, Setup, score/name,
game-over and Help pages, retaining its custom board, physics and score safeguards.
Its shared proposals check document identity and layout revision. The game footer
uses native StatusLine styling; export format choices use native RadioButton
markers. Tetris page Space shortcuts retain their existing game meaning.

Enabled button resolution cannot be bypassed by a list sharing the default action,
and a disabled focused control cannot submit an unrelated default. Tab recovers
to an enabled endpoint. Layout preparation tracks the definition's mouse geometry
and enabled states, retiring stale frames after asynchronous capability changes.
Radio selection does not invalidate stable row targets merely because the native
focus widget moves. No dependency, configuration format or public API change is
introduced; native FileDialog, HelpWindow and owned settings choice integrations
remain unchanged.

## Built-in tools

Calculator and Tetris now implement a small internal plugin interface over these
same native controls. Both are compiled into DN. The session registry owns their
models; Desktop document content retains a plugin reference, while calculator
presentation remains modal. Input/render dispatch uses one Plugin route with
existing owner and frame-revision checks. Tetris can be switched through the window
list and its clock is suspended under modal or inactive surfaces. Existing score
storage and calculator semantics are retained. There is no external loader, new
runtime, dependency or configuration format. See [built-in API](BUILTIN_PLUGINS.md).

## Menu visibility

`interface.hide_menu` is a saved checkbox in Options → Configuration → Interface.
Existing installations keep the visible-bar default. Hidden mode reclaims the top
row for file panes, following `ouiHideMenu` and `SETUPS.PAS::InterfaceSetup`.
F9, Alt menu shortcuts and clicking the top edge temporarily reveal the menu over
row zero; opening/closing it does not resize the panes. Closing or choosing a
command hides the temporary bar. Published menu targets sit above pane/quick-view
headers, and layout revisions reject stale targets after visibility changes.
The manager menu is always suppressed while Tetris is active. Its normal visibility
returns when the game closes. Modal blocking and game function keys stay intact.

## Multi-document foundations

Editors and viewers now have independent session documents, resolved-path reuse,
Save As reservations and MRU quit review. Owned modal IDs replace the independent
focus-priority chain. Shared owner/slot/generation request scopes guard background
results. Viewport preparation is separate from immutable painting; each rendered
frame contains its scene, cursor and translated/clipped mouse targets.
See [document foundations](DOCUMENT_FOUNDATIONS.md) for lifecycle and validation.

## Owned presentation support

- `presentation/layout.rs` owns geometry and the small layout allocator used by
  DN: fixed lengths, minimum widths, percentages and weighted flexible regions,
  horizontal/vertical layouts, spacing and established cumulative rounding.
  Flexible regions honor minimums. Over-constrained tiny viewports degrade within
  bounds; application content is suppressed below 80x25 as before. No constraint
  solver package or old layout implementation is retained.
- `presentation/style.rs` owns terminal colors and composable modifiers, including
  explicit resets and cancellation of inherited modifiers.
- `presentation/text.rs` owns styled spans/lines/text. Segmentation and width use
  unicode-segmentation/unicode-width directly; graphemes and span styles remain
  intact. These are owned implementations, not aliases into another UI toolkit.
- The painter owns block borders, text wrapping/scrolling and clearing. Its
  retained MIT-licensed word-reflow adaptation is acknowledged in
  THIRD_PARTY_NOTICES.md. That source attribution remains required even though
  the Ratatui package and renderer are gone.
- The codec has one native glyph-reference plane. Tokens survive until the
  terminal commits its diff and are then reclaimed. No reference canvas,
  background pixel plane or legacy cell type remains.
- `testing.rs` is a headless snapshot sink using production `NativeFrame` painting.
  It supplies no alternate widget renderer or terminal backend. Hidden wide-cell
  continuations are placeholders, without independently meaningful styles.

The Copy and Rename/move prompts follow `RESOURCE/ENGLISH/DN.DNR`
(`dlgCopyDialog` / `dlgRenameDialog`) and
`FILECOPY.PAS` in `original-dn/`. It uses the original DN 59×16 composition with
native radio buttons, checkboxes, history and four action buttons. Enabled options preserve
no-replace publication: refuse/skip existing top-level destinations, space
preflight, staged read-back verification and verified copy followed by guarded
source removal. History and the existing OutlineViewer directory picker are owned
children; cancellation restores the parent draft without panel navigation.
Overwrite, append, refresh and description copying remain disabled. Rename/move
locks source removal on, shares the destination history and preserves F6/Shift+F6
bases. Native exclusive rename remains the default; space checks apply only when
copying is needed. Selecting verification forces staged copying with guarded
batch cleanup.

The manager can reveal the terminal's real primary screen/scrollback. DN's
`RESOURCE/ENGLISH/DN.DNR` assigns Ctrl+O to `cmShowOutput` (saved output window)
and Alt+F5 to `cmShowUserScreen` (temporary screen reveal); `USERMENU.PAS` owns
the output scroller and `DNAPP.PAS` implements the reveal. This version uses the
terminal's own scrollback, colors and selection, without replaying a saved screen.
Classic Ctrl+O toggles, Mac Ctrl+O twice retains the existing prefix sequences,
and Alt+F5 / Manager → Terminal output are alternate entry points. A dedicated
input scope consumes keys/paste without command execution; native painting pauses
while background polling continues. Return redraws the current viewport. Finished
commands accept Ctrl+O/Esc/Enter with raw, atomic paste handling.

## Validation

Local macOS ARM64 validation covers:

- 525 native Rust tests pass; the existing opt-in native Trash check stays ignored.
- Formatting, all-target Clippy, release build and a no-default-features check.
- Built-in module extraction preserves all 128 pre/post cell/style captures.
  A test-only registered tool verifies generic menu/input/render routing without
  new central enum variants, nested ownership, stale bindings/mouse frames,
  immutable translated painting, duplicate registration and background-effect
  isolation. Release PTYs cover Calculator/Tetris state independence, game window
  switching without restart or gravity catch-up, Help/paste isolation, page/name
  cancellation, resize and terminal restoration.
- Plugins preferences use the shared native settings dialog with registry-driven
  checkboxes. Tests cover draft cancellation, enable/disable lifecycle, blocked
  launches, owned Help retirement, long-list scrolling/stale targets, both
  palettes/languages and persisted preferences. A release PTY covers toggles,
  live game closure, explicit saving, resize and terminal restoration.
- Remaining-control coverage includes 128 before/after cell/style captures in
  both palettes/languages at 80×25 and 132×40: 120 match exactly; the other eight
  differ only in the native export radio marker. Repeated painting, translated
  bounds, clipping, disabled focus, stale discovery actions, calculator child
  history and retired document proposals have regressions. Private fixture tests
  preserve no-overwrite/source-removal rules. Release PTYs exercise Copy/Move
  history/Help/Tree, refusal/skip/verification, tools and histories, native mouse
  controls, Tetris page navigation/name cancellation, resize and terminal restoration.
- Document-dialog shared controls preserve 144 before/after cell/style/hit/cursor
  captures across both palettes/languages and two sizes; the pilot's 72 captures
  also still match. Traversal, unavailable search options, paste isolation, undo,
  safe guard defaults, stale owners and translated immutable painting are covered.
  A release PTY exercises editor/viewer dialogs, replacement and dirty-close
  cancellation, resize, mouse activation and terminal restoration.
- Shared-control pilot: 72 pre/post dialog captures preserve all cells/styles/hits
  across both palettes/languages and two supported sizes. Mixed native traversal,
  disabled/default actions, stale proposals, Unicode limits, nested drafts and
  immutable translated painting have regressions. A release PTY covers common
  prompts, settings choices, cancellation, resize and terminal restoration.
- Dialog navigation regressions cover forward/reverse traversal, disabled controls,
  highlighted Enter actions, cancellation and Tetris pages. A release terminal
  session verifies these keys, isolated score writes and terminal restoration.
- 2,898 layout cases check viewport bounds. Feasible allocations compare against
  frozen pre-removal geometry; 40 wrapping cases preserve narrow widths,
  indentation, long words, blank lines, NBSP/zero-width spaces and Unicode.
- The earlier renderer-removal pass preserved 40 fixed UI scene captures. Current
  native controls intentionally use toolkit button faces and menu check marks.
  Headless wide-cell placeholders normalize hidden continuation attributes.
- 1,008 manager, 420 Tetris and 448 other-surface frame comparisons now compare
  native headless snapshots with independently decoded terminal output. These
  check the native transport and targets; they no longer invoke a legacy renderer.
  Palettes, languages, resized layouts, overlays, nested menus, viewer/editor,
  quick view, utilities, auxiliary dialogs and screensaver remain covered.
- Terminal checks also cover three retained editors and two viewers, independent
  undo, editor Open, discard/quit cancellation and Save As.
- Native file-dialog regressions cover folder navigation, filters, hidden files,
  translated mnemonics, Unicode filenames, document reuse, save conflicts,
  cancellation, nested help, immutable painting and translated mouse targets.
  A terminal workflow checks native browsing and mouse Cancel, Unicode Save As,
  resizing, quit cancellation, shell handoff and terminal restoration.
- Native-control regressions cover Unicode editing/drawing and byte limits,
  neighbour clipping, visible-row encoding, disabled menu commands, nested settings
  choices, immutable painting, stale targets and opening leaf directories by mouse.
  Terminal checks exercise native settings choices with keyboard/mouse, cancellation,
  Unicode mkdir, existing-destination refusal, help scrolling and terminal restoration.
- Composition regressions check native child ownership, full button shadows,
  bottom-border spacing and translated command hits. Help regressions cover
  topic/history scroll restoration, single/double link clicks, translated mouse
  coordinates, minimum-size recovery, Unicode and literal markup. A terminal
  workflow exercises Contents, topic activation and Alt+F1 history.
- Terminal checks cover panes, menus, Tetris, viewer search/wrap, quick view,
  calculator history/help, Unicode editor paste, unsaved-close cancel, Save As to
  disposable files, mouse actions, resize, shell input and terminal restoration.

No Windows/Linux execution, Intel Mac runtime execution or remote CI runs are
claimed. Movable/resizable/cascaded document windows remain a future product
feature; they are separate from preserving the application's existing UI.
