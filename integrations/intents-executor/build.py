#!/usr/bin/env python3
"""Build the pinned local executor offline and record verified build inputs."""

import argparse
from contextlib import contextmanager
import fcntl
import hashlib
import json
import os
from pathlib import Path
import stat
import subprocess
import tempfile

import prepare

PACKAGE_ROOT = Path(__file__).resolve().parent
REPOSITORY_ROOT = PACKAGE_ROOT.parents[1]
TOOLCHAIN = "1.95.0"
BUILD_MANIFEST = "build-manifest.json"
WASM_HEADER = b"\0asm\x01\0\0\0"


class BuildError(Exception):
    pass


def regular_bytes(path):
    """Read a regular input, refusing symbolic path aliases and special files."""
    path = prepare.no_symlink_ancestors(path)
    descriptor = os.open(path, os.O_RDONLY | os.O_NONBLOCK | os.O_NOFOLLOW)
    with os.fdopen(descriptor, "rb") as stream:
        if not stat.S_ISREG(os.fstat(stream.fileno()).st_mode):
            raise BuildError("build input or output must be a regular file")
        return stream.read()


def directory(path):
    path = prepare.no_symlink_ancestors(path)
    path.mkdir(parents=True, exist_ok=True)
    prepare.no_symlink_ancestors(path)
    if not stat.S_ISDIR(path.lstat().st_mode):
        raise BuildError("build output must be a directory")
    return path


