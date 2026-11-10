"""Portable packaging regressions; no Linux tools, Docker, or installation required."""
import gzip
import io
import os
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile
import unittest
from unittest.mock import patch

import package_deb as deb


class DebianFixture:
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="dn-deb-test-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.identity = "DN Maintainer <maintainer@example.org>"
        (self.root / "Cargo.toml").write_text('[package]\nname="directory-navigator"\nversion="1.1.10"\n[[bin]]\nname="dn"\n[package.metadata.deb]\ndepends="$auto"\n')
        (self.root / "Cargo.lock").write_text('version=4\n[[package]]\nname="directory-navigator"\nversion="1.1.10"\n')
        for source in deb.DOCUMENTS.values():
            path = self.root / source
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(("fixture " + source + " Ёжик\n").encode())
        generated = self.root / "target/debian/.metadata"
        generated.mkdir(parents=True)
        (generated / "changelog.Debian.gz").write_bytes(gzip.compress(b"fixture changelog", mtime=0))

    def entries(self, arch="amd64"):
        header = bytearray(64)
        header[:6] = b"\x7fELF\x02\x01"
        header[18:20] = deb.ARCHES[arch][1].to_bytes(2, "little")
        entries = [("./usr/bin/dn", bytes(header), 0o755)]
        for dest, source in deb.DOCUMENTS.items():
            content = (self.root / source).read_bytes()
            if dest == deb.DOC + "copyright":
                entries.append(("./" + dest, content, 0o644))
            else:
                entries.append(("./" + dest + ".gz", gzip.compress(content, mtime=0), 0o644))
        entries.extend([
            ("./" + deb.DOC + "changelog.Debian.gz", gzip.compress(b"fixture changelog", mtime=0), 0o644),
        ])
        return entries

    def payload(self, entries, binary):
        data = io.BytesIO()
        with tarfile.open(fileobj=data, mode="w") as archive:
            for name, contents, mode in entries:
                info = tarfile.TarInfo(name)
                info.size = len(contents)
                info.mode = mode
                archive.addfile(info, io.BytesIO(contents))
        data.seek(0)
        with tarfile.open(fileobj=data, mode="r|") as archive:
            deb.payload(archive, self.root, binary)

