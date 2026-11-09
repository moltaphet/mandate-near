#!/usr/bin/env python3
"""Offline builder checks use real preparation and mocked compiler processes."""

import json
from pathlib import Path
import subprocess
import unittest
from unittest.mock import patch

import build
import prepare
import prepare_test


class BuildTests(unittest.TestCase):
    make_archive = prepare_test.PreparationTests.make_archive

    def setUp(self):
        prepare_test.PreparationTests.setUp(self)
        (self.package / "source-manifest.json").write_text(json.dumps(self.manifest))
        (self.package / "Cargo.lock").write_bytes(b"reviewed fixture lock\n")
        self.output = self.root / "output"
        self.artifact = self.output / "artifact/defuse.wasm"
        self.record = self.output / build.BUILD_MANIFEST
        self.commands = []

    def tearDown(self):
        prepare_test.PreparationTests.tearDown(self)

    def run_build(self, action=None):
        def compiler(command, **kwargs):
            self.commands.append((command, kwargs))
            self.assertNotIn("shell", kwargs)
            if "--version" in command:
                return subprocess.CompletedProcess(command, 0, stdout="mock tool version\n")
            if action:
                action(command, kwargs)
            else:
                self.artifact.write_bytes(build.WASM_HEADER)
            return subprocess.CompletedProcess(command, 0)

        with patch.object(build.subprocess, "run", side_effect=compiler):
            return build.build_executor(self.archive, self.destination, self.output,
                                        package_root=self.package, shared_root=self.shared)

    def previous_outputs(self):
        self.artifact.parent.mkdir(parents=True)
        self.artifact.write_bytes(build.WASM_HEADER + b"old artifact")
        self.record.write_text('{"stale":true}')

    def test_success_records_final_wasm_and_exact_locked_offline_command(self):
        result = self.run_build()
        manifest = json.loads(self.record.read_text())
        preparation = json.loads((self.destination / prepare.MARKER).read_text())
        self.assertEqual(result["build_manifest"], str(self.record))
        self.assertEqual(manifest["source_root"], str(self.destination))
        self.assertEqual(manifest["inputs"], preparation["inputs"])
        self.assertEqual(manifest["inputs"]["lock_source"], "reviewed-package-lock")
        self.assertEqual(manifest["artifact"], {"path": "artifact/defuse.wasm",
                                               "bytes": len(build.WASM_HEADER),
                                               "sha256": prepare.digest(build.WASM_HEADER)})
        command, options = self.commands[-1]
        self.assertEqual(command[:5], ["cargo", "+1.95.0", "near", "build", "non-reproducible-wasm"])
        self.assertIn("--locked", command)
        self.assertIn("--no-abi", command)
        self.assertEqual(command[command.index("--features") + 1], "contract")
        self.assertEqual(command[command.index("--override-toolchain") + 1], "1.95.0")
        self.assertEqual(command[command.index("--out-dir") + 1], str(self.artifact.parent))
        self.assertEqual(options["cwd"], self.destination)
        self.assertTrue(options["check"])
        self.assertEqual(options["env"]["CARGO_NET_OFFLINE"], "true")
        self.assertEqual(options["env"]["CARGO_TARGET_DIR"], str(self.output / "build"))
        self.assertEqual(options["env"]["RUSTUP_AUTO_INSTALL"], "0")
        self.assertEqual(manifest["build"]["command"], command)
        self.assertEqual(set(manifest["build"]["versions"]), {"cargo", "rustc", "cargo_near"})
        self.assertEqual(manifest["deployment_source_correspondence"], "UNVERIFIED")

    def test_missing_frozen_lock_stops_before_compilation_or_output(self):
        (self.package / "Cargo.lock").unlink()
        with self.assertRaisesRegex(build.BuildError, "frozen package Cargo.lock"):
            self.run_build()
        self.assertFalse(self.commands)
        self.assertFalse(self.destination.exists())
        self.assertFalse(self.output.exists())

    def test_changed_archive_is_rejected_before_compilation(self):
        self.archive.write_bytes(b"changed archive")
        with self.assertRaisesRegex(prepare.PreparationError, "archive size or hash"):
            self.run_build()
        self.assertFalse(self.commands)
        self.assertFalse(self.record.exists())

    def test_changed_input_pin_is_rejected_before_compilation(self):
        (self.shared / "src/lib.rs").write_bytes(b"changed shared source")
        with self.assertRaisesRegex(prepare.PreparationError, "input hash"):
            self.run_build()
        self.assertFalse(self.commands)
        self.assertFalse(self.record.exists())

    def test_unverified_existing_source_is_preserved_and_rejected(self):
        self.destination.mkdir()
        user_file = self.destination / "user.txt"
        user_file.write_bytes(b"preserve")
        with self.assertRaisesRegex(prepare.PreparationError, "existing unverified"):
            self.run_build()
        self.assertFalse(self.commands)
        self.assertEqual(user_file.read_bytes(), b"preserve")
        self.assertFalse(self.record.exists())

    def test_failed_build_invalidates_old_record_and_stale_artifact(self):
        self.previous_outputs()

        def fail(command, options):
            self.assertFalse(self.record.exists())
            self.assertFalse(self.artifact.exists())
            raise subprocess.CalledProcessError(1, command)

        with self.assertRaises(subprocess.CalledProcessError):
            self.run_build(fail)
        self.assertFalse(self.record.exists())
        self.assertFalse(self.artifact.exists())

    def test_compiler_and_profile_environment_overrides_are_refused_before_mutation(self):
        self.previous_outputs()
        prior_record = self.record.read_bytes()
        prior_artifact = self.artifact.read_bytes()
        for name in ("RUSTC", "RUSTC_WRAPPER", "RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS",
                     "CARGO_PROFILE_RELEASE_OPT_LEVEL", "CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_LINKER",
                     "CARGO_BUILD_RUSTC"):
            with self.subTest(name=name), patch.dict(build.os.environ, {name: "unrecorded-value"}):
                with self.assertRaisesRegex(build.BuildError, "environment overrides refused") as error:
                    self.run_build()
                self.assertIn(name, str(error.exception))
                self.assertNotIn("unrecorded-value", str(error.exception))
                self.assertFalse(self.commands)
                self.assertFalse(self.destination.exists())
                self.assertEqual(self.record.read_bytes(), prior_record)
                self.assertEqual(self.artifact.read_bytes(), prior_artifact)

    def test_success_without_output_cannot_authenticate_old_artifact(self):
        self.previous_outputs()
        with self.assertRaises(FileNotFoundError):
            self.run_build(lambda command, options: None)
        self.assertFalse(self.record.exists())
        self.assertFalse(self.artifact.exists())

    def test_malformed_output_does_not_get_a_record(self):
        for raw in (b"", b"not wasm", b"\0asm\x02\0\0\0", build.WASM_HEADER + b"\xff",
                    build.WASM_HEADER + b"\x01\x08short", build.WASM_HEADER + b"\x01\x80"):
            with self.subTest(raw=raw):
                with self.assertRaisesRegex(build.BuildError, "WASM module"):
                    self.run_build(lambda command, options: self.artifact.write_bytes(raw))
                self.assertFalse(self.record.exists())

    def test_duplicate_source_metadata_is_rejected_before_compilation(self):
        raw = json.dumps(self.manifest)
        (self.package / "source-manifest.json").write_text('{"format_version":99,' + raw[1:])
        with self.assertRaisesRegex(build.BuildError, "duplicate preparation metadata"):
            self.run_build()
        self.assertFalse(self.commands)
        self.assertFalse(self.record.exists())

    def test_staged_source_change_during_build_refuses_new_record(self):
        def mutate(command, options):
            self.artifact.write_bytes(build.WASM_HEADER)
            (self.destination / "src/ordinary.rs").write_bytes(b"changed after compilation")

        with self.assertRaisesRegex(prepare.PreparationError, "staged source differs"):
            self.run_build(mutate)
        self.assertFalse(self.record.exists())

    def test_extra_source_file_during_build_refuses_new_record(self):
        def mutate(command, options):
            self.artifact.write_bytes(build.WASM_HEADER)
            (self.destination / "extra.rs").write_bytes(b"new source")

        with self.assertRaisesRegex(prepare.PreparationError, "staged source differs"):
            self.run_build(mutate)
        self.assertFalse(self.record.exists())

    def test_shared_input_change_during_build_refuses_new_record(self):
        def mutate(command, options):
            self.artifact.write_bytes(build.WASM_HEADER)
            (self.shared / "src/lib.rs").write_bytes(b"changed trusted input")

        with self.assertRaisesRegex(prepare.PreparationError, "input hash"):
            self.run_build(mutate)
        self.assertFalse(self.record.exists())

    def test_archive_change_during_build_refuses_new_record(self):
        def mutate(command, options):
            self.artifact.write_bytes(build.WASM_HEADER)
            self.archive.write_bytes(b"changed archive")

        with self.assertRaisesRegex(prepare.PreparationError, "archive size or hash"):
            self.run_build(mutate)
        self.assertFalse(self.record.exists())

    def test_frozen_lock_change_during_build_refuses_new_record(self):
        def mutate(command, options):
            self.artifact.write_bytes(build.WASM_HEADER)
            (self.package / "Cargo.lock").write_bytes(b"different lock")

        with self.assertRaisesRegex(build.BuildError, "inputs changed"):
            self.run_build(mutate)
        self.assertFalse(self.record.exists())

    def test_manifest_change_during_build_refuses_new_record(self):
        def mutate(command, options):
            self.artifact.write_bytes(build.WASM_HEADER)
            (self.package / "source-manifest.json").write_text("{}")

        with self.assertRaisesRegex(build.BuildError, "inputs changed"):
            self.run_build(mutate)
        self.assertFalse(self.record.exists())

    def test_output_inside_source_is_refused_before_preparation(self):
        self.output = self.destination / "build-output"
        with self.assertRaisesRegex(build.BuildError, "outside the prepared source"):
            self.run_build()
        self.assertFalse(self.commands)
        self.assertFalse(self.destination.exists())

    def test_source_inside_builder_owned_directory_is_refused_before_preparation(self):
        for relative in ("build", "build/source", "artifact", "artifact/source"):
            with self.subTest(relative=relative):
                self.destination = self.output / relative
                with self.assertRaisesRegex(build.BuildError, "must not overlap"):
                    self.run_build()
                self.assertFalse(self.commands)
                self.assertFalse(self.destination.exists())

    def test_another_build_cannot_invalidate_the_first_builds_outputs(self):
        self.previous_outputs()
        prior_manifest = self.record.read_bytes()
        prior_artifact = self.artifact.read_bytes()
        with build.output_lock(self.output):
            with self.assertRaisesRegex(build.BuildError, "another executor build"):
                self.run_build()
        self.assertFalse(self.commands)
        self.assertEqual(self.record.read_bytes(), prior_manifest)
        self.assertEqual(self.artifact.read_bytes(), prior_artifact)

    def test_symbolic_and_hard_linked_build_locks_are_refused(self):
        self.output.mkdir()
        outside = self.root / "outside-lock"
        outside.write_bytes(b"preserve")
        lock = self.output / ".build.lock"
        lock.symlink_to(outside)
        with self.assertRaisesRegex(prepare.PreparationError, "symbolic-link"):
            self.run_build()
        lock.unlink()
        lock.hardlink_to(outside)
        with self.assertRaisesRegex(build.BuildError, "unaliased regular"):
            self.run_build()
        self.assertFalse(self.commands)
        self.assertEqual(outside.read_bytes(), b"preserve")

    def test_symbolic_build_output_directories_are_refused(self):
        outside = self.root / "outside"
        outside.mkdir()
        for relative in ("", "build", "artifact"):
            with self.subTest(relative=relative):
                self.output = self.root / ("out-" + (relative or "root"))
                link = self.output / relative
                link.parent.mkdir(parents=True, exist_ok=True)
                link.symlink_to(outside, target_is_directory=True)
                with self.assertRaisesRegex(prepare.PreparationError, "symbolic-link"):
                    self.run_build()
                self.assertFalse(self.commands)
                self.assertEqual(list(outside.iterdir()), [])

    def test_symbolic_artifact_and_manifest_are_preserved_and_refused(self):
        outside = self.root / "outside.wasm"
        outside.write_bytes(build.WASM_HEADER)
        for path in (self.record, self.artifact):
            with self.subTest(path=path):
                path.parent.mkdir(parents=True, exist_ok=True)
                path.symlink_to(outside)
                with self.assertRaisesRegex(prepare.PreparationError, "symbolic-link"):
                    self.run_build()
                self.assertFalse(self.commands)
                self.assertTrue(path.is_symlink())
                self.assertEqual(outside.read_bytes(), build.WASM_HEADER)
                path.unlink()

    def test_symbolic_artifact_created_by_compiler_does_not_get_a_record(self):
        outside = self.root / "outside.wasm"
        outside.write_bytes(build.WASM_HEADER)
        with self.assertRaisesRegex(prepare.PreparationError, "symbolic-link"):
            self.run_build(lambda command, options: self.artifact.symlink_to(outside))
        self.assertFalse(self.record.exists())
        self.assertEqual(outside.read_bytes(), build.WASM_HEADER)

    def test_symbolic_manifest_created_during_build_is_preserved_and_refused(self):
        outside = self.root / "outside.json"
        outside.write_bytes(b"preserve metadata")

        def mutate(command, options):
            self.artifact.write_bytes(build.WASM_HEADER)
            self.record.symlink_to(outside)

        with self.assertRaisesRegex(prepare.PreparationError, "symbolic-link"):
            self.run_build(mutate)
        self.assertTrue(self.record.is_symlink())
        self.assertEqual(outside.read_bytes(), b"preserve metadata")

    def test_symbolic_trusted_input_files_are_refused_before_compilation(self):
        for relative in ("source-manifest.json", "Cargo.lock"):
            with self.subTest(relative=relative):
                original = self.package / relative
                outside = self.root / (relative + ".outside")
                original.rename(outside)
                original.symlink_to(outside)
                with self.assertRaisesRegex(prepare.PreparationError, "symbolic-link"):
                    self.run_build()
                self.assertFalse(self.commands)
                self.assertFalse(self.output.exists())
                original.unlink()
                outside.rename(original)


if __name__ == "__main__":
    unittest.main()
