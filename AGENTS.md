# Working on Directory Navigator

These instructions apply throughout this repository. Follow the user's current
request and preserve unrelated work already present in the checkout.

## Release rebuild policy

**After every change other than documentation-only or CI-only edits, rebuild DN
in release mode before reporting completion. This includes UI and application
fixes, refactors, dependencies and ordinary test changes.**

Documentation-only and CI-only changes do not require rebuilding the app. Run
relevant documentation, workflow-policy and release-helper checks instead. The
exemption applies only when application code, dependencies and compilation inputs
are unchanged; mixed changes still require a release rebuild.

```sh
cargo build --release --locked
./target/release/dn --version
```

On Windows, run `target\release\dn.exe --version` instead. Use the configured
toolchain and Cargo target directory; identify the actual output path if a custom
target or target directory is in use.

- The user normally runs `./target/release/dn`. Passing debug tests does not update
  that executable. Verify the release build succeeds after the final edits.
- For UI or interaction fixes, exercise the rebuilt release executable in a real
  terminal, preferably with `tests/terminal_walkthrough.rs`. Cover the reported
  scenario, including populated/empty state, idle redraws and focus changes when
  relevant. Confirm terminal restoration after exit.
- If a build or walkthrough cannot finish, explain the failure and executable
  status plainly. Never describe an older binary as containing the fix.
- Rebuilding does not imply installing, signing, publishing or replacing a running
  process. Tell the user to restart an existing instance when needed.

## Project and source map

DN is a Rust terminal file manager using Turbo Vision. Keep the DN appearance,
Unicode support, shortcuts and mouse behavior unless the request changes them.

- `README.md`: build/run instructions and release workflows.
- `docs/USER_GUIDE.md`: supported behavior and shortcuts.
- `docs/DOCUMENT_FOUNDATIONS.md`: document identity, ownership and lifecycle.
- `docs/SHARED_UI.md`: shared controls, commands, settings and rendering patterns.
- `docs/BUILTIN_PLUGINS.md`: internal plugin API and host responsibilities.
- `PROJECT_STATE.md`: historical implementation notes; verify current code rather
  than treating old validation counts or platform status as current evidence.
- `original-dn/`: original DOS Navigator reference sources. Adapt useful behavior
  to modern terminals and preserve attribution in `THIRD_PARTY_NOTICES.md`.

Read the relevant implementation and tests before changing behavior. Keep changes
focused; avoid adding dependencies or persistence/configuration migrations for
routine fixes.

## Architecture to preserve

- Native desktop selection, geometry and z-order govern window activation. Use
  shared windowing/focus paths rather than per-tool exceptions.
- Target documents and dialogs by live `WindowId`/`ModalId`. Preserve modal
  ownership, parent focus restoration and rejection of stale input/results.
- Independent tools remain switchable documents. Confirmations, settings and
  destination pickers retain their blocking behavior.
- Use shared `DialogSpec` controls and bounds for painting, focus traversal and
  mouse targets. Reserve button shadows and both focus markers.
- Prepare derived layout before painting. Repeated painting must be immutable;
  unchanged redraws must not change window bounds or feed dimensions back into
  their own defaults. Test several consecutive frames, not just the first.
- Keep menus and shortcuts in their shared definitions (`src/menu/definition.rs`,
  `src/commands.rs`) and existing editor/viewer adapters. Avoid duplicating
  definitions in input/render code.
- Plugins use the existing registry, limited context, shared controls and typed
  effects. The host retains terminal ownership, clipping, focus and mouse routing.
- Background work stays bounded and tied to its owner/generation. Inactive-window
  completions must not steal focus or update retired documents.

## Filesystem and lifecycle safeguards

- Preserve no-overwrite defaults, explicit confirmation, cancellation and
  captured source/panel context for file operations.
- Retain safe regular-file opens, link/identity checks, private temporary storage,
  atomic publication, locks and cleanup ownership. Never test against user data.
- Distinguish save failures before publication from durability warnings after
  publication. Preserve dirty buffers, recovery checkpoints and close/quit guards
  until their existing success conditions hold.
- Preserve native paths, Unicode, BOM/EOL and metadata handling. Commands receive
  literal arguments; do not introduce implicit shell execution.

## Validation and CI

Reproduce regressions first and add focused tests that fail for the reported bug.
Use private temporary fixtures for writes, subprocesses, recovery and cleanup.

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo check --locked --no-default-features
```

Scale test execution to the change. Run focused tests while iterating; use the full
native suite for architectural or broad behavior changes. Perform the final
release rebuild unless the documentation-only or CI-only exemption above applies.
Report what actually ran and any skipped platform checks.

**Adding or changing tests must not break CI.** Before reporting completion:

- Inspect the current workflow matrix and commands. Run the affected test suite
  and applicable CI checks locally, including all-target Clippy so test code is
  checked with warnings treated as errors.
- Review new tests for macOS ARM64/Intel, Linux and Windows compatibility, even
  when runtime validation on a platform is out of scope. Avoid platform-specific
  paths, shell commands, timestamp precision and filesystem assumptions; gate
  genuinely platform-specific tests explicitly.
- Keep tests independent and safe under parallel execution. Use private fixtures,
  bounded waits and observable state transitions rather than timing assumptions.
  Repeat timing-sensitive regressions and run their containing suite together to
  catch interference; do not hide failures with retries or longer timeouts.
- If CI results for the change are available, inspect relevant failures and fix
  regressions before calling it CI-verified. Local success alone does not prove
  the platform matrix passes. State unavailable or skipped checks explicitly;
  do not push or trigger workflows without authorization just to obtain results.

- For UI changes, check English/Russian, default/custom colors, 80×25 and larger
  terminals, resize, clipping, cursor/hit alignment and immutable redraws as relevant.
- For release/workflow changes, also run
  `python3 -m unittest discover -s scripts -p 'test_*.py'` and workflow policy tests.
- Keep workflows manual-only and release write permissions isolated. Preserve
  manual publication and collision checks unless explicitly asked to change them.
- Test workflow mutations under LF, CRLF and mixed line endings. Avoid Unix-only
  executables in portable tests and assumptions about timestamp precision, file
  handle access, lock lifetime or filesystem ordering.
- Fix warnings on their affected platforms; do not weaken `-D warnings`, skip
  failing tests or widen tolerances without establishing the intended behavior.
- Keep Rust compilation target triples separate from user-facing asset names.

## Working-tree discipline and handoff

- Stay on the current branch unless asked otherwise. Preserve existing staged,
  unstaged and untracked work; do not reset or clean the checkout.
- Do not commit, push, tag or publish unless the user requests it.
- Check `git diff --check` and inspect the final diff for unintended edits.
- Report the outcome, cause, meaningful validation and limitations concisely.
  Include release-build status, and distinguish local evidence from CI results.
