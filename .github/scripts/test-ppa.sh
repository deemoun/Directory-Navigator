#!/usr/bin/env bash
# Run inside the disposable Noble build container, with networking disabled.
set -euo pipefail
arch="$1"
test "$(dpkg --print-architecture)" = "$arch"
cd /input
sha256sum --check SHA256SUMS
sources=(/input/*.dsc)
test "${#sources[@]}" -eq 1
dpkg-source -x "${sources[0]}" /work/source
cd /work/source
dpkg-checkbuilddeps
# The source package's rules use a fresh Cargo home, --frozen and default audio.
dpkg-buildpackage --build=binary -us -uc
test "$(./target/release/dn --version)" = "Directory Navigator $(python3 -c 'import tomllib; print(tomllib.load(open("Cargo.toml", "rb"))["package"]["version"])')"
cd /work
packages=(directory-navigator_*_"$arch".deb)
test "${#packages[@]}" -eq 1
test "$(dpkg-deb --field "${packages[0]}" Architecture)" = "$arch"
dpkg-deb --field "${packages[0]}" Depends | grep -Eq '(^|, )libasound2(t64)?( |,|$)'
changes=(directory-navigator_*_"$arch".changes)
test "${#changes[@]}" -eq 1
lintian --fail-on error "${changes[0]}"
cp -- "${packages[0]}" /out/
