#!/usr/bin/env python3
"""Build and validate native amd64/arm64 Debian packages. Never uploads packages."""
import argparse
import contextlib
import email.utils
import gzip
import os
from pathlib import Path, PurePosixPath
import platform
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import time
import tomllib

import release

ARCHES = {"amd64": ("x86_64-unknown-linux-gnu", 62),
          "arm64": ("aarch64-unknown-linux-gnu", 183)}
NAME = "directory-navigator"
DOC = f"usr/share/doc/{NAME}/"
DOCUMENTS = {DOC + name: source for name, source in [
    ("README.md", "README.md"), ("README.ru.md", "README.ru.md"),
    ("USER_GUIDE.md", "docs/USER_GUIDE.md"), ("LICENSE", "LICENSE"),
    ("THIRD_PARTY_NOTICES.md", "THIRD_PARTY_NOTICES.md")]}
DOCUMENTS["usr/share/man/man1/dn.1"] = "packaging/debian/dn.1"
DOCUMENTS[DOC + "copyright"] = "packaging/debian/copyright"


def run(args, cwd=None):
    try:
        result = subprocess.run([str(arg) for arg in args], cwd=cwd, check=True,
                                stdout=subprocess.PIPE, text=True)
    except subprocess.CalledProcessError as error:
        if error.stdout:
            print(error.stdout[-8192:], file=sys.stderr)
        raise
    return result.stdout.strip()


def deb_version(version, revision):
    release.validate_version(version)
    if not re.fullmatch(r"[1-9][0-9]{0,8}", revision):
        raise ValueError("Revision must be a positive integer (for example 1 or 2)")
    base, plus, build = version.partition("+")
    return base.replace("-", "~", 1) + plus + build + "-" + revision


def maintainer(value):
    if any(ord(c) < 32 or ord(c) == 127 for c in value):
        raise ValueError("Maintainer must be Name <email>, without control characters")
    name, address = email.utils.parseaddr(value)
    if not name or not re.fullmatch(r"[^\s<>@]+@[^\s<>@]+\.[^\s<>@]+", address) or value != f"{name} <{address}>":
        raise ValueError("Pass --maintainer 'Your Name <your-email@example.org>'")
    return value


def environment(arch):
    if platform.system() != "Linux":
        raise ValueError("Build inside a Debian/Ubuntu Linux machine or container, not macOS/Windows")
    distro = platform.freedesktop_os_release()
    if distro.get("ID") not in ("debian", "ubuntu"):
        raise ValueError("Use a Debian or Ubuntu build environment for correct runtime dependencies")
    for tool in ("cargo", "cargo-deb", "rustc", "dpkg", "dpkg-deb", "dpkg-shlibdeps", "lintian"):
        if shutil.which(tool) is None:
            raise ValueError(f"Missing build tool: {tool}; see docs/DEBIAN_PACKAGING.md")
    if run(["dpkg", "--print-architecture"]) != arch:
        raise ValueError("Architecture must match the build environment; use a separate matching container/runner")
    host = next((line.removeprefix("host: ") for line in run(["rustc", "-vV"]).splitlines() if line.startswith("host: ")), None)
    if host != ARCHES[arch][0]:
        raise ValueError(f"Rust host must be {ARCHES[arch][0]}; cross-compilation is not supported by this wrapper")
    return distro


def metadata(root):
    manifest = tomllib.loads((root / "Cargo.toml").read_text())
    lock = tomllib.loads((root / "Cargo.lock").read_text())
    package = manifest["package"]
    if package["name"] != NAME or not any(item.get("name") == "dn" for item in manifest.get("bin", [])):
        raise ValueError("Expected directory-navigator with executable dn")
    matches = [p for p in lock["package"] if p["name"] == NAME and "source" not in p]
    if len(matches) != 1 or matches[0]["version"] != package["version"]:
        raise ValueError("Cargo.toml and Cargo.lock versions must match")
    if package.get("metadata", {}).get("deb", {}).get("depends") != "$auto":
        raise ValueError("Debian runtime dependencies must use $auto")
    return package["version"]


def elf(binary, arch):
    with binary.open("rb") as stream:
        header = stream.read(20)
    if len(header) != 20 or header[:6] != b"\x7fELF\x02\x01" or int.from_bytes(header[18:20], "little") != ARCHES[arch][1]:
        raise ValueError(f"dn must be a little-endian 64-bit Linux ELF executable for {arch}")


