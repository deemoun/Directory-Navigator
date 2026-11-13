# Built-in tools

The built-in plugins are Calculator, Tetris/Pentix, Music Player, APK Workshop,
Remote connections and Nearby transfers. They are compiled into DN; there is no
dynamic loader, binary ABI, installation format, external SDK or separate plugin
configuration format. This is an internal Rust interface which
may evolve with the application.

## Enable and disable

Options → Configuration → Plugins lists every registered built-in tool, including
disabled tools. Checkboxes edit a draft: OK applies it, Cancel discards it.
Tools are enabled by default except Music Player, which requires explicit opt-in.
Enabling Nearby transfers does not start networking. Compiled-in defaults live in `plugins::enabled_by_default`.
Explicit user choices override those defaults and survive saving/restarting. Disabling closes the current presentation and
hides its menu entry and blocks shortcut launches. Enabling restores the menu
entry without opening the tool automatically. Calculator
session history and pending Tetris score records survive disabling.

The preference uses the existing configuration file and save behavior (Save desktop
or save on exit). Its optional TOML v2 section is:

```toml
[plugins]
disabled = ["tetris"]
enabled = ["mp3_player"]
```

Older configurations without explicit overrides use the compiled-in defaults.
The legacy disabled list remains supported; the optional enabled list records
explicit opt-ins, including default-off plugins. IDs cannot appear in both lists.
Unknown IDs in either list are retained on save. No configuration version change
is required.

## Contract

`plugins::ToolPlugin` supplies a `Definition`, opening availability, open/event/close
callbacks, an optional wake deadline, controls and an immutable view. A definition
has a stable `PluginId`, localized title/menu label, existing shortcut label, menu
optional named root category and either Dialog or Document presentation. Existing launch shortcuts
remain in DN's shortcut maps and dispatch the generic `menu::Action::Plugin(id)`
or host opening operation. The manager hierarchy, built-in plugin captions,
shortcut labels and placement live in `src/menu/definition.rs`. Built-ins derive
menu metadata from that hierarchy; removing their entry leaves them registered
and accessible through existing launch shortcuts. Other registered tools may
request an optional named root category; entries not already placed are appended
to that category, independently of its display position.

Events cover keys, atomic paste, local `ActionId` values, mouse input, focus,
activity transitions and timer ticks. The host runs ordinary Tab/Shift+Tab,
Enter/Esc and button Space through the plugin's actual shared native controls.
A plugin can retain specialized key behavior, as Tetris does for Space.

Callbacks receive a small `Context`: presentation owner, viewport, language,
palette, activity, injected time, typed music defaults, the Nearby transfers
presentation snapshot and the existing configuration storage path.
They never receive `App` or own the terminal/event loop. Effects are typed requests for closing their own presentation, copying text
through the host clipboard bridge (with its internal fallback), opening owned
Help, captured panel paths, validated music preferences, APK operations, remote
connections and Nearby transfer actions. Interactive Copy/Help effects require the foreground binding;
background activity cannot change another tool's focus or clipboard. Filesystem
publication, networking and score validation remain host responsibilities; plugins
do not receive a generic filesystem, configuration-writing or terminal API.

A view is either a shared dialog or a custom native scene plus shared controls.
Preparation delivers activity changes before capturing the frame; painting reads
model state and uses the last injected time. The host retains clipping, cursor,
mouse publication, native focus and terminal restoration.

## Lifetime and ownership

The registry lives for the entire application session in `Desktop.plugins`.
Document content stores a plugin ID rather than a game model. Each presentation
binds the plugin to a fresh existing `ModalId` or `WindowId`; callbacks and effects
are checked against that live binding. Published mouse frames retain the existing
owner/layout revision checks. Blocking dialogs and terminal-output mode prevent
input from reaching underlying tools. Closing a document retires its binding and
owned children through the existing document/modal lifecycle.

