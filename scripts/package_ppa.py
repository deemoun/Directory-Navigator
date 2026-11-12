#!/usr/bin/env python3
"""Prepare a vendored Noble source package for manual signing and PPA upload."""
import argparse
import email.utils
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import platform
import re
import shutil
import subprocess
import tarfile
import tempfile
import tomllib

import package_deb as deb

NAME = deb.NAME
TEMPLATES = ("control", "rules", "install", "docs", "manpages", "source/format")
ROOT_FILES = {"Cargo.toml", "Cargo.lock", "LICENSE", "README.md", "README.ru.md",
              "THIRD_PARTY_NOTICES.md", "AGENTS.md", "PROJECT_STATE.md", ".cargo/config.toml"}
SOURCE_DIRS = ("src/", "tests/", "locales/", "docs/", "scripts/", "packaging/debian/",
               "packaging/ppa/", ".github/workflows/", ".github/scripts/")
HELPERS = {"scripts/package_ppa.py", "scripts/test_package_ppa.py"}


def package_version(version, revision, series):
    if series != "noble":
        raise ValueError("This packaging targets Ubuntu 24.04 (noble)")
    deb.deb_version(version, revision)
    return deb.deb_version(version, "1") + f"~ppa{revision}~{series}1"


def source_files(root):
    tracked = subprocess.check_output(["git", "ls-files", "--cached", "-z"], cwd=root)
    names = {os.fsdecode(name) for name in tracked.split(b"\0") if name}
    names |= HELPERS | {"packaging/ppa/" + name for name in TEMPLATES}
    selected = sorted(name for name in names if name in ROOT_FILES or name.startswith(SOURCE_DIRS))
    if not ROOT_FILES <= set(selected):
        raise ValueError("Required source files are missing from the source inventory")
    for name in selected:
        path = root / name
        if (PurePosixPath(name).is_absolute() or ".." in PurePosixPath(name).parts
                or path.is_symlink() or not path.is_file()
                or not path.resolve().is_relative_to(root)):
            raise ValueError(f"Unsafe or missing source file: {name}")
    return selected


def merge_config(original, generated):
    current = tomllib.loads(original)
    replacement = tomllib.loads(generated)
    if "source" in current:
        raise ValueError("Source checkout already has Cargo source overrides; review them before vendoring")
    if set(replacement) != {"source"}:
        raise ValueError("Unexpected cargo vendor configuration")
    sources = replacement["source"]
    if (sources.get("crates-io") != {"replace-with": "vendored-sources"}
            or sources.get("vendored-sources") != {"directory": "vendor"}
            or len(sources) != 2):
        raise ValueError("Expected a relative vendor directory and crates.io source replacement")
    combined = original.rstrip() + "\n\n" + generated.strip() + "\n"
    if tomllib.loads(combined) != dict(current, **replacement):
        raise ValueError("Cargo configuration merge changed existing settings")
    return combined