class DebianTests(DebianFixture, unittest.TestCase):
    def test_versions_revision_and_identity_are_not_shell_fragments(self):
        self.assertEqual(deb.deb_version("1.1.10", "1"), "1.1.10-1")
        self.assertEqual(deb.deb_version("1.2.0-rc.1+build.2", "3"), "1.2.0~rc.1+build.2-3")
        for revision in ["", "0", "-1", "1\n", "../bad", "1; command"]:
            with self.assertRaises(ValueError):
                deb.deb_version("1.1.10", revision)
        self.assertEqual(deb.maintainer(self.identity), self.identity)
        for identity in ["Name", "Name <bad>", "A <a@example.org>\nDepends: evil", "A\x00 <a@example.org>"]:
            with self.assertRaises(ValueError):
                deb.maintainer(identity)

    def test_metadata_matches_lock_and_does_not_rewrite_it(self):
        before = (self.root / "Cargo.lock").read_bytes()
        self.assertEqual(deb.metadata(self.root), "1.1.10")
        self.assertEqual(before, (self.root / "Cargo.lock").read_bytes())
        (self.root / "Cargo.lock").write_bytes(before.replace(b"1.1.10", b"1.1.9"))
        with self.assertRaisesRegex(ValueError, "versions must match"):
            deb.metadata(self.root)

    def test_native_environment_rejects_wrong_host_before_building(self):
        with patch.object(deb.platform, "system", return_value="Darwin"), patch.object(deb, "run") as run:
            with self.assertRaisesRegex(ValueError, "not macOS/Windows"):
                deb.environment("amd64")
            run.assert_not_called()
        for arch in deb.ARCHES:
            with patch.object(deb.platform, "system", return_value="Linux"), patch.object(deb.platform, "freedesktop_os_release", return_value={"ID":"debian"}, create=True), patch.object(deb.shutil, "which", return_value="tool"), patch.object(deb, "run", side_effect=[arch,"host: " + deb.ARCHES[arch][0]]):
                self.assertEqual(deb.environment(arch), {"ID":"debian"})
            with patch.object(deb.platform, "system", return_value="Linux"), patch.object(deb.platform, "freedesktop_os_release", return_value={"ID":"debian"}, create=True), patch.object(deb.shutil, "which", return_value="tool"), patch.object(deb, "run", return_value="other"):
                with self.assertRaisesRegex(ValueError, "Architecture must match"):
                    deb.environment(arch)

    def test_payload_streams_both_architectures_and_verifies_documents(self):
        for arch in deb.ARCHES:
            binary = self.root / (arch + "-dn")
            self.payload(self.entries(arch), binary)
            deb.elf(binary, arch)
            other = "arm64" if arch == "amd64" else "amd64"
            with self.assertRaises(ValueError):
                deb.elf(binary, other)
        binary = self.root / "bad-dn"
        binary.write_bytes(b"Mach-O or PE is not a Linux binary")
        with self.assertRaises(ValueError):
            deb.elf(binary, "amd64")

    def test_missing_changed_duplicate_and_foreign_assets_are_rejected(self):
        entries = self.entries()
        cases = [entries[1:], entries + [entries[1]], entries + [("./etc/dn.conf",b"outside",0o644)],
                 entries + [("./usr/bin/evil",b"outside",0o755)],
                 [entry if "README.md.gz" not in entry[0] else (entry[0],gzip.compress(b"changed"),entry[2]) for entry in entries]]
        for i, case in enumerate(cases):
            with self.assertRaises(ValueError):
                self.payload(case, self.root / f"bad-{i}")

    def test_paths_links_special_files_privilege_bits_and_ownership_are_rejected(self):
        for name in ["/usr/bin/dn","../outside","usr/../../outside"]:
            info = tarfile.TarInfo(name)
            with self.assertRaises(ValueError): deb.member_name(info)
        for kind in [tarfile.SYMTYPE,tarfile.LNKTYPE,tarfile.FIFOTYPE,tarfile.CHRTYPE]:
            info = tarfile.TarInfo("usr/bin/dn"); info.type=kind
            with self.assertRaises(ValueError): deb.member_name(info)
        for attribute, value in [("uid",1000),("gid",1000),("mode",0o4755),("mode",0o777)]:
            info = tarfile.TarInfo("usr/bin/dn"); setattr(info,attribute,value)
            with self.assertRaises(ValueError): deb.member_name(info)

    def test_local_publication_never_overwrites_and_checksum_matches(self):
        stage = self.root / "stage"; output = self.root / "output"
        stage.mkdir(); output.mkdir()
        package = stage / "directory-navigator_1.1.10-1_amd64.deb"
        package.write_bytes(b"private publication fixture, not a real Debian package")
        dest = deb.publish_files(package, output)
        self.assertEqual(dest.read_bytes(),package.read_bytes())
        self.assertEqual(Path(str(dest)+".sha256").read_text(),f"{deb.release.digest(dest)}  {dest.name}\n")
        with self.assertRaises(FileExistsError): deb.publish_files(package,output)
        self.assertEqual(dest.read_bytes(),package.read_bytes())

    def test_checksum_collision_cleans_only_newly_linked_package(self):
        stage = self.root / "stage"; output = self.root / "output"
        stage.mkdir(); output.mkdir()
        package=stage / "fixture.deb"; package.write_bytes(b"new")
        (output / "fixture.deb.sha256").write_bytes(b"keep")
        with self.assertRaises(FileExistsError): deb.publish_files(package,output)
        self.assertFalse((output / "fixture.deb").exists())
        self.assertEqual((output / "fixture.deb.sha256").read_bytes(),b"keep")

    def test_container_test_has_readonly_private_mount_and_no_host_install(self):
        package=self.root / "fixture.deb"; package.write_bytes(b"container command fixture")
        for arch in deb.ARCHES:
            with patch.object(deb.shutil,"which",return_value="docker"), patch.object(deb,"run") as run:
                deb.smoke(package,arch,"1.1.10-1","1.1.10","debian:bookworm-slim")
                args=run.call_args.args[0]
                self.assertEqual(args[:3],["docker","run","--rm"])
                self.assertIn("linux/"+arch,args)
                self.assertTrue(any("dst=/packages,readonly" in str(arg) for arg in args))
                self.assertNotIn("--privileged",args)
                self.assertIn("apt-get remove",deb.SMOKE)
                self.assertIn("+dnsmoke1",deb.SMOKE)
        with self.assertRaises(ValueError): deb.smoke(package,"amd64","v","a","--privileged")

    def test_cli_help_is_available_without_linux_tools(self):
        result=subprocess.run([sys.executable,str(Path(deb.__file__)),"--help"],capture_output=True,text=True,check=True)
        self.assertIn("amd64,arm64",result.stdout)
        self.assertIn("never installs on host",result.stdout)

    def test_checked_in_assets_and_metadata_are_complete(self):
        import tomllib
        root=Path(__file__).resolve().parents[1]
        metadata=tomllib.loads((root / "Cargo.toml").read_text())["package"]["metadata"]["deb"]
        self.assertEqual(metadata["depends"],"$auto")
        self.assertEqual(metadata["name"],deb.NAME)
        assets={destination:source for source,destination,_ in metadata["assets"]}
        self.assertEqual(assets["usr/bin/dn"],"target/release/dn")
        for target,source in deb.DOCUMENTS.items():
            if target == deb.DOC + "copyright":
                self.assertEqual(metadata["license-file"], [source,"0"])
            else:
                self.assertEqual(assets[target],source)
        copyright = (root / "packaging/debian/copyright").read_text()
        normalized = "\n".join("" if line == " ." else line.removeprefix(" ") for line in copyright.splitlines())+"\n"
        self.assertIn((root / "LICENSE").read_text(), normalized)
        self.assertTrue(copyright.startswith("Format: https://www.debian.org/"))
        self.assertNotIn("maintainer-scripts",metadata)
        self.assertNotIn("systemd-units",metadata)