There is one presentation per built-in tool. Calculator retains its expression,
result, base and history across closes; opening resets focus and its history page.
Tetris releases its current game on close, while the score store and pending
unsaved records remain in the module for later reopening/retry. The existing
`tetris.conf` path, format, DOS import, merging and write safeguards are unchanged.
Calculator, Tetris and Music Player are singleton documents in the window list.
Calculator now uses Document presentation while retaining its shared native
form, session history and controls. Main menus and window switching remain
available; owned decision pages keep their blocking behavior.
Tetris appears in the window list; inactivity, blocking dialogs, output/screensaver
and viewports below 80×25 suspend its clock without catch-up.

Compatibility launch/action helpers in `utilities` and `tetris`, and typed state
accessors used by tests, forward to the registry. They are not a separate plugin
controller or a stable external API. Other utilities and the editor/viewer remain
on their existing integrations.

## Editing menus and command bars

Edit `src/menu/definition.rs`: `ROOTS` sets the category order, caption and ordered
entries; `submenu_items` describes all nested manager menus. `EDITOR_ROOTS`
defines editor categories, their enabled state, entries and separators.
`EDITOR_BAR` and `VIEWER_BAR` define the bottom command bars, including captions,
explicit shortcut labels, actions, gaps and widths. The viewer retains its command
bar without gaining a dropdown menu. Moving an entry preserves its existing key;
blank editor function-key slots stay explicit entries. `item(caption, shortcut,
action)` adds a command or submenu link, and `line()` places a separator. Captions
use the existing `~mnemonic~` syntax and remain localization keys; add translations
to the language catalogs when introducing new captions. Shortcut text describes
existing input bindings; changing a displayed label does not rebind a key.

Named `RootId` values keep F9, sort shortcuts and plugin registration independent
of category positions. `EditorRootId` similarly keeps F10 opening File regardless
of the editor category order; Tab/Shift+Tab derive the enabled categories from
the same definition. Reordering an existing category or moving a command among
root menus requires no changes to input or rendering code. Command execution stays
in `menu::execute`; adding new behavior still requires its implementation.

## Adding another built-in tool

1. Add a module implementing `ToolPlugin`, with its own state and local action IDs.
2. Register its constructor in `Runtime::default`; use the existing dialog and
   scene primitives from its view implementation.
3. Declare its menu entry and presentation. No new central Focus, WindowKind,
   mouse-target or command variant is needed: Plugin is the shared route.

The test-only counter tool verifies generic registration, duplicate rejection,
menu activation, keyboard/paste/mouse routing, nested Help, immutable translated
painting, document reuse and retirement. Calculator/Tetris tests retain their
original behavior checks, including Unicode, history, page focus, physics, score
failure recovery and cancellation.

## Validation

macOS ARM64: 538 native Rust tests pass; the existing opt-in native Trash test is
ignored. Formatting, all-target Clippy, release and no-default-features checks pass.
All 128 pre/post cell/style captures match across both palettes/languages and two
terminal sizes. Release PTYs cover module/window switching, inactive gravity,
calculator state, owned Help/paste isolation, native controls, score-name
cancellation, resize and terminal restoration. Plugins settings additionally cover
draft cancellation, registry-driven lists, long-list scrolling/stale targets,
disable/re-enable, owned Help retirement and configuration safeguards. A release
PTY verifies toggles, blocked shortcuts, live game retirement, explicit saving,
resize and terminal restoration. Windows/Linux validation is skipped.


## Music Player

The opt-in singleton player keeps its DN-style compact 36×9 and expanded 66×18
windows. Native zoom, movement, arrangement, palette, clipping, owned Help and
stale-surface guards use the existing shared windowing paths.