def sync_directory(path):
    descriptor = os.open(path, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    try:
        os.fsync(descriptor)
    finally:
        os.close(descriptor)


def remove_regular(path):
    """Unlink only the builder's named regular output; preserve unsafe paths."""
    prepare.no_symlink_ancestors(path)
    try:
        metadata = path.lstat()
    except FileNotFoundError:
        return
    if not stat.S_ISREG(metadata.st_mode):
        raise BuildError("existing build output must be a regular file")
    path.unlink()
    sync_directory(path.parent)


def write_manifest(path, manifest):
    prepare.no_symlink_ancestors(path)
    if path.exists() and not stat.S_ISREG(path.lstat().st_mode):
        raise BuildError("build manifest must be a regular file")
    raw = (json.dumps(manifest, indent=2, sort_keys=True) + "\n").encode()
    descriptor, temporary_name = tempfile.mkstemp(prefix=".build-manifest-", dir=path.parent)
    temporary = Path(temporary_name)
    try:
        with os.fdopen(descriptor, "wb") as stream:
            stream.write(raw)
            stream.flush()
            os.fsync(stream.fileno())
        prepare.no_symlink_ancestors(path)
        if path.exists() and not stat.S_ISREG(path.lstat().st_mode):
            raise BuildError("build manifest changed during recording")
        temporary.replace(path)
        sync_directory(path.parent)
    finally:
        if temporary.exists():
            temporary.unlink()


def run_version(command, source_root, environment):
    result = subprocess.run(command, cwd=source_root, env=environment, check=True,
                            capture_output=True, text=True)
    version = result.stdout.strip()
    if not version:
        raise BuildError("required build tool did not report its version")
    return version


def overlaps(first, second):
    return first == second or first in second.parents or second in first.parents


def controlled_environment(build_directory):
    environment = os.environ.copy()
    compiler_overrides = {
        "RUSTC", "RUSTDOC", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER", "RUSTFLAGS", "RUSTDOCFLAGS",
        "CARGO_ENCODED_RUSTFLAGS", "CARGO_ENCODED_RUSTDOCFLAGS", "CARGO_INCREMENTAL", "RUSTC_BOOTSTRAP",
    }
    refused = sorted(name for name, value in environment.items() if value and (
        name in compiler_overrides or name.startswith("CARGO_PROFILE_")
        or (name.startswith("CARGO_TARGET_") and name != "CARGO_TARGET_DIR")
        or (name.startswith("CARGO_BUILD_") and name != "CARGO_BUILD_JOBS")))
    if refused:
        raise BuildError("compiler or Cargo build environment overrides refused: " + ", ".join(refused))
    environment.update({"CARGO_NET_OFFLINE": "true", "CARGO_TARGET_DIR": str(build_directory),
                        "RUSTUP_AUTO_INSTALL": "0"})
    return environment


def metadata_object(raw):
    def unique_object(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise BuildError("duplicate preparation metadata field")
            result[key] = value
        return result

    value = json.loads(raw, object_pairs_hook=unique_object)
    if not isinstance(value, dict):
        raise BuildError("preparation metadata must be a JSON object")
    return value


def verify_wasm(raw):
    """Check header and section framing; cargo-near validates compiled code."""
    if not raw.startswith(WASM_HEADER):
        raise BuildError("build artifact must be a version 1 WASM module")
    position = len(WASM_HEADER)
    while position < len(raw):
        section = raw[position]
        position += 1
        if section > 13:
            raise BuildError("build artifact has malformed WASM module sections")
        size = 0
        for index in range(5):
            if position == len(raw):
                raise BuildError("build artifact has truncated WASM module sections")
            byte = raw[position]
            position += 1
            if index == 4 and byte & 0xf0:
                raise BuildError("build artifact has malformed WASM module section length")
            size |= (byte & 0x7f) << (index * 7)
            if not byte & 0x80:
                break
        position += size
        if position > len(raw):
            raise BuildError("build artifact has truncated WASM module sections")


@contextmanager
def output_lock(output):
    lock_path = output / ".build.lock"
    prepare.no_symlink_ancestors(lock_path)
    descriptor = os.open(lock_path, os.O_RDWR | os.O_CREAT | os.O_NONBLOCK | os.O_NOFOLLOW, 0o600)
    with os.fdopen(descriptor, "r+b") as lock:
        metadata = os.fstat(lock.fileno())
        if not stat.S_ISREG(metadata.st_mode) or metadata.st_nlink != 1:
            raise BuildError("build lock must be an unaliased regular file")
        try:
            fcntl.flock(lock.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError as error:
            raise BuildError("another executor build owns this output directory") from error
        yield


def build_executor(archive_path, destination, output, *, package_root=None, shared_root=None):
    package_root = prepare.no_symlink_ancestors(package_root or PACKAGE_ROOT)
    destination = prepare.no_symlink_ancestors(destination)
    output = prepare.no_symlink_ancestors(output)
    if output == destination or destination in output.parents:
        raise BuildError("build output must be outside the prepared source tree")
    if output == package_root or package_root in output.parents:
        raise BuildError("build output must be outside the preparation package")
    for owned_directory in (output / "build", output / "artifact"):
        if any(overlaps(owned_directory, source) for source in (destination, package_root)):
            raise BuildError("builder-owned directories must not overlap preparation sources")
    environment = controlled_environment(output / "build")
    source_manifest_path = package_root / "source-manifest.json"
    frozen_lock_path = package_root / "Cargo.lock"
    try:
        source_manifest_raw = regular_bytes(source_manifest_path)
        frozen_lock_raw = regular_bytes(frozen_lock_path)
    except FileNotFoundError as error:
        raise BuildError("source manifest and frozen package Cargo.lock are required") from error
    source_manifest = metadata_object(source_manifest_raw)
    if not frozen_lock_raw:
        raise BuildError("frozen package Cargo.lock must not be empty")
    shared_root = prepare.no_symlink_ancestors(
        shared_root or REPOSITORY_ROOT / source_manifest["shared_crate_source"])
    if any(overlaps(owned, shared_root) for owned in (output / "build", output / "artifact")):
        raise BuildError("builder-owned directories must not overlap preparation sources")

    def verify_inputs():
        if (regular_bytes(source_manifest_path) != source_manifest_raw
                or regular_bytes(frozen_lock_path) != frozen_lock_raw):
            raise BuildError("build preparation inputs changed")
        prepared = prepare.prepare_source(archive_path, destination, manifest=source_manifest,
                                          package_root=package_root, shared_root=shared_root)
        if (prepared["lock_source"] != "reviewed-package-lock"
                or prepared["effective_lock_sha256"] != prepare.digest(frozen_lock_raw)):
            raise BuildError("build requires the exact frozen package Cargo.lock")
        if (regular_bytes(source_manifest_path) != source_manifest_raw
                or regular_bytes(frozen_lock_path) != frozen_lock_raw):
            raise BuildError("build preparation inputs changed")
        return metadata_object(regular_bytes(Path(prepared["input_manifest"])))

    preparation = verify_inputs()
    output = directory(output)
    # A second invocation cannot invalidate or publish the first build's output.
    with output_lock(output):
        build_directory = directory(output / "build")
        artifact_directory = directory(output / "artifact")
        manifest_path = output / BUILD_MANIFEST
        artifact_path = artifact_directory / "defuse.wasm"
        # Invalidate the old claim and remove stale WASM before invoking any tool.
        # A successful subprocess that emits nothing cannot reuse an old artifact.
        remove_regular(manifest_path)
        remove_regular(artifact_path)

        source_root = Path(destination)
        command = ["cargo", f"+{TOOLCHAIN}", "near", "build", "non-reproducible-wasm",
                   "--manifest-path", str(source_root / "contracts/defuse/Cargo.toml"),
                   "--features", "contract", "--override-toolchain", TOOLCHAIN, "--locked", "--no-abi",
                   "--out-dir", str(artifact_directory)]
        versions = {
            "cargo": run_version(["cargo", f"+{TOOLCHAIN}", "--version"], source_root, environment),
            "rustc": run_version(["rustc", f"+{TOOLCHAIN}", "--verbose", "--version"], source_root, environment),
            "cargo_near": run_version(["cargo", f"+{TOOLCHAIN}", "near", "--version"], source_root, environment),
        }
        subprocess.run(command, cwd=source_root, env=environment, check=True)
        artifact = regular_bytes(artifact_path)
        verify_wasm(artifact)
        if verify_inputs() != preparation:
            raise BuildError("prepared source binding changed during build")
        # Re-read after checking source so the manifest binds the final output bytes.
        if regular_bytes(artifact_path) != artifact:
            raise BuildError("build artifact changed during verification")
        manifest = {
            "format_version": 1,
            "source_root": str(source_root),
            "inputs": preparation["inputs"],
            "source_manifest_sha256": hashlib.sha256(source_manifest_raw).hexdigest(),
            "artifact": {"path": "artifact/defuse.wasm", "sha256": prepare.digest(artifact),
                         "bytes": len(artifact)},
            "build": {"command": command, "cwd": str(source_root), "toolchain": TOOLCHAIN,
                      "environment": {name: environment[name] for name in
                                      ("CARGO_NET_OFFLINE", "CARGO_TARGET_DIR", "RUSTUP_AUTO_INSTALL")},
                      "versions": versions, "exit_code": 0,
                      "recipe": "cargo-near-non-reproducible-wasm-locked-offline-no-abi-v1"},
            "audit_status": "NOT_INDEPENDENTLY_AUDITED",
            "deployment_source_correspondence": "UNVERIFIED",
        }
        write_manifest(manifest_path, manifest)
        return {"source_root": str(source_root), "build_manifest": str(manifest_path),
                "artifact": str(artifact_path), "artifact_sha256": manifest["artifact"]["sha256"]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--archive", required=True, type=Path)
    parser.add_argument("--destination", type=Path,
                        default=REPOSITORY_ROOT / "target/intents-executor/source")
    parser.add_argument("--output", type=Path, default=REPOSITORY_ROOT / "target/intents-executor")
    args = parser.parse_args()
    try:
        result = build_executor(args.archive, args.destination, args.output)
    except (BuildError, prepare.PreparationError, OSError, ValueError, KeyError,
            subprocess.CalledProcessError, prepare.tarfile.TarError) as error:
        parser.exit(1, f"Executor build refused: {error}\n")
    print(json.dumps(result, sort_keys=True))


if __name__ == "__main__":
    main()
