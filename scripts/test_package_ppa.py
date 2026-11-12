"""Portable source-package tests using private fixtures and mocked Linux commands."""
import json
import shutil
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest
from unittest.mock import patch

import package_ppa as ppa


class PpaTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="dn-ppa-test-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        self.identity = "Test Maintainer <test@example.org>"
        repository = Path(__file__).resolve().parent.parent
        for name in ppa.ROOT_FILES | ppa.HELPERS:
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("fixture\n")
        for name in ppa.TEMPLATES:
            path = self.root / "packaging/ppa" / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes((repository / "packaging/ppa" / name).read_bytes())
        (self.root / "Cargo.toml").write_text('[package]\nname="directory-navigator"\nversion="1.1.10"\n[[bin]]\nname="dn"\n[package.metadata.deb]\ndepends="$auto"\n')
        self.digest = "a" * 64
        (self.root / "Cargo.lock").write_text('version=4\n[[package]]\nname="directory-navigator"\nversion="1.1.10"\n[[package]]\nname="example"\nversion="1.0.0"\nsource="registry+https://github.com/rust-lang/crates.io-index"\nchecksum="' + self.digest + '"\n')
        (self.root / ".cargo/config.toml").write_text('[target.x86_64-pc-windows-msvc]\nrustflags=["-C", "target-feature=+crt-static"]\n')
        copyright = self.root / "packaging/debian/copyright"
        copyright.parent.mkdir(parents=True, exist_ok=True)
        copyright.write_text("Files: *\nLicense: MIT\n fixture license\n")
        source = self.root / "src/main.rs"
        source.parent.mkdir()
        source.write_text('fn main() {}\n')
        self.names = sorted(p.relative_to(self.root).as_posix() for p in self.root.rglob("*") if p.is_file())
        self.config = '[source.crates-io]\nreplace-with="vendored-sources"\n[source.vendored-sources]\ndirectory="vendor"\n'

    def vendor(self, tree):
        path = tree / "vendor/example-1.0.0"
        path.mkdir(parents=True)
        (path / "Cargo.toml").write_text('[package]\nname="example"\nversion="1.0.0"\nlicense="MIT"\n')
        (path / "LICENSE").write_text("Private test license fixture\n")
        files = {p.name: ppa.sha256(p) for p in path.iterdir()}
        (path / ".cargo-checksum.json").write_text(json.dumps({"package": self.digest, "files": files}))
        return path

    def git_inventory(self, *args, **kwargs):
        return b"\0".join(name.encode() for name in self.names) + b"\0"

    def command(self, args, cwd=None):
        if args[:2] == ["git", "log"]:
            return "1700000000"
        self.assertEqual(args[:4], ["cargo", "vendor", "--locked", "--versioned-dirs"])
        self.assertIn("--offline", args)
        self.vendor(cwd)
        return self.config

    def test_versions_and_identity_validation(self):
        self.assertEqual(ppa.package_version("1.1.10", "1", "noble"), "1.1.10-1~ppa1~noble1")
        self.assertEqual(ppa.package_version("1.1.10", "2", "noble"), "1.1.10-1~ppa2~noble1")
        for version, revision, series in [("1.1.10", "0", "noble"), ("bad", "1", "noble"), ("1.1.10", "1", "jammy")]:
            with self.assertRaises(ValueError):
                ppa.package_version(version, revision, series)
        with self.assertRaises(ValueError):
            ppa.prepare(self.root, self.root / "out", "Invalid", prepare_only=True)

    def test_inventory_excludes_untracked_outputs_secrets_and_unapproved_paths(self):
        self.names += [".env", ".cargo/credentials.toml", "target/private", "original-dn/reference", "dist/dn"]
        with patch.object(ppa.subprocess, "check_output", side_effect=self.git_inventory):
            names = ppa.source_files(self.root)
        self.assertIn("src/main.rs", names)
        self.assertFalse(any(name in names for name in self.names[-5:]))
        self.assertIn("scripts/test_package_ppa.py", names)
        helper = self.root / ".github/scripts/test-ppa.sh"
        helper.parent.mkdir(parents=True)
        helper.write_text("#!/usr/bin/env bash\n")
        self.names.append(".github/scripts/test-ppa.sh")
        with patch.object(ppa.subprocess, "check_output", side_effect=self.git_inventory):
            self.assertIn(".github/scripts/test-ppa.sh", ppa.source_files(self.root))
        with patch.object(ppa.subprocess, "check_output", side_effect=self.git_inventory):
            (self.root / "src/main.rs").unlink()
            with self.assertRaisesRegex(ValueError, "missing source"):
                ppa.source_files(self.root)

    def test_symlink_source_is_rejected(self):
        source = self.root / "src/main.rs"
        source.unlink()
        try:
            source.symlink_to(self.root / "LICENSE")
        except OSError as error:
            self.skipTest(f"Symlink creation unavailable: {error}")
        with patch.object(ppa.subprocess, "check_output", side_effect=self.git_inventory):
            with self.assertRaisesRegex(ValueError, "Unsafe"):
                ppa.source_files(self.root)

    def test_config_merge_preserves_targets_and_rejects_source_overrides(self):
        original = (self.root / ".cargo/config.toml").read_text()
        for newline in ("\n", "\r\n"):
            result = ppa.merge_config(original.replace("\n", newline), self.config)
            self.assertIn("+crt-static", result)
            self.assertIn('directory="vendor"', result)
        for config in (self.config.replace('"vendor"', '"/private/cache"'), self.config + '[env]\nSECRET="x"\n'):
            with self.assertRaises(ValueError):
                ppa.merge_config(original, config)
        with self.assertRaises(ValueError):
            ppa.merge_config(self.config, self.config)

    def test_vendor_inventory_retains_licenses_and_checks_every_file(self):
        crate = self.vendor(self.root)
        inventory = ppa.inventory_vendor(self.root)
        self.assertEqual(inventory[0]["license_files"], ["LICENSE"])
        self.assertIn("Private test license fixture", (self.root / "VENDORED_LICENSES.txt").read_text())
        (crate / "LICENSE").write_text("corrupted")
        with self.assertRaisesRegex(ValueError, "checksum mismatch"):
            ppa.inventory_vendor(self.root)
        (crate / "LICENSE").unlink()
        with self.assertRaisesRegex(ValueError, "Missing vendored"):
            ppa.inventory_vendor(self.root)

    def test_original_tar_roundtrip_is_deterministic_and_excludes_debian(self):
        self.vendor(self.root)
        ppa.inventory_vendor(self.root)
        ppa.write_debian(self.root, self.identity, "1.1.10-1~ppa1~noble1", 1700000000)
        archive = self.root.parent / (self.root.name + ".tar.xz")
        self.addCleanup(lambda: archive.unlink(missing_ok=True))
        ppa.tar_source(self.root, archive, 1700000000)
        ppa.verify_tar(self.root, archive)
        first = ppa.sha256(archive)
        ppa.tar_source(self.root, archive, 1700000000)
        self.assertEqual(first, ppa.sha256(archive))
        with tarfile.open(archive) as contents:
            self.assertFalse(any(len(Path(m.name).parts) > 1 and Path(m.name).parts[1] == 'debian' for m in contents))
        self.assertIn(self.identity, (self.root / "debian/control").read_text())
        self.assertIn("Architecture: amd64 arm64", (self.root / "debian/control").read_text())

    def test_prepare_only_is_atomic_frozen_and_preserves_originals(self):
        before = {name: (self.root / name).read_bytes() for name in ("Cargo.lock", "Cargo.toml", ".cargo/config.toml")}
        with patch.object(ppa.subprocess, "check_output", side_effect=self.git_inventory), patch.object(ppa.deb, "run", side_effect=self.command), patch.object(ppa.subprocess, "run") as metadata:
            result = ppa.prepare(self.root, self.root / "out", self.identity, prepare_only=True, offline=True)
        self.assertEqual(metadata.call_count, 2)
        targets = []
        for call in metadata.call_args_list:
            self.assertIn("--frozen", call.args[0])
            self.assertEqual(call.kwargs['env']['CARGO_NET_OFFLINE'], 'true')
            targets.append(call.args[0][-1])
        self.assertCountEqual(targets, [p[0] for p in ppa.deb.ARCHES.values()])
        self.assertEqual(sorted(p.name for p in result.iterdir()), ["SHA256SUMS", "directory-navigator-1.1.10", "directory-navigator_1.1.10.orig.tar.xz"])
        for name, content in before.items():
            self.assertEqual((self.root / name).read_bytes(), content)
        with self.assertRaisesRegex(ValueError, "Output already exists"):
            ppa.prepare(self.root, self.root / "out", self.identity, prepare_only=True)
        self.assertEqual(len(list((self.root / "out").iterdir())), 1)

    def test_identity_can_be_deferred_until_upload_file_generation(self):
        with self.assertRaisesRegex(ValueError, "Pass --maintainer"):
            ppa.prepare(self.root, self.root / "out")
        self.assertFalse((self.root / "out").exists())
        with patch.object(ppa.subprocess, "check_output", side_effect=self.git_inventory), patch.object(ppa.deb, "run", side_effect=self.command), patch.object(ppa.subprocess, "run"):
            result = ppa.prepare(self.root, self.root / "out", prepare_only=True, offline=True)
        tree = result / "directory-navigator-1.1.10"
        for name in ("control", "changelog"):
            text = (tree / "debian" / name).read_text()
            self.assertIn("@MAINTAINER@", text)
            self.assertNotIn(self.identity, text)

    def test_failed_vendoring_or_offline_metadata_exposes_no_output(self):
        for stage in ("vendor", "metadata"):
            output = self.root / ("failed-" + stage)
            vendor = self.command if stage == "metadata" else ValueError("vendor failure")
            with patch.object(ppa.subprocess, "check_output", side_effect=self.git_inventory), patch.object(ppa.deb, "run", side_effect=vendor), patch.object(ppa.subprocess, "run", side_effect=subprocess.CalledProcessError(101, "metadata")):
                with self.assertRaises((ValueError, subprocess.CalledProcessError)):
                    ppa.prepare(self.root, output, self.identity, prepare_only=True, offline=True)
            self.assertEqual(list(output.iterdir()), [])

    def test_source_artifact_references_reject_corruption_and_traversal(self):
        archive = self.root / "source.tar.xz"
        archive.write_bytes(b"private fixture")
        digest = ppa.sha256(archive)
        changes = self.root / "source.changes"
        changes.write_text(f"Checksums-Sha256:\n {digest} {archive.stat().st_size} {archive.name}\n")
        self.assertEqual(ppa.verify_references(changes, self.root), {archive.name})
        for line in [f" {digest} 15 ../source.tar.xz", f" {'0' * 64} 15 source.tar.xz", f" {digest} 1 source.tar.xz"]:
            changes.write_text("Checksums-Sha256:\n" + line + "\n")
            with self.assertRaises(ValueError):
                ppa.verify_references(changes, self.root)

    def test_linux_source_checks_references_and_extracted_contents_before_lintian(self):
        for corrupted in (False, True):
            with self.subTest(corrupted=corrupted), tempfile.TemporaryDirectory(dir=self.root) as directory:
                work = Path(directory)
                tree = work / "directory-navigator-1.1.10"
                tree.mkdir()
                (tree / "Cargo.toml").write_text("private source fixture")
                upstream = "directory-navigator_1.1.10.orig.tar.xz"
                version = "1.1.10-1~ppa1~noble1"
                debian = f"directory-navigator_{version}.debian.tar.xz"
                dsc_name = f"directory-navigator_{version}.dsc"
                changes_name = f"directory-navigator_{version}_source.changes"
                calls = []

                def references(names):
                    return "Checksums-Sha256:\n" + "".join(
                        f" {ppa.sha256(work / name)} {(work / name).stat().st_size} {name}\n" for name in names)

                def command(args, cwd=None):
                    calls.append(args)
                    if args[0] == "dpkg-buildpackage":
                        self.assertIn("-sa", args)
                        self.assertIn("-us", args)
                        self.assertIn("-uc", args)
                        for name in (upstream, debian):
                            (work / name).write_bytes(b"command fixture, not a real source archive")
                        (work / dsc_name).write_text(references((upstream, debian)))
                        (work / changes_name).write_text("Distribution: noble\nArchitecture: source\n" + references((upstream, debian, dsc_name)))
                    elif args[0] == "dpkg-source":
                        shutil.copytree(tree, Path(args[-1]))
                        if corrupted:
                            (Path(args[-1]) / "Cargo.toml").write_text("changed")
                    return ""

                with patch.object(ppa.deb, "run", side_effect=command):
                    if corrupted:
                        with self.assertRaisesRegex(ValueError, "Extracted source differs"):
                            ppa.linux_source(tree, work, version, "1.1.10")
                    else:
                        result = ppa.linux_source(tree, work, version, "1.1.10")
                        self.assertEqual({path.name for path in result}, {upstream, debian, dsc_name, changes_name})
                self.assertEqual(sum(args[0] == "lintian" for args in calls), int(not corrupted))

    def test_linux_generation_is_refused_on_mac(self):
        with patch.object(ppa.platform, "system", return_value="Darwin"):
            with self.assertRaisesRegex(ValueError, "prepare-only"):
                ppa.prepare(self.root, self.root / "out", self.identity)


if __name__ == "__main__":
    unittest.main()
