"""Cargo configuration policy, checked before the release platform builds.

Keep TOML parsing here: workflow_policy.rs also runs directly under rustc,
without Cargo dependencies. Python 3.12 is installed by the release workflow.
"""
from pathlib import Path
import tomllib
import unittest


ROOT = Path(__file__).resolve().parent.parent


def assert_windows_static_crt(test, text):
    config = tomllib.loads(text)
    targets = config["target"]
    flags = targets["x86_64-pc-windows-msvc"]["rustflags"]
    test.assertIsInstance(flags, list)
    test.assertIn(("-C", "target-feature=+crt-static"), list(zip(flags, flags[1:])))
    test.assertNotIn("+crt-static", str(config.get("build", {}).get("rustflags", "")))
    test.assertNotIn("+crt-static", str(config.get("env", {}).get("RUSTFLAGS", "")))
    for target, settings in targets.items():
        if not target.endswith("windows-msvc"):
            test.assertNotIn("+crt-static", str(settings.get("rustflags", "")), target)


def line_endings(text):
    lf = text.replace("\r\n", "\n")
    return (lf, lf.replace("\n", "\r\n"), lf.replace("\n", "\r\n", 1))


class CargoWorkflowPolicyTests(unittest.TestCase):
    def test_windows_builds_embed_crt_without_changing_other_targets(self):
        text = (ROOT / ".cargo/config.toml").read_bytes().decode("utf-8")
        for variant in line_endings(text):
            with self.subTest(text=repr(variant)):
                assert_windows_static_crt(self, variant)

    def test_crt_policy_rejects_missing_windows_flags_and_non_windows_linkage(self):
        base = ('[target.x86_64-pc-windows-msvc]\n'
                'rustflags = ["-C", "target-feature=+crt-static"]\n')
        unsafe = [
            base.replace('"target-feature=+crt-static"', '"target-feature=-crt-static"'),
            base + '[build]\nrustflags = ["-C", "target-feature=+crt-static"]\n',
            base + '[env]\nRUSTFLAGS = { value = "-C target-feature=+crt-static", force = true }\n',
            base + '[target.aarch64-apple-darwin]\nrustflags = ["-C", "target-feature=+crt-static"]\n',
            base + '[target.x86_64-unknown-linux-gnu]\nrustflags = ["-C", "target-feature=+crt-static"]\n',
        ]
        for text in unsafe:
            for variant in line_endings(text):
                with self.subTest(text=repr(variant)):
                    with self.assertRaises(AssertionError):
                        assert_windows_static_crt(self, variant)
