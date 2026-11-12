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
`Depends`. Since cargo-deb omits `libgcc-s1`, the script restores that exact
native-scanner requirement when it is the only discrepancy, rebuilds the private
package control metadata, and repeats all validation on the final package. Other
dependency mismatches remain errors. Unresolved dependencies or a missing default-audio ALSA dependency
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

## Launchpad PPA source packages (Ubuntu 24.04)

The source-package helper targets `ppa:deemoun/directory-navigator` with a single
source upload for **amd64 and arm64**. Enable both architectures on the PPA.
The target suite is **noble**. DN 1.1.10, revision 1, produces packaging version
`1.1.10-1~ppa1~noble1`; `--revision 2` produces `1.1.10-1~ppa2~noble1` for a
packaging correction. The application version and dependency lockfile are retained.

Noble's `rustc-1.97` and `cargo-1.97` packages provide Rust 1.97.1, required by
current archive dependencies. Enable Noble's `universe`, `noble-updates` and
`noble-security` repositories. The Debian rules explicitly invoke
`/usr/bin/rustc-1.97` and `/usr/bin/cargo-1.97`, retain default audio support and
use `--frozen`, a package-local Cargo home and vendored dependency sources.

### Prepare on macOS

Python 3.11+, Git and a working Rust/Cargo installation are required. From the
repository, prepare the sources before choosing the public maintainer identity:

```sh
python3 scripts/package_ppa.py --prepare-only
```

Add `--offline` when all locked crate sources are already cached. Preparation
otherwise downloads the locked dependencies. No signing key is read by this
helper. Without `--maintainer`, the staged `debian/control` and `debian/changelog`
use `@MAINTAINER@`. Supply the identity when generating the upload files.
Its output is under `target/ppa/1.1.10-1~ppa1~noble1/`:

- `directory-navigator-1.1.10/`, including the generated `debian/` directory.
- `directory-navigator_1.1.10.orig.tar.xz`, containing the source and dependencies.
- `SHA256SUMS`, covering the original tarball.

Preparation uses the working contents of tracked files from an explicit set of
source, test, documentation, resource, packaging and workflow directories, plus
the source-package helper/templates. Untracked application changes must be added
to the source inventory before preparing a release. Local credentials, build
outputs and original DOS reference sources are excluded. All generated state
stays in the ignored `target/` directory. Existing revision outputs are refused.

The original tarball excludes the top-level `debian/` directory, supplied
separately by the source package. It includes every locked dependency and its
original license files. `VENDORED_LICENSES.json` records crate versions, license
expressions, package checksums and license-file paths. `VENDORED_LICENSES.txt`
collects those notices and is installed with the other package documentation.
Review that inventory when dependencies change. Vendored file checksums are
verified against Cargo's metadata, and the original tarball is read back and
compared with the staged sources. Cargo metadata resolution is also checked for
both Linux targets using an empty Cargo home and offline mode. This checks source
completeness; it does not compile Linux binaries.

### Generate the unsigned upload bundle on Ubuntu

On Ubuntu 24.04, install the packaging tools:

```sh
sudo apt update
sudo apt install build-essential debhelper devscripts dpkg-dev lintian \
  python3 gnupg pkg-config libasound2-dev rustc-1.97 cargo-1.97
```

From a repository checkout, the helper can prepare and generate the complete
unsigned source upload in one command:

```sh
python3 scripts/package_ppa.py \
  --maintainer 'Your Name <your-verified-email@example.org>'
```

Alternatively, copy the complete macOS-prepared output directory to Ubuntu,
verify its checksum, and generate from the staged tree. First replace
`@MAINTAINER@` in the staged `debian/control` and `debian/changelog` with your
chosen `Name <email>` matching the signing key registered on Launchpad:

```sh
cd '1.1.10-1~ppa1~noble1'
sha256sum --check SHA256SUMS
cd directory-navigator-1.1.10
dpkg-buildpackage --build=source --no-check-builddeps -sa -us -uc
cd ..
lintian --fail-on error directory-navigator_1.1.10-1~ppa1~noble1_source.changes
```

