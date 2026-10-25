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

- 423 native Rust tests pass; the existing opt-in native Trash check stays ignored.
- Formatting, all-target Clippy, release build and a no-default-features build.
- 2,898 layout cases check viewport bounds. Feasible allocations compare against
  frozen pre-removal geometry; 40 wrapping cases preserve narrow widths,
  indentation, long words, blank lines, NBSP/zero-width spaces and Unicode.
- A before/after capture of 40 fixed UI scenes matches cells/styles/mouse targets.
  Headless wide-cell placeholders normalize hidden continuation attributes.
- 1,008 manager, 420 Tetris and 448 other-surface frame comparisons now compare
  native headless snapshots with independently decoded terminal output. These
  check the native transport and targets; they no longer invoke a legacy renderer.
  Palettes, languages, resized layouts, overlays, nested menus, viewer/editor,
  quick view, utilities, auxiliary dialogs and screensaver remain covered.
- Terminal checks also cover three retained editors and two viewers, independent
  undo, editor Open, discard/quit cancellation and Save As.
- Terminal checks cover panes, menus, Tetris, viewer search/wrap, quick view,
  calculator history/help, Unicode editor paste, unsaved-close cancel, Save As to
  disposable files, mouse actions, resize, shell input and terminal restoration.

No Windows/Linux execution, Intel Mac runtime execution or remote CI runs are
claimed. Movable/resizable/cascaded document windows remain a future product
feature; they are separate from preserving the application's existing UI.
