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
  characters, clipping, cursor placement and existing compact control bounds.
  Native button faces and menu check marks replace their hand-drawn counterparts.
- Settings choice fields open an owned native dialog/list child with F4 or the
  arrow control. Child Cancel restores the parent draft; accepting a choice still
  requires accepting the parent to apply settings. Plain-text help uses read-only
  `Memo` controls. Numeric fields retain existing validation rather than silently
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

## Validation

Local macOS ARM64 validation covers:

- 442 native Rust tests pass; the existing opt-in native Trash check stays ignored.
- Formatting, all-target Clippy, release build and a no-default-features build.
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
- Terminal checks cover panes, menus, Tetris, viewer search/wrap, quick view,
  calculator history/help, Unicode editor paste, unsaved-close cancel, Save As to
  disposable files, mouse actions, resize, shell input and terminal restoration.

No Windows/Linux execution, Intel Mac runtime execution or remote CI runs are
claimed. Movable/resizable/cascaded document windows remain a future product
feature; they are separate from preserving the application's existing UI.