The helper's Linux mode additionally verifies SHA-256 references in the `.dsc`
and `_source.changes`, extracts the `.dsc` with `dpkg-source -x`, compares the
extracted files with the staged source, and runs Lintian. Its `SHA256SUMS` covers
the unsigned upload artifacts. The bundle contains the `.dsc`, `.orig.tar.xz`,
`.debian.tar.xz`, `_source.changes` and any referenced `_source.buildinfo`.
Signing later changes metadata files; regenerate checksums after signing if you
need checksums for the signed upload.

### Validate native Noble builds

Use a clean native Noble machine or disposable VM/container for **each** of
amd64 and arm64. Install the declared build dependencies, then extract the `.dsc`
into a private working directory. Disable network access after dependency
installation. The vendored sources and fresh package-local Cargo home must
suffice for the build and tests:

```sh
dpkg-source -x directory-navigator_1.1.10-1~ppa1~noble1.dsc dn-build
cd dn-build
dpkg-buildpackage --build=binary -us -uc
cd ..
lintian --fail-on error directory-navigator_1.1.10-1~ppa1~noble1_*.changes
```

The build runs the portable helper tests, full native Rust suite and
no-default-features check unless the operator explicitly sets Debian's standard
`DEB_BUILD_OPTIONS=nocheck`. For release validation, leave that option unset.
Check the resulting `.deb` architecture, default-audio ALSA dependency and
`dn --version`. In disposable environments, test installation, upgrade,
removal, reinstallation and purge. Installing into a container does not validate
GPM, real-console behavior or audible playback. Record those checks separately.

### Manual GitHub Actions PPA validation

Select **Actions → Build PPA source package → Run workflow** after the packaging
files and workflow are available on GitHub. Enter the public **maintainer**
identity as `Name <email>` and a positive **revision** (default `1`). The workflow
uses the application version from the selected checkout and targets Noble.
The identity is supplied for that run and embedded in its package metadata.

One source job generates unsigned upload files, verifies their references and
checksums, extracts the source package and runs Lintian. Native amd64 and arm64
jobs then build the same source bundle in clean Ubuntu 24.04 containers using
the versioned Rust 1.97 toolchain. Networking is disabled during compilation and
the complete package test suite. A separate disposable container tests package
installation, reinstallation, upgrade, removal and purge.

After both architectures pass, download **dn-ppa-source-noble** from the run's
artifacts. It contains the unsigned `.dsc`, original/Debian source tarballs,
`_source.changes`, referenced `_source.buildinfo` and `SHA256SUMS`, retained for
30 days. **dn-ppa-validation-amd64** and **dn-ppa-validation-arm64** retain build
and installation logs for 30 days. A **dn-ppa-source-candidate** intermediate
artifact is retained for two days, including when native validation fails.
Use the final `dn-ppa-source-noble` artifact for the manual signing step.

The workflow has only `workflow_dispatch` and read-only repository permissions.
It saves Actions artifacts without accessing signing keys, uploading to
Launchpad, creating GitHub releases or changing tags.

### Sign and upload manually

After source and binary validation, sign with the full fingerprint of the key
registered on Launchpad. Run on a machine containing that private key and the
complete set of generated source upload files:

```sh
debsign -k YOUR_FINGERPRINT \
  directory-navigator_1.1.10-1~ppa1~noble1_source.changes
dput ppa:deemoun/directory-navigator \
  directory-navigator_1.1.10-1~ppa1~noble1_source.changes
```

The helper never invokes these commands. Watch acceptance mail and both build
results at the PPA's package page. Once accepted, a corrected upload needs a new
packaging revision. The existing binary `.deb` workflow remains a separate manual
artifact build.

References: [Launchpad source uploads](https://ubuntu.com/docs/launchpad/user/how-to/packaging/ppa-package-upload/),
[Noble Rust 1.97 packages](https://packages.ubuntu.com/noble-updates/devel/rustc-1.97),
[Cargo vendoring](https://doc.rust-lang.org/cargo/commands/cargo-vendor.html).