@contextlib.contextmanager
def archive(package, option):
    # Stream dpkg's tar output; do not buffer the executable or extract unknown paths.
    with tempfile.TemporaryFile() as errors:
        proc = subprocess.Popen(["dpkg-deb", option, str(package)], stdout=subprocess.PIPE, stderr=errors)
        try:
            with tarfile.open(fileobj=proc.stdout, mode="r|") as contents:
                yield contents
            if proc.wait() != 0:
                errors.seek(0)
                raise ValueError("dpkg-deb could not read package: " + errors.read(8192).decode(errors="replace"))
        finally:
            if proc.poll() is None:
                proc.kill()
                proc.wait()
            proc.stdout.close()


def member_name(member):
    path = PurePosixPath(member.name)
    if path.is_absolute() or ".." in path.parts or member.issym() or member.islnk() or not (member.isfile() or member.isdir()):
        raise ValueError(f"Unsafe package entry: {member.name}")
    if member.uid != 0 or member.gid != 0 or member.mode & 0o7022:
        raise ValueError(f"Unexpected ownership or permissions: {member.name}")
    return str(path)


def payload(contents, root, binary):
    seen, canonical = set(), set()
    directories = {".", "usr", "usr/bin", "usr/share", "usr/share/doc", DOC.rstrip("/"), "usr/share/man", "usr/share/man/man1"}
    for member in contents:
        name = member_name(member)
        if name in seen:
            raise ValueError(f"Duplicate package entry: {name}")
        seen.add(name)
        if member.isdir():
            if name not in directories:
                raise ValueError(f"Unexpected package directory: {name}")
            continue
        if member.size > 512 * 1024 * 1024:
            raise ValueError("Package member exceeds 512 MiB")
        normalized = name.removesuffix(".gz")
        if normalized in canonical:
            raise ValueError(f"Duplicate compressed/plain asset: {name}")
        canonical.add(normalized)
        with contents.extractfile(member) as stream:
            if name == "usr/bin/dn":
                if member.mode != 0o755:
                    raise ValueError("dn must have mode 0755")
                with binary.open("xb") as dest:
                    shutil.copyfileobj(stream, dest)
                binary.chmod(0o755)
            elif normalized in DOCUMENTS:
                expected = (root / DOCUMENTS[normalized]).read_bytes()
                reader = gzip.GzipFile(fileobj=stream) if name.endswith(".gz") else stream
                if reader.read(len(expected) + 1) != expected:
                    raise ValueError(f"Packaged document differs from source: {name}")
            elif name == DOC + "changelog.Debian.gz":
                expected = gzip.decompress((root / "target/debian/.metadata/changelog.Debian.gz").read_bytes())
                if gzip.GzipFile(fileobj=stream).read(len(expected) + 1) != expected:
                    raise ValueError("Packaged changelog differs from generated metadata")
            else:
                raise ValueError(f"Unexpected package asset: {name}")
    required = set(DOCUMENTS) | {"usr/bin/dn", DOC + "copyright", DOC + "changelog.Debian", "usr/share/man/man1/dn.1"}
    if not required <= canonical:
        raise ValueError("Missing package assets: " + ", ".join(sorted(required - canonical)))


def dependencies(value):
    return {" ".join(item.split()) for item in value.split(",") if item.strip()}


def omitted_libgcc(actual, recorded):
    """Only repair cargo-deb's intentional omission, never other mismatches."""
    required, packaged = dependencies(actual), dependencies(recorded)
    missing = required - packaged
    return (packaged < required and len(missing) == 1
            and all(re.fullmatch(r"libgcc-s1(?: \(>= [0-9][A-Za-z0-9.+:~\-]*\))?", item)
                    for item in missing)
            and not any(re.match(r"libgcc-s1(?:\s|:|$)", item) for item in packaged))


def replace_depends(control, value):
    if not value or any(char in value for char in "\r\n\0"):
        raise ValueError("Invalid replacement Depends field")
    lines = control.splitlines(keepends=True)
    starts = [i for i, line in enumerate(lines) if line.lower().startswith("depends:")]
    if len(starts) != 1:
        raise ValueError("Expected exactly one Depends control field")
    start = starts[0]
    end = start + 1
    while end < len(lines) and lines[end].startswith((" ", "\t")):
        end += 1
    return "".join(lines[:start] + [f"Depends: {value}\n"] + lines[end:])


