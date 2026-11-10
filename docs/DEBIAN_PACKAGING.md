# Building Debian packages

`scripts/package_deb.py` prepares binary packages for downloads or your own APT
repository. It never signs, uploads, publishes repository indexes, changes the
application version, or installs anything on the build host. Existing release
workflows remain manual and unchanged.

## Build environment

Use a **native Debian or Ubuntu GNU/Linux environment** for each architecture:

| Debian architecture | Rust target | Docker platform |
| --- | --- | --- |
| `amd64` | `x86_64-unknown-linux-gnu` | `linux/amd64` |
| `arm64` | `aarch64-unknown-linux-gnu` | `linux/arm64` |

For example, build on Debian 12 or Ubuntu 24.04 using the matching architecture.
On macOS or Windows, run the script inside a matching Linux VM/container; it
rejects host binaries and cross-compilation rather than guessing runtime library
requirements. Emulation, if used, must already be configured by the operator.
Build separately for distribution releases you intend to support: a binary built
on newer glibc is not automatically usable on older systems.

Requirements: Python **3.11+**, current stable Rust/Cargo (use rustup if the
system Rust is older), and the external **cargo-deb** build tool. No additional
Rust dependency is added to DN.

Inside the Linux build environment, install:

```sh
sudo apt-get update
sudo apt-get install build-essential pkg-config libasound2-dev dpkg-dev lintian
cargo install cargo-deb --locked
```

Use `apt-get` without `sudo` in a disposable container already running as root.
The build retains the default audio feature. Build dependencies such as
`libasound2-dev` are not installed as runtime dependencies of the resulting DN
package; the runtime ALSA/glibc packages and minimum versions are resolved on the
build system. This handles differences such as `libasound2` and `libasound2t64`.

## Create a package

From the checkout:

```sh
python3 scripts/package_deb.py --arch amd64 \
  --maintainer 'Your Name <your-email@example.org>'
```

On the ARM64 build environment, use `--arch arm64` instead. Supply a real
maintainer contact; the script never invents or silently infers an email address.

The version is read from the matching `Cargo.toml` and `Cargo.lock`. Revision
`1` is the default. For example, DN 1.1.10 produces:

```text
target/debian/directory-navigator_1.1.10-1_amd64.deb
target/debian/directory-navigator_1.1.10-1_amd64.deb.sha256
```

Use `--revision 2` for a packaging-only revision without changing the application
version. SemVer prereleases use Debian's `~` ordering. `--output PATH` changes the
artifact directory. Existing packages and checksums are never overwritten; use a
new revision or output directory. Do not mix packages built on different distro
releases in one output directory with the same version/architecture filename.

Cargo-deb is called with `--locked` and an explicit native Rust target. It honors
Cargo's configured target directory for compiled artifacts. The wrapper's output
still defaults to `target/debian`, separate from the existing `dist` release
archive bundles. A generated changelog and per-checkout metadata lock also live
under `target/debian/.metadata`; these files are ignored by Git and never modify
source files. Set `SOURCE_DATE_EPOCH` to a nonnegative Unix timestamp if you need
a reproducible changelog date.

### Package contents and checks

Packages install:

- `/usr/bin/dn`, mode 0755.
- English/Russian READMEs, the user guide, MIT license, third-party notices,
  copyright information, and a Debian changelog under
  `/usr/share/doc/directory-navigator/` (documents may be compressed).
- The `dn(1)` manual page under `/usr/share/man/man1/`.

There are no maintainer scripts, services, global configuration files, or changes
to user preferences. GPM, the OpenSSH terminal client and desktop-opening tools
are optional suggested packages; SFTP/FTP/FTPS browsing uses DN's internal code.

Before exposing the output files, the script checks control metadata, package
paths/ownership/permissions, required documents against their source, ELF
architecture, and `dn --version`. It reruns `dpkg-shlibdeps` without ignoring
missing dependency information and compares the result with the package's
`Depends`. Unresolved dependencies or a missing default-audio ALSA dependency
are failures. Lintian errors fail the build; warnings remain visible and should
be reviewed before distribution. Publication to the local output directory
refuses replacement and writes a SHA-256 checksum.

## Optional disposable installation test

Docker is needed only for `--smoke-test`. Choose a **matching Debian/Ubuntu
release**, with its official APT repositories available:

```sh
python3 scripts/package_deb.py --arch amd64 \
  --maintainer 'Your Name <your-email@example.org>' \
  --smoke-test debian:bookworm-slim
```

For an Ubuntu 24.04 build use `--smoke-test ubuntu:24.04`. Use ARM64 on the matching
runner/container. This explicitly starts a disposable container, downloads its
APT dependencies, installs the package, checks its version, reinstalls it, tests
an upgrade with a temporary packaging revision, then removes/reinstalls/purges
it. Only a private copy of the package is mounted read-only; neither the checkout
nor host preferences are mounted. The container is removed afterward. Failures
prevent final package/checksum publication. No host installation is performed.

No container runtime is required for ordinary package creation; without this
option, installation/upgrade/removal have **not** been tested by the script.

## Manual GitHub Actions build

After these files are committed and pushed to the default branch, select
**Actions → Build Debian packages → Run workflow**. The workflow is
[`.github/workflows/debian.yml`](../.github/workflows/debian.yml) and accepts:

- **version**: optional; leave empty to use `Cargo.toml`. A supplied version is
  applied only in the job's checkout, without commits, tags or pushes.
- **revision**: positive Debian packaging revision; default `1`.
- **maintainer**: required real `Name <email>` contact, embedded in the package.

Two independent native Ubuntu 24.04 jobs run on `ubuntu-24.04` (amd64/x86-64)
and `ubuntu-24.04-arm` (arm64). They validate inputs, run tooling and application
tests, install the pinned cargo-deb 3.8.0 build tool, and create packages with
`--smoke-test ubuntu:24.04`. Thus package installation, reinstallation, upgrade,
removal and purge are required to succeed in a matching disposable container.
Lintian, ELF/document/dependency checks and checksum verification also gate upload.

The only outputs are **Actions artifacts**, retained for 30 days:

| Artifact | Contents |
| --- | --- |
| `dn-deb-x86_64` | amd64 `.deb` and `.deb.sha256` |
| `dn-deb-arm64` | arm64 `.deb` and `.deb.sha256` |

GitHub wraps each artifact download in a ZIP; the package inside is a `.deb`.
Artifacts target Ubuntu 24.04's runtime libraries; they are not automatically
compatible with older Ubuntu or Debian releases. This workflow has read-only
repository permissions and no GitHub release, draft release, tag, registry or
APT publication step. There are no push/PR/scheduled triggers. Adding or checking
the workflow locally does not execute it; it runs only when explicitly dispatched.

## Repository publication is separate

Review the packages, checksums and Lintian output before manually adding them to
your APT repository's correct distribution suite and architecture. Repository
indexing, signing keys and hosting are not configured by this script. Ubuntu
PPAs accept source packages; these binary artifacts are not PPA source uploads or
automatic admission to official Debian/Ubuntu repositories.

References: [cargo-deb](https://github.com/kornelski/cargo-deb),
[Debian shared-library dependency policy](https://www.debian.org/doc/debian-policy/ch-sharedlibs.html),
[Launchpad PPA requirements](https://ubuntu.com/docs/launchpad/user/reference/packaging/ppas/ppa/).
