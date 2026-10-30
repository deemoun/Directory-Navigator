import contextlib
import io
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest
from unittest.mock import patch
import urllib.error
import zipfile
import release


class ReleaseTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="dn-release-test-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        (self.root / "Cargo.toml").write_text('[package]\nname = "dn"\nversion = "1.0.0-beta"\n\n[dependencies]\nother = "1"\n')
        (self.root / "Cargo.lock").write_text('version = 4\n\n[[package]]\nname = "dn"\nversion = "1.0.0-beta"\n\n[[package]]\nname = "other"\nversion = "1.0.0-beta"\nsource = "registry+example"\n')

    def test_valid_versions_and_prerelease_classification(self):
        for value, prerelease in [("0.1.0", False), ("1.0.0", False), ("1.2.3+build.01", False),
                                  ("1.0.0-beta", True), ("2.0.0-rc.1+build", True), ("1.0.0-0", True)]:
            self.assertEqual(release.validate_version(value), prerelease)

    def test_invalid_versions_never_modify_sources(self):
        original = (self.root / "Cargo.toml").read_bytes(), (self.root / "Cargo.lock").read_bytes()
        for value in ["", "v1.0.0", "1.0", "01.0.0", "1.0.0-01", "1.0.0-", "1.0.0+", "1.0.0\n", "1.0.0;echo bad", "$(whoami)", "1.0.0/evil", "1.0.0-" + "a" * 129, "18446744073709551616.0.0"]:
            with self.assertRaises(ValueError, msg=value):
                release.prepare(self.root, value)
            self.assertEqual(original, ((self.root / "Cargo.toml").read_bytes(), (self.root / "Cargo.lock").read_bytes()))

    def test_version_change_updates_only_root_metadata_and_is_repeatable(self):
        release.prepare(self.root, "1.2.3-beta.4")
        manifest = (self.root / "Cargo.toml").read_text()
        lock = (self.root / "Cargo.lock").read_text()
        self.assertIn('version = "1.2.3-beta.4"', manifest)
        self.assertIn('name = "dn"\nversion = "1.2.3-beta.4"', lock)
        self.assertIn('name = "other"\nversion = "1.0.0-beta"', lock)
        release.prepare(self.root, "1.2.3-beta.4")
        self.assertEqual(manifest, (self.root / "Cargo.toml").read_text())
        self.assertEqual(lock, (self.root / "Cargo.lock").read_text())
        self.assertEqual({p.name for p in self.root.iterdir()}, {"Cargo.toml", "Cargo.lock"})

    def test_malformed_or_mismatched_metadata_is_not_partially_changed(self):
        for broken in ['not TOML at all', 'version = 4\n', '[[package]]\nname = "dn"\nversion = "0.9.0"\n']:
            (self.root / "Cargo.lock").write_text(broken)
            before = (self.root / "Cargo.toml").read_bytes()
            with self.assertRaises(ValueError):
                release.prepare(self.root, "1.2.3")
            self.assertEqual(before, (self.root / "Cargo.toml").read_bytes())
            self.assertEqual(broken, (self.root / "Cargo.lock").read_text())

    def test_workflow_outputs_keep_version_tag_and_prerelease_in_sync(self):
        for version, prerelease in [("1.2.3-beta.1", "true"), ("1.2.3+build.1", "false")]:
            output = self.root / ("output-" + version)
            with patch("sys.argv", ["release.py", "prepare", version, "--root", str(self.root), "--github-output", str(output)]):
                release.main()
            self.assertEqual(output.read_text(), f"version={version}\ntag=v{version}\nprerelease={prerelease}\n")

    def package_fixture(self, version="1.2.3"):
        for target in release.TARGETS:
            binary = self.root / "target" / target / "release" / ("dn.exe" if target.endswith("windows-msvc") else "dn")
            binary.parent.mkdir(parents=True)
            binary.write_bytes(b"fake binary")
            binary.chmod(0o755)
        for name in ["LICENSE", "README.md", "THIRD_PARTY_NOTICES.md", "docs/USER_GUIDE.md"]:
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("fixture: " + name)
        result = subprocess.CompletedProcess([], 0, f"Directory Navigator {version}\n", "")
        return patch("release.subprocess.run", return_value=result)

    def test_packages_and_checksums_preserve_executable_and_include_only_distribution_files(self):
        output = self.root / "dist"
        with self.package_fixture():
            for target in release.TARGETS:
                release.package(self.root, "1.2.3", target, output)
                archive_path = output / release.archive_name("1.2.3", target)
                if target.endswith("windows-msvc"):
                    with zipfile.ZipFile(archive_path) as archive:
                        self.assertEqual(archive.namelist(), ["dn.exe", "LICENSE", "README.md", "THIRD_PARTY_NOTICES.md", "docs/USER_GUIDE.md"])
                        self.assertEqual(archive.read("dn.exe"), b"fake binary")
                        self.assertTrue(all(not member.is_dir() for member in archive.infolist()))
                else:
                    with tarfile.open(archive_path) as archive:
                        self.assertEqual(archive.getnames(), ["dn", "LICENSE", "README.md", "THIRD_PARTY_NOTICES.md", "docs/USER_GUIDE.md"])
                        self.assertTrue(archive.getmember("dn").mode & 0o111)
                        self.assertTrue(all(member.isfile() for member in archive.getmembers()))
        release.verify_assets(output, "1.2.3")
        self.assertEqual(len((output / "SHA256SUMS.txt").read_text().splitlines()), 5)

    def test_platform_bundles_are_independent_and_require_exact_assets(self):
        with self.package_fixture():
            for platform in ("macos", "linux"):
                with self.subTest(platform=platform):
                    output = self.root / platform
                    targets = release.PLATFORM_TARGETS[platform]
                    release.package(self.root, "1.2.3", targets[0], output)
                    with self.assertRaises(ValueError):
                        release.verify_assets(output, "1.2.3", platform)
                    self.assertFalse((output / "SHA256SUMS.txt").exists())
                    release.package(self.root, "1.2.3", targets[1], output)
                    extra = output / "unexpected.txt"
                    extra.write_text("unexpected")
                    with self.assertRaises(ValueError):
                        release.verify_assets(output, "1.2.3", platform)
                    extra.unlink()
                    other = "linux" if platform == "macos" else "macos"
                    with self.assertRaises(ValueError):
                        release.verify_assets(output, "1.2.3", other)
                    with patch("sys.argv", ["release.py", "verify-assets", "1.2.3", "--platform", platform, "--output", str(output)]):
                        release.main()
                    manifest = (output / "SHA256SUMS.txt").read_text().splitlines()
                    self.assertEqual(len(manifest), 2)
                    self.assertEqual([line.split("  ")[1] for line in manifest],
                                     [release.archive_name("1.2.3", target) for target in targets])

    def test_incomplete_tampered_and_existing_outputs_are_refused(self):
        output = self.root / "dist"
        with self.package_fixture():
            for target in release.TARGETS:
                release.package(self.root, "1.2.3", target, output)
            target = release.TARGETS[0]
            with self.assertRaises(ValueError):
                release.package(self.root, "1.2.3", target, output)
        archive = output / release.archive_name("1.2.3", target)
        archive.write_bytes(b"changed")
        with self.assertRaises(ValueError):
            release.verify_assets(output, "1.2.3")
        self.assertFalse((output / "SHA256SUMS.txt").exists())
        archive.unlink()
        with self.assertRaises(ValueError):
            release.verify_assets(output, "1.2.3")

    def test_wrong_binary_version_and_unknown_targets_produce_no_package(self):
        output = self.root / "dist"
        with self.package_fixture(version="0.9.0"):
            with self.assertRaises(ValueError):
                release.package(self.root, "1.2.3", release.TARGETS[0], output)
        self.assertFalse(output.exists())
        with self.assertRaises(ValueError):
            release.archive_name("1.2.3", "unsupported-target")

    def test_windows_bundle_requires_its_zip_and_checksum(self):
        output = self.root / "dist"
        target = "x86_64-pc-windows-msvc"
        with self.package_fixture():
            release.package(self.root, "1.2.3", target, output)
        name = "dn-1.2.3-x86_64-pc-windows-msvc.zip"
        self.assertEqual(release.archive_name("1.2.3", target), name)
        with self.assertRaises(ValueError):
            release.verify_assets(output, "1.2.3", "all")
        release.verify_assets(output, "1.2.3", "windows")
        self.assertEqual((output / "SHA256SUMS.txt").read_text(),
                         f"{release.digest(output / name)}  {name}\n")

    def test_linux_download_names_and_checksums_use_clean_architecture_labels(self):
        output = self.root / "dist"
        names = ["dn-1.2.3-linux-aarch64.tar.gz", "dn-1.2.3-linux-x86_64.tar.gz"]
        with self.package_fixture():
            for target, name in zip(release.PLATFORM_TARGETS["linux"], names):
                self.assertEqual(release.archive_name("1.2.3", target), name)
                release.package(self.root, "1.2.3", target, output)
                self.assertEqual((output / (name + ".sha256")).read_text(),
                                 f"{release.digest(output / name)}  {name}\n")
        self.assertEqual({path.name for path in output.iterdir()},
                         set(names + [name + ".sha256" for name in names]))
        release.verify_assets(output, "1.2.3", "linux")
        self.assertEqual([line.split("  ")[1] for line in
                          (output / "SHA256SUMS.txt").read_text().splitlines()], names)

    def test_remote_collision_auth_errors_and_unknown_network_failures_fail_closed(self):
        env = {"GH_TOKEN": "fake-test-token", "GITHUB_REPOSITORY": "example/dn", "GITHUB_API_URL": "https://api.github.com"}
        def missing(*_args, **_kwargs):
            raise urllib.error.HTTPError("https://example", 404, "missing", {}, None)
        with patch.dict(os.environ, env), patch("release.urllib.request.urlopen", side_effect=missing) as call:
            release.check_unused("1.2.3")
            self.assertEqual(call.call_count, 2)
        with patch.dict(os.environ, env), patch("release.urllib.request.urlopen", return_value=contextlib.closing(io.BytesIO(b"{}"))):
            with self.assertRaisesRegex(ValueError, "already exists"):
                release.check_unused("1.2.3")
        for status in [401, 403, 429, 500]:
            with patch.dict(os.environ, env), patch("release.urllib.request.urlopen", side_effect=urllib.error.HTTPError("https://example", status, "failed", {}, None)):
                with self.assertRaisesRegex(ValueError, f"HTTP {status}"):
                    release.check_unused("1.2.3")
        with patch.dict(os.environ, env), patch("release.urllib.request.urlopen", side_effect=urllib.error.URLError("offline")):
            with self.assertRaises(urllib.error.URLError):
                release.check_unused("1.2.3")


if __name__ == "__main__":
    unittest.main()