The host captures panel paths for Utilities → Add music to player. The typed
`RequestMusicFile { play }` effect opens an owned DN file picker using the active
panel's real filesystem directory. Its result remains inside the live player;
Open adds and plays, while Playlist → Add appends only. Picker keyboard/mouse
input is owner-bound, translated for its moved native page, and its scene is
prepared before immutable painting. `PlayMp3(PathBuf)` remains the typed Enter
request. MP3, FLAC, WAV/WAVE and Ogg Vorbis use the shared format registry.
Playlist is a live session queue: Enter plays, removal commits immediately,
Close keeps changes. No queue paths, credentials, or picker location are persisted.
Settings retains its draft/OK/Cancel behavior. Inline volume and Shuffle apply
immediately; Repeat uses an owned Off/One/All page with apply/cancel. The expanded
presentation derives separate metadata, one timer and display-only position/volume
meters from existing state. Compact controls retain their previous layout. The queue has no unavailable Save button.

On supported audio builds, the optional `plugins.mp3_player.play_on_enter` preference routes regular
supported music files through a typed `PlayMp3(PathBuf)` event before custom associations.
Explicit OS opening bypasses that route. The player reuses a matching native path
or imports the captured single file and starts it after successful completion;
marks and the current queue remain independent. The host validates `Mp3Setup`
effects and updates only saved seek/default-volume fields. Plugins never write
configuration themselves; normal DN saving policy applies. Missing fields default
to external opening, a five-second seek step and 50% volume.

The public-facing name is Music Player; internal plugin IDs, typed events and
`plugins.mp3_player` keys retain their legacy names for configuration compatibility.
The shared menu definition supplies mnemonic S and preserves the legacy 3 alias.
FLAC STREAMINFO, WAV format/data chunks and Ogg Vorbis headers/comments supply
bounded tags and duration. Ogg pages are checked for CRC, sequence, continuation
and end granules; unsupported codecs or chained streams fail clearly. No media
samples or artwork are decoded during import.

Each track carries optional duration. MP3 import uses trusted Xing/Info/VBRI metadata
through the existing decoder, supplied only bounded initial MPEG frames, or a
cancellable frame-header scan for CBR/VBR streams without trustworthy metadata.
No audio device is opened and full sample decoding is unnecessary. Playback
snapshots reconcile live queue durations by entry ID. Drawing reads
these values without changing state. Time switches elapsed/remaining track time;
Settings applies seek/default-volume drafts; inline volume remains session-only.

macOS, Linux and Windows use Rodio with playback and MP3/FLAC/PCM-WAV/Vorbis features. One lazily started worker per
plugin owns the decoder and output device. A fixed-size shared mailbox coalesces
requests; UI callbacks never open audio files or seek synchronously. Files use
`filesystem::open_regular`. Generation checks reject superseded loads and
snapshots. Stop/close silence the published player immediately, invalidate pending
loads, and let the worker release the device. The idle worker is reused across
window openings and exits when its engine is dropped.

`next_wake`/`Tick` poll snapshots while playing or paused, including inactive
windows, without requesting focus or dialogs. Rendering is immutable. End of track
advances once; end of queue or an error stops. The default Cargo feature `audio` enables Rodio/Symphonia on macOS, Linux and
Windows. One shared compile-time capability gates player controls, imports, menu
actions, plugin effects and internal file opening. Builds without that feature
compile without those dependencies and retain the unavailable window, disabled
audio/import controls and external opening. Device availability is checked
lazily, not when enabling the plugin. Tests use an injected engine so ordinary CI needs no audio hardware.

The import worker uses the existing bounded task pool with a document-owned
request scope. One pending batch is retained per player. Cancellation discards
the entire batch; the live owner applies completed results atomically to the
committed queue or its draft. Traversal checks directory identities, avoids
symlinks, sorts names and bounds queue capacity, enumerated entries (10,000) and
nesting (64). Errors/limits are reported without losing readable tracks. Progress
atomics are copied into UI state during Tick; painting never reads mutable worker
progress directly.

Read-only ID3 parsing reuses Symphonia 0.5.5. Tags are limited to 1 MiB, individual
text frames to 16 KiB and displayed fields to 512 characters. Only artist, title
and album text reaches the metadata parser; artwork is skipped. ID3v1 supplies
missing fields. File handles use safe regular opens and identity checks.