class PipelineTests(DebianFixture, unittest.TestCase):
    def test_build_routes_both_architectures_and_never_installs_or_uploads(self):
        from unittest.mock import MagicMock
        for arch in deb.ARCHES:
            output = self.root / ("output-" + arch)
            seen = []
            def command(args, cwd=None):
                seen.append(args)
                Path(args[args.index("--output")+1]).write_bytes(b"private command-routing fixture")
                return ""
            with patch.object(deb,"environment"), patch.object(deb,"run",side_effect=command), patch.object(deb,"validate") as validate, patch.dict(sys.modules,{"fcntl":MagicMock()}):
                package=deb.build(self.root,arch,"2",self.identity,output)
            self.assertEqual(package.name,f"directory-navigator_1.1.10-2_{arch}.deb")
            self.assertEqual(len(seen),1)
            self.assertIn(deb.ARCHES[arch][0],seen[0])
            self.assertIn("--locked",seen[0])
            self.assertNotIn("--install",seen[0])
            self.assertEqual(seen[0][:2],["cargo","deb"])
            validate.assert_called_once()

    def test_validation_failure_publishes_nothing_and_preserves_sources(self):
        from unittest.mock import MagicMock
        before=(self.root / "Cargo.toml").read_bytes(),(self.root / "Cargo.lock").read_bytes()
        output=self.root / "out"
        def command(args,cwd=None):
            Path(args[args.index("--output")+1]).write_bytes(b"fixture")
            return ""
        with patch.object(deb,"environment"),patch.object(deb,"run",side_effect=command),patch.object(deb,"validate",side_effect=ValueError("invalid package")),patch.dict(sys.modules,{"fcntl":MagicMock()}):
            with self.assertRaisesRegex(ValueError,"invalid package"):
                deb.build(self.root,"amd64","1",self.identity,output)
        self.assertEqual(list(output.iterdir()),[])
        self.assertEqual(before,((self.root / "Cargo.toml").read_bytes(),(self.root / "Cargo.lock").read_bytes()))

    def test_validation_requires_resolved_dependencies_and_alsa(self):
        from contextlib import contextmanager
        @contextmanager
        def fake_archive(*args):
            yield []
        for recorded,resolved,accepted in [
            ("libc6", "libc6", False),
            ("libasound2, libc6", "libc6", False),
            ("", "", False),
            ("libasound2 (>= 1.0.16), libc6 (>= 2.36)", "libc6 (>= 2.36), libasound2 (>= 1.0.16)", True),
            ("libasound2t64 (>= 1.0.16), libc6", "libc6, libasound2t64 (>= 1.0.16)", True),
        ]:
            with tempfile.TemporaryDirectory(dir=self.root) as temporary:
                work=Path(temporary)
                fields={"Package":deb.NAME,"Version":"1.1.10-1","Architecture":"amd64","Maintainer":self.identity,"Depends":recorded}
                def command(args,cwd=None):
                    if args[0]=="dpkg-deb": return fields[args[-1]]
                    if args[0]=="dpkg-shlibdeps": return "shlibs:Depends="+resolved
                    return "Directory Navigator 1.1.10"
                with patch.object(deb,"archive",fake_archive),patch.object(deb,"payload"),patch.object(deb,"elf"),patch.object(deb,"run",side_effect=command):
                    if accepted:
                        deb.validate(work / "package.deb",self.root,work,"amd64","1.1.10-1",self.identity,"1.1.10")
                    else:
                        with self.assertRaises(ValueError):
                            deb.validate(work / "package.deb",self.root,work,"amd64","1.1.10-1",self.identity,"1.1.10")

