# Multi-document foundations

Turbo Vision owns native selection and document windows. The UI keeps its existing
full-screen appearance; Alt+0 / Windows… switches among the manager and any number
of editors/viewers. Tetris keeps a single instance and its own internal pages.

## Identity and lifecycle

`Desktop` owns content under permanent, non-reusable `WindowId` values. A kind is
a classification, not a unique key. Controllers address an active document or an
explicit ID; polling enumerates every live document without changing native focus.
The registry tracks MRU order for fallback and quit review, while the window list
sorts IDs into stable creation order with the manager first. Untitled labels use
stable session numbers.

Path spelling is recorded immediately. Workers resolve canonical paths, merge
aliases and retire clean loading placeholders into existing documents. Existing
buffers, edits and undo history survive merging. Resolution does not activate
anything unless the placeholder remains selected. Editors and viewers have
separate reuse namespaces. Explicit text/hex requests reload the existing viewer.
Hard-link inode merging is intentionally absent.

Save As reserves both the requested spelling and its resolved destination. It
waits for pending editor identities before checking the resolved destination,
refuses another editor's ownership/reservation and releases reservations on
failure. Opening a reserved destination activates its saving editor. Existing
safe-publication, conflict, link and metadata protections remain in force. A
controller close waits for an in-flight save before releasing the document.

Quit reviews editors in MRU order with the existing Save/Discard/Cancel choices.
Approvals do not remove buffers. Cancel or settings-save failure keeps every
buffer; completed saves stay saved. Workspace export/operation guards run before
exit. Documents and session IDs are not persisted.

## Modal ownership

`ModalStack` owns typed dialog payloads and entries containing `ModalId`,
workspace/document owner, parent ID and return-document focus. Hosts live with the
workspace services or their document. Their IDs establish one visible stack and
painting order; input reads that same stack. Native input receipts include the
modal instance and owner, so a dismissed dialog cannot target its replacement.

Nested help/confirmations retain their parent. Closing a child restores its live
parent; retiring a document prunes its modal children. A temporary take/restore
inside one controller transaction retains the original modal ID and worker scope.
Blocking dialogs prevent window-list switching. Existing action enums/controllers
remain; this introduces no command bus.

## Background results

`RequestScope` assigns an owner, request slot and non-reusable generation. Tasks
hold weak leases: replacing a slot or dropping its owner makes old completions
ineligible. Successful one-shot replies consume their generation. Dedicated panel,
traversal and file-operation streams validate the same token before each event.
The bounded short-task pool and existing dedicated workers remain.

Documents poll independently of focus. Background loads/saves update their owner
without activation. An inactive failed load retains a placeholder and an owned
error for the user's return; a foreground failed load returns to the previous
window and presents the error. Definition-edit and archive-preview opens check
whether their original focus intent is still current. External-opener effects
validate panel intent while process handles remain workspace-owned and reaped.
File-operation jobs and export publication remain workspace-owned.

## Preparation and painting

`layout::prepare_window` receives bounds, screen clip and time. It updates derived
viewports, wrapping/clamping and game obscuration, then produces `LayoutSnapshot`.
`ui::render` takes immutable application/snapshot references and returns one
`RenderedFrame` containing the scene, cursor and mouse regions. Primitive `Frame`
has no application-specific manager/game hooks.

One transformation translates local cells, cursor and hit regions and clips them
to window/screen bounds. Wide glyphs crossing a clip boundary are suppressed.
Publishing associates mouse regions with the document, modal and layout revision.
Geometry/content changes invalidate old targets, including scans, scrolling,
editor revisions, viewer loads, menu changes and Tetris pages. Painting does not
read time or mutate document models.

## Validation scope

Validated locally on macOS ARM64: 423 tests pass, with the existing opt-in native
Trash test ignored. Formatting, all-target Clippy, release build and the
no-default-features all-target check pass. Real terminal workflows cover three
editors and two viewers, Open, independent undo, dirty close/quit cancellation,
retained discard-approved buffers, Save As, resize, shell handoff and restoration.

The macOS native suite covers independent edits/undo/search/scroll, file and alias
reuse, pending collisions, explicit viewer modes, destination reservations,
save failure/retry, targeted/stale close, quit cancellation/settings failure,
nested modal restoration/retirement, delayed request replacement/owner retirement,
focus intent and repeated immutable rendering with translated coordinates.
Existing appearance, Unicode, native terminal-output and filesystem safeguards
remain in the complete suite. Formatting, all-target Clippy, release and
no-default-features checks are required alongside terminal workflow checks.

Windows/Linux and Intel Mac runtime validation are not claimed. Movable windows,
shared-buffer views, workspace persistence, large-file storage, panel-source
abstractions and command-routing redesign remain outside this work.
