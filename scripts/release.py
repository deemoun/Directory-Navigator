#!/usr/bin/env python3
"""Version and package release sources; all commands are local except check-unused."""
import argparse
import hashlib
import os
from pathlib import Path
import re
import subprocess
import tarfile
import tomllib
import urllib.error
import urllib.parse
import urllib.request
import zipfile

TARGETS = (
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "aarch64-unknown-linux-gnu",
    "x86_64-unknown-linux-gnu",
    "x86_64-pc-windows-msvc",
)
PLATFORM_TARGETS = {"all": TARGETS, "macos": TARGETS[:2], "linux": TARGETS[2:4], "windows": TARGETS[4:]}
SEMVER = re.compile(
    r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)"
    r"(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?"
    r"(?:\+([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?"
)


def validate_version(value):
    match = SEMVER.fullmatch(value)
    if len(value) > 128 or not match:
        raise ValueError("Enter a SemVer version without v, for example 1.0.0 or 1.1.0-beta.1")
    if any(int(n) > 2**64 - 1 for n in match.groups()[:3]):
        raise ValueError("Version numbers exceed Cargo's supported range")
    prerelease = match.group(4)
    if prerelease and any(s.isdigit() and len(s) > 1 and s[0] == "0" for s in prerelease.split(".")):
        raise ValueError("Numeric prerelease identifiers must not have leading zeros")
    return bool(prerelease)


def replace_version(section, version):
    changed, count = re.subn(
        r'(?m)^version[ \t]*=[ \t]*"[^"\n]*"[ \t]*(?:#[^\n]*)?$',
        f'version = "{version}"', section,
    )
    if count != 1:
        raise ValueError("Expected exactly one package version")
    return changed


def prepare(root, version):
    validate_version(version)
    manifest_path, lock_path = root / "Cargo.toml", root / "Cargo.lock"
    manifest, lock = manifest_path.read_text(), lock_path.read_text()
    package = tomllib.loads(manifest).get("package", {})
    packages = [p for p in tomllib.loads(lock).get("package", []) if p.get("name") == "dn" and "source" not in p]
    if package.get("name") != "dn" or len(packages) != 1 or packages[0].get("version") != package.get("version"):
        raise ValueError("Expected matching dn package versions in Cargo.toml and Cargo.lock")
    section = re.search(r"(?ms)^\[package\]\n.*?(?=^\[|\Z)", manifest)
    if section is None:
        raise ValueError("Missing [package] section")
    manifest = manifest[:section.start()] + replace_version(section.group(), version) + manifest[section.end():]
    blocks = re.split(r"(?m)(?=^\[\[package\]\]$)", lock)
    matches = 0
    for i, block in enumerate(blocks):
        if block.startswith("[[package]]"):
            item = tomllib.loads(block)["package"][0]
            if item.get("name") == "dn" and "source" not in item:
                blocks[i] = replace_version(block, version)
                matches += 1
    if matches != 1:
        raise ValueError("Could not locate the dn lockfile entry")
    manifest_path.write_text(manifest)
    lock_path.write_text("".join(blocks))


def archive_name(version, target):
    validate_version(version)
    if target not in TARGETS:
        raise ValueError("Unsupported release target")
    extension = "zip" if target.endswith("windows-msvc") else "tar.gz"
    return f"dn-{version}-{target}.{extension}"


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def package(root, version, target, output):
    name = archive_name(version, target)
    binary_name = "dn.exe" if target.endswith("windows-msvc") else "dn"
    binary = root / "target" / target / "release" / binary_name
    files = [(binary, binary_name), (root / "LICENSE", "LICENSE"), (root / "README.md", "README.md"),
             (root / "docs/USER_GUIDE.md", "docs/USER_GUIDE.md")]
    if any(p.is_symlink() or not p.is_file() for p, _ in files):
        raise ValueError("Missing or non-regular release input")
    if not os.access(binary, os.X_OK):
        raise ValueError("Release executable is not executable")
    result = subprocess.run([str(binary.resolve()), "--version"], capture_output=True, text=True, check=True, timeout=15)
    if result.stdout != f"Directory Navigator {version}\n" or result.stderr:
        raise ValueError("Built binary version does not match the requested release")
    output.mkdir(parents=True, exist_ok=True)
    archive = output / name
    checksum = output / (name + ".sha256")
    if archive.exists() or archive.is_symlink() or checksum.exists() or checksum.is_symlink():
        raise ValueError("Release output already exists")
    if binary_name == "dn.exe":
        with zipfile.ZipFile(archive, "x", compression=zipfile.ZIP_DEFLATED) as bundle:
            for path, member in files:
                bundle.write(path, arcname=member)
    else:
        with tarfile.open(archive, "x:gz") as bundle:
            for path, member in files:
                info = bundle.gettarinfo(str(path), arcname=member)
                info.uid = info.gid = 0
                info.uname = info.gname = ""
                with path.open("rb") as stream:
                    bundle.addfile(info, stream)
    with checksum.open("x") as stream:
        stream.write(f"{digest(archive)}  {name}\n")


