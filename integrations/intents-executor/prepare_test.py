#!/usr/bin/env python3
"""Offline preparation tests; fixtures exercise bytes and paths, never Cargo/builds."""

import copy
import importlib.util
import io
import json
from pathlib import Path
import tarfile
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("executor_prepare", Path(__file__).with_name("prepare.py"))
prepare = importlib.util.module_from_spec(spec)
spec.loader.exec_module(prepare)

WORKSPACE = b'''[workspace]
members = [
  "contracts/defuse",
]
[workspace.dependencies]
near-sdk = "5.30.0"
'''
DEFUSE = b'''[dependencies]
near-sdk = { workspace = true, features = ["deterministic-account-ids"] }
[dev-dependencies]
defuse = { path = ".", features = ["abi"] }
near-sdk = { workspace = true, features = ["non-contract-usage"] }
'''
MODULE = b"mod deposit;\nmod native;\nmod storage_deposit;\nmod withdraw;\n"
VENDOR_MANIFEST = b'''[package]
name = "near-plugins"
version = "0.5.3"
[dependencies.near-sdk]
version = "5.2"
[dev-dependencies.near-sdk]
version = "5.2"
features = ["unit-testing"]
'''


class PreparationTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name).resolve()
        self.package = self.root / "package"
        self.shared = self.root / "shared"
        self.destination = self.root / "source"
        self.archive = self.root / "archive.tar.gz"
        self.files = {
            "Cargo.toml": WORKSPACE, "Cargo.lock": b"original upstream lock\n",
            "contracts/defuse/Cargo.toml": DEFUSE, prepare.MODULE_PATH: MODULE,
            "src/ordinary.rs": b"ordinary unchanged upstream source\n",
        }
        self.overlay = b"// source overlay test fixture; not an executor implementation\n"
        path = self.package / "overlay" / prepare.JOURNAL_PATH
        path.parent.mkdir(parents=True)
        path.write_bytes(self.overlay)
        shared_files = {"Cargo.toml": b"shared crate manifest\n", "src/lib.rs": b"shared crate source\n"}
        for name, raw in shared_files.items():
            path = self.shared / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(raw)
        self.vendor_files = {name: b"unchanged vendor source fixture\n" for name in prepare.VENDOR_FILES}
        self.vendor_files["Cargo.toml"] = VENDOR_MANIFEST
        for name, raw in self.vendor_files.items():
            path = self.package / prepare.VENDOR_SOURCE / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(raw)
        staged_vendor = dict(self.vendor_files)
        staged_vendor["Cargo.toml"] = VENDOR_MANIFEST.replace(b'version = "5.2"\n', b'version = "5.2"\ndefault-features = false\n')
        self.links = [
            {"path": ".claude/CLAUDE.md", "target": "../.agents/AGENTS.md", "type": "symbolic"},
            {"path": ".claude/skills", "target": "../.agents/skills/", "type": "symbolic"},
            {"path": "releases/previous.wasm", "target": "missing.wasm", "type": "symbolic"},
        ]
        self.manifest = {
            "format_version": 1, "recipe": "native-nep141-journal-overlay-v1",
            "upstream": {"archive_root": "intents-test", "commit": "test-commit"},
            "skipped_links": self.links,
            "baseline_files": {name: prepare.digest(raw) for name, raw in self.files.items()},
            "overlay_files": {prepare.JOURNAL_PATH: prepare.digest(self.overlay)},
            "shared_crate_source": "unused-test-path", "shared_crate_destination": prepare.SHARED_DESTINATION,
            "shared_crate_files": {name: prepare.digest(raw) for name, raw in shared_files.items()},
            "vendor_crate": {
                "name": "near-plugins", "version": "0.5.3", "license": "CC0-1.0",
                "archive_sha256": "test-vendor-archive-pin", "archive_bytes": 123,
                "source": prepare.VENDOR_SOURCE, "destination": prepare.VENDOR_DESTINATION,
                "original_files": {name: prepare.digest(raw) for name, raw in self.vendor_files.items()},
                "staged_files": {name: prepare.digest(raw) for name, raw in staged_vendor.items()},
            },
        }
        self.make_archive()

    def tearDown(self):
        self.temporary.cleanup()

    def make_archive(self, extra=None):
        entries = [("intents-test/" + name, tarfile.REGTYPE, raw) for name, raw in self.files.items()]
        entries += [("intents-test/" + link["path"], tarfile.SYMTYPE, link["target"]) for link in self.links]
        if extra:
            entries.append(extra)
        with tarfile.open(self.archive, "w:gz") as archive:
            for name, kind, raw in entries:
                info = tarfile.TarInfo(name)
                info.type = kind
                info.mode = 0o644
                if kind == tarfile.REGTYPE:
                    info.size = len(raw)
                    archive.addfile(info, io.BytesIO(raw))
                else:
                    info.linkname = raw if isinstance(raw, str) else ""
                    archive.addfile(info)
        raw = self.archive.read_bytes()
        self.manifest["upstream"].update({
            "archive_bytes": len(raw), "archive_sha256": prepare.digest(raw),
            "regular_files": sum(kind == tarfile.REGTYPE for _, kind, _ in entries),
            "unpacked_file_bytes": sum(len(raw) for _, kind, raw in entries if kind == tarfile.REGTYPE),
        })

    def run_prepare(self):
        return prepare.prepare_source(self.archive, self.destination, manifest=self.manifest,
                                      package_root=self.package, shared_root=self.shared)

    def test_valid_preparation_patches_only_reviewed_files_and_preserves_lock(self):
        result = self.run_prepare()
        self.assertEqual(result["source_root"], str(self.destination))
        self.assertFalse(result["reused"])
        self.assertEqual((self.destination / "Cargo.lock").read_bytes(), self.files["Cargo.lock"])
        self.assertEqual((self.destination / "src/ordinary.rs").read_bytes(), self.files["src/ordinary.rs"])
        self.assertEqual((self.destination / prepare.MODULE_PATH).read_bytes(), MODULE + b"mod journal;\n")
        self.assertEqual((self.destination / prepare.JOURNAL_PATH).read_bytes(), self.overlay)
        self.assertIn('default-features = false', (self.destination / "Cargo.toml").read_text())
        workspace = (self.destination / "Cargo.toml").read_text()
        self.assertIn('  "crates/mandate-near-plugins",\n', workspace)
        self.assertTrue(workspace.endswith('[patch.crates-io]\nnear-plugins = { path = "crates/mandate-near-plugins" }\n'))
        cargo = (self.destination / "contracts/defuse/Cargo.toml").read_text()
        self.assertIn('features = ["contract"]', cargo)
        self.assertIn('"non-contract-usage", "unit-testing"', cargo)
        self.assertEqual((self.destination / prepare.SHARED_DESTINATION / "src/lib.rs").read_bytes(), b"shared crate source\n")
        for link in self.links:
            self.assertFalse((self.destination / link["path"]).exists())
            self.assertFalse((self.destination / link["path"]).is_symlink())
        self.assertTrue((self.destination / prepare.MARKER).is_file())

    def test_vendor_preserves_source_and_changes_only_sdk_manifest_defaults(self):
        self.run_prepare()
        for name, original in self.vendor_files.items():
            self.assertEqual((self.package / prepare.VENDOR_SOURCE / name).read_bytes(), original)
            staged = (self.destination / prepare.VENDOR_DESTINATION / name).read_bytes()
            expected = original
            if name == "Cargo.toml":
                expected = original.replace(b'version = "5.2"\n', b'version = "5.2"\ndefault-features = false\n')
            self.assertEqual(staged, expected)
            self.assertEqual(prepare.digest(staged), self.manifest["vendor_crate"]["staged_files"][name])
        marker = json.loads((self.destination / prepare.MARKER).read_text())
        self.assertEqual(marker["inputs"]["vendor_crate"], self.manifest["vendor_crate"])

    def test_changed_original_vendor_source_hash_is_refused(self):
        (self.package / prepare.VENDOR_SOURCE / "src/lib.rs").write_bytes(b"modified vendor source")
        with self.assertRaisesRegex(prepare.PreparationError, "input hash"):
            self.run_prepare()
        self.assertFalse(self.destination.exists())

    def test_changed_original_vendor_manifest_hash_is_refused(self):
        (self.package / prepare.VENDOR_SOURCE / "Cargo.toml").write_bytes(b"modified vendor manifest")
        with self.assertRaisesRegex(prepare.PreparationError, "input hash"):
            self.run_prepare()

    def test_changed_staged_vendor_manifest_pin_is_refused(self):
        self.manifest["vendor_crate"]["staged_files"]["Cargo.toml"] = "0" * 64
        with self.assertRaisesRegex(prepare.PreparationError, "staged vendor hash"):
            self.run_prepare()

    def test_unreviewed_vendor_selection_is_refused(self):
        self.manifest["vendor_crate"]["destination"] = "crates/unreviewed-path"
        with self.assertRaisesRegex(prepare.PreparationError, "unsupported vendor selection"):
            self.run_prepare()

    def test_changed_staged_vendor_source_is_preserved_and_refused(self):
        self.run_prepare()
        path = self.destination / prepare.VENDOR_DESTINATION / "src/lib.rs"
        path.write_bytes(b"user edits to vendor must remain")
        with self.assertRaisesRegex(prepare.PreparationError, "staged source differs"):
            self.run_prepare()
        self.assertEqual(path.read_bytes(), b"user edits to vendor must remain")

    def test_changed_vendor_input_binding_is_refused_on_reuse(self):
        self.run_prepare()
        self.manifest["vendor_crate"]["archive_sha256"] = "changed provenance"
        with self.assertRaisesRegex(prepare.PreparationError, "input manifest does not match"):
            self.run_prepare()

    def test_second_preparation_reuses_verified_tree_without_rewriting_sources(self):
        self.run_prepare()
        before = (self.destination / prepare.JOURNAL_PATH).stat().st_mtime_ns
        self.assertTrue(self.run_prepare()["reused"])
        self.assertEqual((self.destination / prepare.JOURNAL_PATH).stat().st_mtime_ns, before)

    def test_wrong_archive_hash_is_refused_before_unpacking(self):
        raw = bytearray(self.archive.read_bytes())
        raw[0] ^= 1
        self.archive.write_bytes(raw)
        with self.assertRaisesRegex(prepare.PreparationError, "archive size or hash"):
            self.run_prepare()
        self.assertFalse(self.destination.exists())

    def test_malicious_archive_paths_are_refused_even_with_fixture_hash_matching(self):
        for path in ["intents-test/../escape", "/absolute-escape", "C:/escape", "intents-test/a\\escape", "intents-test/./escape"]:
            with self.subTest(path=path):
                self.make_archive((path, tarfile.REGTYPE, b"bad"))
                with self.assertRaises(prepare.PreparationError):
                    self.run_prepare()
                self.assertFalse(self.destination.exists())
                self.assertFalse((self.root / "escape").exists())

    def test_special_archive_member_is_refused(self):
        self.make_archive(("intents-test/fifo", tarfile.FIFOTYPE, b""))
        with self.assertRaisesRegex(prepare.PreparationError, "unsupported archive member"):
            self.run_prepare()

    def test_duplicate_archive_path_is_refused(self):
        self.make_archive(("intents-test/Cargo.toml", tarfile.REGTYPE, WORKSPACE))
        with self.assertRaisesRegex(prepare.PreparationError, "duplicate archive path"):
            self.run_prepare()

    def test_changed_overlay_hash_is_refused(self):
        (self.package / "overlay" / prepare.JOURNAL_PATH).write_bytes(b"changed overlay")
        with self.assertRaisesRegex(prepare.PreparationError, "input hash"):
            self.run_prepare()

    def test_changed_shared_crate_hash_is_refused(self):
        (self.shared / "src/lib.rs").write_bytes(b"changed shared crate")
        with self.assertRaisesRegex(prepare.PreparationError, "input hash"):
            self.run_prepare()

    def test_changed_staged_source_is_preserved_and_refused(self):
        self.run_prepare()
        path = self.destination / "src/ordinary.rs"
        path.write_bytes(b"user edits must remain")
        with self.assertRaisesRegex(prepare.PreparationError, "staged source differs"):
            self.run_prepare()
        self.assertEqual(path.read_bytes(), b"user edits must remain")

    def test_forging_marker_file_hashes_does_not_accept_changed_source(self):
        self.run_prepare()
        path = self.destination / "src/ordinary.rs"
        path.write_bytes(b"changed source")
        marker = json.loads((self.destination / prepare.MARKER).read_text())
        marker["files"] = prepare.tree_files(self.destination)
        (self.destination / prepare.MARKER).write_text(json.dumps(marker))
        with self.assertRaisesRegex(prepare.PreparationError, "staged source differs"):
            self.run_prepare()

    def test_existing_unverified_user_tree_is_never_replaced(self):
        self.destination.mkdir()
        user_file = self.destination / "keep.txt"
        user_file.write_bytes(b"preserve")
        with self.assertRaisesRegex(prepare.PreparationError, "existing unverified"):
            self.run_prepare()
        self.assertEqual(user_file.read_bytes(), b"preserve")

    def test_extra_staged_file_is_refused_without_deleting_it(self):
        self.run_prepare()
        user_file = self.destination / "new-user-file.txt"
        user_file.write_bytes(b"preserve")
        with self.assertRaisesRegex(prepare.PreparationError, "staged source differs"):
            self.run_prepare()
        self.assertEqual(user_file.read_bytes(), b"preserve")

    def test_symbolic_destination_is_refused(self):
        outside = self.root / "outside"
        outside.mkdir()
        self.destination.symlink_to(outside, target_is_directory=True)
        with self.assertRaisesRegex(prepare.PreparationError, "symbolic-link"):
            self.run_prepare()
        self.assertEqual(list(outside.iterdir()), [])

    def test_frozen_package_lock_is_used_without_modifying_original_archive(self):
        frozen = b"reviewed package lock\n"
        (self.package / "Cargo.lock").write_bytes(frozen)
        self.run_prepare()
        self.assertEqual((self.destination / "Cargo.lock").read_bytes(), frozen)
        self.assertEqual(self.manifest["baseline_files"]["Cargo.lock"], prepare.digest(self.files["Cargo.lock"]))
        self.assertEqual(self.run_prepare()["lock_source"], "reviewed-package-lock")

    def test_initial_generated_lock_requires_frozen_review_before_reuse(self):
        self.run_prepare()
        reviewed = b"new explicitly reviewed lock\n"
        (self.destination / "Cargo.lock").write_bytes(reviewed)
        with self.assertRaisesRegex(prepare.PreparationError, "staged source differs"):
            self.run_prepare()
        (self.package / "Cargo.lock").write_bytes(reviewed)
        self.assertTrue(self.run_prepare()["reused"])
        marker = json.loads((self.destination / prepare.MARKER).read_text())
        self.assertEqual(marker["inputs"]["effective_lock_sha256"], prepare.digest(reviewed))
        self.assertEqual(marker["inputs"]["lock_source"], "reviewed-package-lock")

    def test_changed_baseline_hash_is_refused(self):
        self.manifest = copy.deepcopy(self.manifest)
        self.manifest["baseline_files"]["Cargo.toml"] = "0" * 64
        with self.assertRaisesRegex(prepare.PreparationError, "input hash"):
            self.run_prepare()


if __name__ == "__main__":
    unittest.main()