Shuffle applies immediately; Repeat opens an owned Off/One/All selector. Playback order uses stable
entry IDs independently of visible queue order. A shuffled cycle anchors the
current entry, permutes the rest and supports Previous; Repeat All wraps that
same cycle. Repeat One applies only at EOF. Queue changes rebuild the cycle;
repeat-only mode changes preserve it. Modes/import/Info work on every supported audio build. Shuffle/repeat remain
session-local; seek/default-volume preferences use the existing settings bridge.

## APK Workshop

`apk_workshop` is an opt-in document on every platform. Its typed host effects
open snapshot viewers, real project files in the editor, the working directory
in the manager, or a captured archive extraction request. Native paths are never
constructed from display labels; SDK jobs use literal arguments and never invoke
apktool or an implicit command shell. Each live binding owns its request scope,
private staging and logcat process. Closing cancels work and retires its pages;
inactive completions update only that tool and cannot open another document.

Projects are explicit persistent user directories, not DN settings or system-temp
workspaces. SDK path edits and the last device/export are session-only. The
existing configuration mechanism stores only the plugin's enabled override.
See the user guide for supported binary structures and editing restrictions.

## Remote connections

`remote_connections` is a default-enabled, owned blocking connection dialog.
The plugin only edits drafts and connection profiles and emits typed
`RemoteConnected` or `RemoteTerminal` effects. The host validates the emitting
plugin, owns pane installation and terminal handoff, and keeps remote paths
separate from native filesystem paths. Credentials are never serialized.

Protocol workers use a separate eight-slot bounded task budget with request
leases. Listings and previews are tied to the pane generation. A connection that
finishes while inactive waits for activation; it cannot replace another pane or
steal document focus. File operations use typed remote requests in Transfer Center;
no fabricated native paths enter the native operation planner. Remote editor
working copies have private cleanup ownership and are excluded from history,
session snapshots; recovery retains only untitled text without a remote path. Interactive terminal sessions delegate to installed
OpenSSH through the existing literal-argument command runner.

The saved-profile list and its Cancel-default deletion confirmation are owned pages
of the connection dialog. Page transitions retire their action targets. Edit/Delete
capture profile metadata and reject concurrent changes before atomic publication;
profile removal does not retire active remote connections or transfers.

## Nearby transfer ownership

`nearby_transfers` presents discovery, verification and offer approval through
shared controls. Typed effects request capture, Receive/Stop, connect, comparison,
accept/decline or an owned destination Tree picker. The host validates the live
binding, captures native sources and destinations and runs the lazy Iroh service.
A dedicated runtime uses bounded queues; overflow rejects unaccepted work and
retains a failure notice. Discovery and pairing events carry the window generation.
Inactive completions update their own document without activating it.

An accepted typed peer request belongs to Transfer Center, with workspace-owned
cancellation, conflict decisions and guarded publication. Closing the plugin
retires unaccepted work and stops advertising, while accepted jobs retain their
session. Pairing trust and endpoint keys remain in memory until DN exits.

Nearby discovery snapshots carry an owner generation, a refresh request and a
binding revision. The host rejects stale snapshots independently of accepted
workspace transfer jobs. Refresh uses the same session key/trust and defers socket
rebinding until idle; presentation remains read-only.

Nearby connection tests use a separate ALPN and no file manifest or transfer job. The host owns the in-memory identity/nickname, interface snapshots and bilateral verification. Nicknames are explicit session-only hints; they are never derived from usernames/hostnames or persisted. Device info uses an owned shared help document.

Nearby text sharing uses `dn-text/1`, host-owned offers/receipts and a 64 KiB UTF-8
limit. It never enters the filesystem-transfer queue. The receiver's live owner
must retain the body before receipt; a shared reservation covers both the network
exchange and outstanding preview. Scoped clipboard reads capture binding, composer
session, buffer revision, cursor and selection; stale replies are discarded. Copy
status updates target the received-item serial, including inactive live owners.