def verify_assets(output, version, platform="all"):
    targets = PLATFORM_TARGETS[platform]
    expected = {archive_name(version, target) for target in targets}
    expected |= {name + ".sha256" for name in expected}
    if {p.name for p in output.iterdir()} != expected:
        raise ValueError(f"Expected exactly {len(targets)} {platform} platform archives and their checksums")
    lines = []
    for target in targets:
        name = archive_name(version, target)
        archive, checksum = output / name, output / (name + ".sha256")
        if any(p.is_symlink() or not p.is_file() for p in (archive, checksum)):
            raise ValueError("Release assets must be regular files")
        line = f"{digest(archive)}  {name}\n"
        if checksum.read_text() != line:
            raise ValueError(f"Checksum mismatch: {name}")
        lines.append(line)
    with (output / "SHA256SUMS.txt").open("x") as stream:
        stream.writelines(lines)


def check_unused(version):
    validate_version(version)
    repo = os.environ["GITHUB_REPOSITORY"]
    api = os.environ.get("GITHUB_API_URL", "https://api.github.com").rstrip("/")
    token = os.environ["GH_TOKEN"]
    if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repo) or not api.startswith("https://"):
        raise ValueError("Invalid GitHub repository/API endpoint")
    tag = urllib.parse.quote("v" + version, safe="")
    for path in (f"git/ref/tags/{tag}", f"releases/tags/{tag}"):
        request = urllib.request.Request(f"{api}/repos/{repo}/{path}", headers={
            "Authorization": f"Bearer {token}", "Accept": "application/vnd.github+json",
        })
        try:
            with urllib.request.urlopen(request, timeout=30):
                pass
        except urllib.error.HTTPError as error:
            status = error.code
            error.close()
            if status == 404:
                continue
            raise ValueError(f"GitHub preflight failed (HTTP {status}); refusing to proceed") from None
        raise ValueError(f"Tag or release v{version} already exists; choose an unused version")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=["prepare", "package", "verify-assets", "check-unused"])
    parser.add_argument("version")
    parser.add_argument("--root", type=Path, default=Path.cwd())
    parser.add_argument("--target", choices=TARGETS)
    parser.add_argument("--platform", choices=PLATFORM_TARGETS, default="all")
    parser.add_argument("--output", type=Path, default=Path("dist"))
    parser.add_argument("--github-output", type=Path)
    args = parser.parse_args()
    try:
        prerelease = validate_version(args.version)
        if args.command == "prepare":
            prepare(args.root, args.version)
            if args.github_output:
                with args.github_output.open("a") as stream:
                    stream.write(f"version={args.version}\ntag=v{args.version}\nprerelease={str(prerelease).lower()}\n")
        elif args.command == "package":
            if not args.target:
                parser.error("package requires --target")
            package(args.root, args.version, args.target, args.output)
        elif args.command == "verify-assets":
            verify_assets(args.output, args.version, args.platform)
        else:
            check_unused(args.version)
    except (ValueError, OSError, subprocess.SubprocessError) as error:
        parser.exit(1, f"Release error: {error}\n")


if __name__ == "__main__":
    main()