def validate(package, root, work, arch, version, identity, app_version, repair=True):
    expected = {"Package": NAME, "Version": version, "Architecture": arch, "Maintainer": identity}
    for field, value in expected.items():
        if run(["dpkg-deb", "--field", package, field]) != value:
            raise ValueError(f"Package {field} does not match {value}")
    with archive(package, "--ctrl-tarfile") as control:
        for member in control:
            name = member_name(member)
            if name not in {".", "control", "md5sums"}:
                raise ValueError(f"Unexpected control entry/maintainer script: {name}")
    binary = work / "debian" / NAME / "usr/bin/dn"
    binary.parent.mkdir(parents=True)
    with archive(package, "--fsys-tarfile") as contents:
        payload(contents, root, binary)
    elf(binary, arch)
    if run([binary, "--version"], cwd=work) != f"Directory Navigator {app_version}":
        raise ValueError("Packaged executable version does not match Cargo.toml")
    source = work / "debian"
    source.mkdir(exist_ok=True)
    (source / "control").write_text(f"Source: {NAME}\nSection: utils\nPriority: optional\nMaintainer: {identity}\nStandards-Version: 4.7.0\n\nPackage: {NAME}\nArchitecture: any\nDescription: Terminal file manager\n")
    resolved = run(["dpkg-shlibdeps", "-O", "-e" + str(binary)], cwd=work)
    actual = next((line.split("=", 1)[1] for line in resolved.splitlines() if line.startswith("shlibs:Depends=")), "")
    recorded = run(["dpkg-deb", "--field", package, "Depends"])
    if not actual or dependencies(actual) != dependencies(recorded):
        if repair and omitted_libgcc(actual, recorded):
            # Both tar archives have passed the strict path/type/asset checks.
            # Repair only the private staged package, then validate it anew.
            with tempfile.TemporaryDirectory(prefix="libgcc-", dir=work) as temporary:
                staging = Path(temporary)
                tree = staging / "package"
                run(["dpkg-deb", "--raw-extract", package, tree])
                control = tree / "DEBIAN/control"
                control.write_text(replace_depends(control.read_text(), actual))
                rebuilt = staging / "rebuilt.deb"
                run(["dpkg-deb", "--build", "--root-owner-group", tree, rebuilt])
                os.replace(rebuilt, package)
                checked = staging / "checked"
                checked.mkdir()
                return validate(package, root, checked, arch, version, identity, app_version, repair=False)
        raise ValueError(f"Runtime dependencies unresolved or mismatched: expected {actual!r}, packaged {recorded!r}")
    if not any(re.match(r"libasound2(?:t64)?(?:\s|$)", item) for item in dependencies(recorded)):
        raise ValueError("Default audio build must declare its ALSA runtime dependency")
    run(["lintian", "--fail-on", "error", package], cwd=work)


SMOKE = r'''
set -eu
export DEBIAN_FRONTEND=noninteractive
apt-get update
apt-get install -y --no-install-recommends dpkg-dev "/packages/$1"
test "$(dn --version)" = "Directory Navigator $2"
test "$(dpkg-query -W -f='${Version}' directory-navigator)" = "$3"
apt-get install -y --reinstall "/packages/$1"
dpkg-deb --raw-extract "/packages/$1" /tmp/upgrade
sed -i "s/^Version: .*/Version: $3+dnsmoke1/" /tmp/upgrade/DEBIAN/control
dpkg-deb --build --root-owner-group /tmp/upgrade /tmp/upgrade.deb
apt-get install -y /tmp/upgrade.deb
test "$(dpkg-query -W -f='${Version}' directory-navigator)" = "$3+dnsmoke1"
test "$(dn --version)" = "Directory Navigator $2"
apt-get remove -y directory-navigator
! command -v dn
apt-get install -y --allow-downgrades "/packages/$1"
dn --help >/dev/null
apt-get purge -y directory-navigator
! test -e /usr/bin/dn
'''