class LibgccRegressionTests(DebianFixture, unittest.TestCase):
    def test_cargo_deb_libgcc_omission_is_restored_and_final_package_revalidated(self):
        from contextlib import contextmanager
        @contextmanager
        def fake_archive(*args):
            yield []
        for arch, outcome in ((arch, outcome) for arch in deb.ARCHES
                              for outcome in ("success", "build failure", "still missing")):
            with tempfile.TemporaryDirectory(dir=self.root) as temporary:
                work = Path(temporary)
                package = work / "package.deb"
                package.write_bytes(b"private packaging-command fixture, not a real deb")
                expected = "libasound2t64 (>= 1.0.29), libc6 (>= 2.39), libgcc-s1 (>= 4.2)"
                fields = {"Package":deb.NAME, "Version":"1.1.10-1", "Architecture":arch,
                          "Maintainer":self.identity, "Depends":"libasound2t64 (>= 1.0.29), libc6 (>= 2.39)"}
                calls = []
                def command(args, cwd=None):
                    calls.append(args)
                    if args[:2] == ["dpkg-deb", "--field"]:
                        return fields[args[-1]]
                    if args[:2] == ["dpkg-deb", "--raw-extract"]:
                        control = Path(args[-1]) / "DEBIAN/control"
                        control.parent.mkdir(parents=True, exist_ok=True)
                        control.write_text("Package: directory-navigator\nVersion: 1.1.10-1\nDepends: libasound2t64 (>= 1.0.29),\n libc6 (>= 2.39)\nDescription: keep this\n continuation stays\n")
                    elif args[:2] == ["dpkg-deb", "--build"]:
                        if outcome == "build failure":
                            raise ValueError("rebuild failed")
                        control = (Path(args[-2]) / "DEBIAN/control").read_text()
                        self.assertIn("Description: keep this\n continuation stays\n", control)
                        repaired = next(line.removeprefix("Depends: ") for line in control.splitlines() if line.startswith("Depends: "))
                        self.assertEqual(deb.dependencies(repaired), deb.dependencies(expected))
                        if outcome != "still missing":
                            fields["Depends"] = repaired
                        Path(args[-1]).write_bytes(b"repacked fixture")
                    elif args[0] == "dpkg-shlibdeps":
                        self.assertNotIn("--ignore-missing-info", args)
                        return "shlibs:Depends=" + expected
                    elif args[0] != "lintian":
                        return "Directory Navigator 1.1.10"
                    return ""
                with patch.object(deb,"archive",fake_archive), patch.object(deb,"payload"), patch.object(deb,"elf"), patch.object(deb,"run",side_effect=command):
                    if outcome == "success":
                        deb.validate(package,self.root,work,arch,"1.1.10-1",self.identity,"1.1.10")
                    else:
                        with self.assertRaisesRegex(ValueError, "rebuild failed|Runtime dependencies"):
                            deb.validate(package,self.root,work,arch,"1.1.10-1",self.identity,"1.1.10")
                self.assertEqual(sum(args[0]=="dpkg-shlibdeps" for args in calls), 1 if outcome == "build failure" else 2)
                self.assertEqual(sum(args[0]=="lintian" for args in calls), int(outcome == "success"))
                self.assertEqual(sum(args[:2]==["dpkg-deb", "--build"] for args in calls), 1)
                if outcome == "build failure":
                    self.assertEqual(package.read_bytes(), b"private packaging-command fixture, not a real deb")
                else:
                    self.assertEqual(package.read_bytes(), b"repacked fixture")

    def test_repair_is_limited_to_missing_libgcc_s1(self):
        base = "libasound2t64 (>= 1.0.29), libc6 (>= 2.39)"
        self.assertTrue(deb.omitted_libgcc(base + ", libgcc-s1 (>= 4.2)", base))
        for actual, recorded in [
            (base + ", libstdc++6", base),
            (base + ", libgcc-s1, libstdc++6", base),
            (base + ", libgcc-other", base),
            (base + ", libgcc-s1 (>= 4.2)", base + ", libgcc-s1 (>= 3.0)"),
            (base + ", libgcc-s1", base + ", libgcc-s1:any"),
            (base + ", libgcc-s1", base + ", unexpected"),
            (base, base),
        ]:
            with self.subTest(actual=actual, recorded=recorded):
                self.assertFalse(deb.omitted_libgcc(actual, recorded))

    def test_control_rewrite_preserves_other_fields_and_rejects_ambiguity(self):
        for newline in ("\n", "\r\n"):
            before = newline.join(["Package: dn", "Depends: libc6,", " libasound2", "Description: text", " continuation", ""])
            result = deb.replace_depends(before, "libc6, libasound2, libgcc-s1")
            self.assertIn("Description: text" + newline + " continuation" + newline, result)
            self.assertIn("Depends: libc6, libasound2, libgcc-s1\n", result)
        for control, value in [("Package: dn\n", "libgcc-s1"),
                               ("Depends: a\nDepends: b\n", "libgcc-s1"),
                               ("Depends: a\n", "libgcc-s1\nInjected: yes"),
                               ("Depends: a\n", "")]:
            with self.assertRaises(ValueError):
                deb.replace_depends(control, value)


if __name__ == "__main__":
    unittest.main()