def sha256(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def inventory_vendor(tree):
    lock = tomllib.loads((tree / "Cargo.lock").read_text())
    expected = {(p["name"], p["version"]): p for p in lock["package"] if "source" in p}
    if any(p["source"] != "registry+https://github.com/rust-lang/crates.io-index" for p in expected.values()):
        raise ValueError("Only locked crates.io sources are supported")
    inventory, notices, seen = [], [], set()
    for directory in sorted((tree / "vendor").iterdir()):
        if directory.is_symlink() or not directory.is_dir():
            raise ValueError(f"Unexpected vendor entry: {directory.name}")
        manifest = tomllib.loads((directory / "Cargo.toml").read_text())["package"]
        key = manifest["name"], manifest["version"]
        if key not in expected or key in seen:
            raise ValueError(f"Unexpected or duplicate vendored crate: {key}")
        seen.add(key)
        checks = json.loads((directory / ".cargo-checksum.json").read_text())
        if checks["package"] != expected[key].get("checksum"):
            raise ValueError(f"Vendored package checksum differs from lockfile: {key}")
        license_paths = []
        for path in sorted(directory.rglob("*")):
            if path.is_symlink() or not path.resolve().is_relative_to(directory.resolve()):
                raise ValueError(f"Unsafe vendored path: {path}")
            if not path.is_file():
                continue
            relative = path.relative_to(directory).as_posix()
            if relative != ".cargo-checksum.json" and checks["files"].get(relative) != sha256(path):
                raise ValueError(f"Vendored file checksum mismatch: {path}")
            if (re.match(r"(?i)^(licen[sc]e|copying|copyright|notice|unlicense)([.\-_]|$)", path.name)
                    or relative == manifest.get("license-file")):
                license_paths.append(relative)
                notices.append(f"\n--- {directory.name}/{relative} ---\n" + path.read_text(errors="replace"))
        for relative in checks["files"]:
            path = directory / relative
            if (PurePosixPath(relative).is_absolute() or ".." in PurePosixPath(relative).parts
                    or path.is_symlink() or not path.resolve().is_relative_to(directory.resolve())):
                raise ValueError(f"Unsafe vendored checksum path: {relative}")
            if not path.is_file():
                raise ValueError(f"Missing vendored file: {path}")
        expression = manifest.get("license")
        if not expression and not license_paths:
            raise ValueError(f"No license information for vendored crate: {key}")
        inventory.append({"name": key[0], "version": key[1], "license": expression,
                          "source": expected[key]["source"], "checksum": checks["package"],
                          "directory": "vendor/" + directory.name, "license_files": license_paths})
    if seen != set(expected):
        raise ValueError("Vendor directory does not contain every locked dependency")
    (tree / "VENDORED_LICENSES.json").write_text(json.dumps(inventory, ensure_ascii=False, indent=2) + "\n")
    header = "Vendored dependency licenses\n\nSee VENDORED_LICENSES.json for all locked crates and their license expressions.\nLicense files below are preserved from the dependency sources.\n"
    summaries = [f"{p['name']} {p['version']}: {p['license'] or 'see license file'}" for p in inventory]
    (tree / "VENDORED_LICENSES.txt").write_text(header + "\n".join(summaries) + "\n" + "\n".join(notices))
    return inventory


def write_debian(tree, identity, version, epoch):
    destination = tree / "debian"
    destination.mkdir()
    for name in TEMPLATES:
        path = destination / name
        path.parent.mkdir(parents=True, exist_ok=True)
        text = (tree / "packaging/ppa" / name).read_text().replace("@MAINTAINER@", identity)
        path.write_text(text)
    (destination / "rules").chmod(0o755)
    copyright_text = (tree / "packaging/debian/copyright").read_text()
    copyright_text += "\nFiles: vendor/*\nCopyright: See the individual dependency sources\nLicense: vendored-dependencies\n Each crate retains its original license files in vendor/.\n VENDORED_LICENSES.json records license expressions and file locations.\n VENDORED_LICENSES.txt includes the collected dependency license notices.\n"
    (destination / "copyright").write_text(copyright_text)
    stamp = email.utils.formatdate(epoch, usegmt=False)
    (destination / "changelog").write_text(
        f"{NAME} ({version}) noble; urgency=medium\n\n"
        "  * Package Directory Navigator for Ubuntu 24.04 on amd64 and arm64.\n"
        "  * Bundle locked Rust sources for offline builds with audio support.\n\n"
        f" -- {identity}  {stamp}\n")


def tar_source(tree, output, epoch):
    with tarfile.open(output, "w:xz", format=tarfile.PAX_FORMAT) as archive:
        for path in [tree] + sorted(tree.rglob("*")):
            relative = path.relative_to(tree)
            if relative.parts and relative.parts[0] == "debian":
                continue
            if path.is_symlink():
                raise ValueError(f"Source links are unsupported: {path}")
            info = archive.gettarinfo(path, arcname=tree.name + ("/" + relative.as_posix() if relative.parts else ""))
            info.uid = info.gid = 0
            info.uname = info.gname = "root"
            info.mtime = epoch
            info.mode = 0o755 if path.is_dir() or os.access(path, os.X_OK) else 0o644
            if path.is_file():
                with path.open("rb") as stream:
                    archive.addfile(info, stream)
            else:
                archive.addfile(info)


def verify_tar(tree, archive_path):
    # Read back every regular member and compare bytes without extracting paths.
    expected = {p.relative_to(tree).as_posix(): p for p in tree.rglob("*")
                if p.is_file() and p.relative_to(tree).parts[0] != "debian"}
    seen = set()
    with tarfile.open(archive_path, "r:xz") as archive:
        for member in archive:
            name = PurePosixPath(member.name)
            if name.is_absolute() or ".." in name.parts or name.parts[0] != tree.name:
                raise ValueError("Unsafe source archive path")
            if member.isdir():
                continue
            relative = PurePosixPath(*name.parts[1:]).as_posix()
            if not member.isfile() or relative in seen or relative not in expected:
                raise ValueError("Unexpected source archive member")
            with archive.extractfile(member) as stream:
                if hashlib.file_digest(stream, "sha256").hexdigest() != sha256(expected[relative]):
                    raise ValueError("Source archive content mismatch")
            seen.add(relative)
    if seen != set(expected):
        raise ValueError("Source archive is incomplete")


def verify_references(path, directory):
    text = path.read_text()
    if text.startswith("-----BEGIN PGP"):
        raise ValueError("Preparation expects unsigned source metadata")
    match = re.search(r"(?m)^Checksums-Sha256:\n((?:[ \t].*\n?)+)", text)
    if not match:
        raise ValueError(f"Missing SHA-256 references: {path.name}")
    names = set()
    for line in match[1].splitlines():
        digest, size, name = line.split()
        if name in names or Path(name).name != name or name in (".", ".."):
            raise ValueError("Unsafe or duplicate source artifact reference")
        names.add(name)
        artifact = directory / name
        if artifact.is_symlink() or not artifact.is_file() or artifact.stat().st_size != int(size) or sha256(artifact) != digest:
            raise ValueError(f"Source artifact checksum mismatch: {name}")
    return names


def linux_source(tree, work, version, upstream):
    originals = {path.relative_to(tree): sha256(path) for path in tree.rglob("*") if path.is_file()}
    deb.run(["dpkg-buildpackage", "--build=source", "--no-check-builddeps", "-sa", "-us", "-uc"], cwd=tree)
    changes = work / f"{NAME}_{version}_source.changes"
    dsc = work / f"{NAME}_{version}.dsc"
    expected = {f"{NAME}_{upstream}.orig.tar.xz", f"{NAME}_{version}.debian.tar.xz"}
    if verify_references(dsc, work) != expected:
        raise ValueError("Unexpected .dsc source files")
    references = verify_references(changes, work)
    required = expected | {dsc.name}
    if not required <= references or references - required - {f"{NAME}_{version}_source.buildinfo"}:
        raise ValueError("Source changes must include .dsc and original/debian tarballs")
    if not re.search(r"(?m)^Distribution: noble$", changes.read_text()) or not re.search(r"(?m)^Architecture: source$", changes.read_text()):
        raise ValueError("Expected a Noble source-only upload")
    extracted = work / "source-check"
    deb.run(["dpkg-source", "-x", dsc, extracted], cwd=work)
    # Source extraction must preserve the complete upstream and Debian contents.
    for relative, digest in originals.items():
        other = extracted / relative
        if other.is_symlink() or not other.is_file() or digest != sha256(other):
            raise ValueError(f"Extracted source differs: {relative}")
    deb.run(["lintian", "--fail-on", "error", changes], cwd=work)
    return [work / name for name in sorted(references | {changes.name})]


def prepare(root, output, identity=None, revision="1", series="noble", prepare_only=False, offline=False):
    root, output = root.resolve(), output.resolve()
    if identity is None:
        if not prepare_only:
            raise ValueError("Pass --maintainer 'Name <email>' when generating the upload bundle")
        identity = "@MAINTAINER@"
    else:
        identity = deb.maintainer(identity)
    app_version = deb.metadata(root)
    version = package_version(app_version, revision, series)
    upstream = deb.deb_version(app_version, "1").rsplit("-", 1)[0]
    if not prepare_only:
        if platform.system() != "Linux" or platform.freedesktop_os_release().get("VERSION_CODENAME") != "noble":
            raise ValueError("Source generation requires Ubuntu 24.04; use --prepare-only on macOS")
        for command in ("dpkg-buildpackage", "dpkg-source", "lintian"):
            if shutil.which(command) is None:
                raise ValueError(f"Missing source packaging tool: {command}")
    output.mkdir(parents=True, exist_ok=True)
    final = output / version
    if os.path.lexists(final):
        raise ValueError(f"Output already exists: {final}; choose another output or revision")
    epoch = int(os.environ.get("SOURCE_DATE_EPOCH", deb.run(["git", "log", "-1", "--format=%ct"], cwd=root)))
    if not 0 <= epoch <= 253402300799:
        raise ValueError("SOURCE_DATE_EPOCH is outside the supported range")
    with tempfile.TemporaryDirectory(prefix=".dn-ppa-", dir=output) as temporary:
        work = Path(temporary)
        tree = work / f"{NAME}-{upstream}"
        tree.mkdir()
        for name in source_files(root):
            dest = tree / name
            dest.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(root / name, dest)
            dest.chmod(0o755 if os.access(root / name, os.X_OK) else 0o644)
        before = (tree / "Cargo.lock").read_bytes()
        args = ["cargo", "vendor", "--locked", "--versioned-dirs"]
        if offline:
            args.append("--offline")
        config = deb.run(args + ["vendor"], cwd=tree)
        if (tree / "Cargo.lock").read_bytes() != before:
            raise ValueError("Vendoring changed Cargo.lock")
        cargo_config = tree / ".cargo/config.toml"
        cargo_config.write_text(merge_config(cargo_config.read_text(), config))
        inventory_vendor(tree)
        # Confirm that a fresh Cargo home can resolve both targets from this bundle.
        cargo_home = work / "cargo-home"
        cargo_home.mkdir()
        env = dict(os.environ, CARGO_HOME=str(cargo_home), CARGO_NET_OFFLINE="true")
        for target in deb.ARCHES.values():
            subprocess.run(["cargo", "metadata", "--frozen", "--format-version", "1", "--filter-platform", target[0]],
                           cwd=tree, env=env, check=True, stdout=subprocess.DEVNULL)
        orig = work / f"{NAME}_{upstream}.orig.tar.xz"
        tar_source(tree, orig, epoch)
        verify_tar(tree, orig)
        write_debian(tree, identity, version, epoch)
        artifacts = [orig] if prepare_only else linux_source(tree, work, version, upstream)
        result = work / "result"
        result.mkdir()
        shutil.move(tree, result / tree.name)
        for path in artifacts:
            shutil.copyfile(path, result / path.name)
        sums = "".join(f"{sha256(result / p.name)}  {p.name}\n" for p in artifacts)
        (result / "SHA256SUMS").write_text(sums)
        # mkdir reserves the output revision without replacing another process's work.
        final.mkdir()
        try:
            for path in result.iterdir():
                shutil.move(path, final / path.name)
        except BaseException:
            shutil.rmtree(final)
            raise
    return final


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--maintainer", help="Public Name <email>; required for upload-file generation, optional with --prepare-only")
    parser.add_argument("--revision", default="1")
    parser.add_argument("--series", default="noble", choices=["noble"])
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--output", type=Path, default=None)
    parser.add_argument("--prepare-only", action="store_true", help="Stage sources and original tarball without Debian tools")
    parser.add_argument("--offline", action="store_true", help="Vendor only dependency sources already cached locally")
    args = parser.parse_args()
    try:
        result = prepare(args.root, args.output or args.root / "target/ppa", args.maintainer,
                         args.revision, args.series, args.prepare_only, args.offline)
        print(f"Prepared source bundle: {result}")
        if args.prepare_only:
            print("Unsigned .dsc and _source.changes generation still requires Ubuntu 24.04. See docs/DEBIAN_PACKAGING.md.")
    except (ValueError, OSError, KeyError, subprocess.SubprocessError, tarfile.TarError) as error:
        parser.exit(1, f"PPA packaging error: {error}\n")


if __name__ == "__main__":
    main()