def smoke(package, arch, version, app_version, image):
    if not image or image.startswith("-") or not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._:/@-]*", image):
        raise ValueError("Pass a valid Debian/Ubuntu Docker image via --smoke-test IMAGE")
    if shutil.which("docker") is None:
        raise ValueError("Docker is required only for --smoke-test")
    with tempfile.TemporaryDirectory(prefix="dn-deb-smoke-") as temporary:
        directory = Path(temporary)
        shutil.copyfile(package, directory / package.name)
        run(["docker", "run", "--rm", "--platform", "linux/" + arch,
             "--user", "0:0", "--entrypoint", "sh",
             "--mount", f"type=bind,src={directory},dst=/packages,readonly", image,
             "-ec", SMOKE, "dn-package-smoke", package.name, app_version, version])


def publish_files(package, output):
    """Local publication only; refuse replacement, including dangling links."""
    dest = output / package.name
    checksum = output / (package.name + ".sha256")
    staged = package.with_suffix(package.suffix + ".sha256")
    staged.write_text(f"{release.digest(package)}  {package.name}\n")
    linked = []
    try:
        for source, target in [(package, dest), (staged, checksum)]:
            source.chmod(0o644)
            os.link(source, target)
            linked.append((source, target))
    except OSError:
        for source, target in linked:
            if target.exists() and os.path.samestat(source.stat(), target.stat()):
                target.unlink()
        raise
    return dest


def build(root, arch, revision, identity, output, test_image=None):
    root = root.resolve()
    identity = maintainer(identity)
    app_version = metadata(root)
    version = deb_version(app_version, revision)
    environment(arch)
    if test_image and shutil.which("docker") is None:
        raise ValueError("Install Docker before requesting a container smoke test")
    output.mkdir(parents=True, exist_ok=True)
    filename = f"{NAME}_{version}_{arch}.deb"
    for path in (output / filename, output / (filename + ".sha256")):
        if os.path.lexists(path):
            raise ValueError(f"Output already exists: {path}; use another revision/output directory")
    # A shared generated Cargo asset needs a per-checkout lock. Normal Cargo
    # builds never read this file; source/version/configuration files stay untouched.
    import fcntl
    generated = root / "target/debian/.metadata"
    generated.mkdir(parents=True, exist_ok=True)
    for path in (generated, generated / "lock", generated / "changelog.Debian.gz"):
        if path.is_symlink():
            raise ValueError(f"Generated packaging metadata must not be a symlink: {path}")
    with (generated / "lock").open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        epoch = int(os.environ.get("SOURCE_DATE_EPOCH", str(int(time.time()))))
        if not 0 <= epoch <= 253402300799:
            raise ValueError("SOURCE_DATE_EPOCH must be a nonnegative supported Unix timestamp")
        stamp = email.utils.formatdate(epoch, usegmt=False)
        changelog = f"{NAME} ({version}) unstable; urgency=medium\n\n  * Package Directory Navigator {app_version} for local binary distribution.\n\n -- {identity}  {stamp}\n"
        (generated / "changelog.Debian.gz").write_bytes(gzip.compress(changelog.encode(), mtime=0))
        with tempfile.TemporaryDirectory(prefix=".dn-deb-", dir=output) as temporary:
            work = Path(temporary)
            package = work / filename
            run(["cargo", "deb", "--locked", "--manifest-path", root / "Cargo.toml",
                 "--target", ARCHES[arch][0], "--maintainer", identity,
                 "--deb-version", version, "--output", package], cwd=root)
            validate(package, root, work, arch, version, identity, app_version)
            if test_image:
                smoke(package, arch, version, app_version, test_image)
            return publish_files(package, output)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--arch", choices=ARCHES, required=True)
    parser.add_argument("--maintainer", required=True, help="Name <email>; never inferred or invented")
    parser.add_argument("--revision", default="1")
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--output", type=Path, default=None)
    parser.add_argument("--smoke-test", metavar="IMAGE", help="Opt-in Docker install/reinstall/upgrade/remove test; never installs on host")
    args = parser.parse_args()
    try:
        artifact = build(args.root, args.arch, args.revision, args.maintainer,
                         (args.output or args.root / "target/debian").resolve(), args.smoke_test)
        print(f"Validated package: {artifact}\nChecksum: {artifact}.sha256")
    except (ValueError, OSError, KeyError, tarfile.TarError, subprocess.SubprocessError) as error:
        parser.exit(1, f"Debian packaging error: {error}\n")


if __name__ == "__main__":
    main()
