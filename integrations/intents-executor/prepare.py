#!/usr/bin/env python3
"""Prepare pinned executor sources without network, Cargo, or deployment actions."""

import argparse
import hashlib
import io
import json
import os
from pathlib import Path, PurePosixPath
import shutil
import stat
import tarfile
import tempfile

PACKAGE_ROOT = Path(__file__).resolve().parent
REPOSITORY_ROOT = PACKAGE_ROOT.parents[1]
MARKER = ".mandate-preparation.json"
JOURNAL_PATH = "contracts/defuse/src/contract/tokens/nep141/journal.rs"
MODULE_PATH = "contracts/defuse/src/contract/tokens/nep141/mod.rs"
SHARED_DESTINATION = "crates/mandate-withdrawal-journal"
VENDOR_SOURCE = "vendor/near-plugins-0.5.3"
VENDOR_DESTINATION = "crates/mandate-near-plugins"
VENDOR_FILES = {
    "Cargo.toml", "README.md", "src/access_control_role.rs", "src/access_controllable.rs",
    "src/events.rs", "src/lib.rs", "src/ownable.rs", "src/pausable.rs", "src/upgradable.rs",
}


class PreparationError(Exception):
    pass


def digest(raw):
    return hashlib.sha256(raw).hexdigest()


def safe_relative(name):
    if not name or name.startswith("/") or "\\" in name or "\0" in name:
        raise PreparationError("unsafe archive path")
    if len(name) >= 2 and name[1] == ":":
        raise PreparationError("unsafe archive path")
    if any(part in (".", "..") for part in name.split("/")):
        raise PreparationError("unsafe archive path")
    path = PurePosixPath(name)
    if not path.parts:
        raise PreparationError("unsafe archive path")
    return path


def no_symlink_ancestors(path):
    absolute = Path(os.path.abspath(path))
    for item in (absolute, *absolute.parents):
        if item.is_symlink():
            raise PreparationError("symbolic-link input or destination refused")
    return absolute


def verified_bytes(path, expected):
    path = no_symlink_ancestors(path)
    if not path.is_file():
        raise PreparationError("required preparation input is missing")
    raw = path.read_bytes()
    if digest(raw) != expected:
        raise PreparationError("preparation input hash does not match")
    return raw


def replace_once(text, old, new):
    if text.count(old) != 1:
        raise PreparationError("verified source recipe does not match")
    return text.replace(old, new, 1)


def verified_vendor(package_root, specification):
    if (specification["name"] != "near-plugins" or specification["version"] != "0.5.3"
            or specification["source"] != VENDOR_SOURCE
            or specification["destination"] != VENDOR_DESTINATION
            or set(specification["original_files"]) != VENDOR_FILES
            or set(specification["staged_files"]) != VENDOR_FILES):
        raise PreparationError("unsupported vendor selection")
    files = {}
    for name, expected in specification["original_files"].items():
        contents = verified_bytes(package_root / VENDOR_SOURCE / name, expected)
        if name == "Cargo.toml":
            for section in ("dependencies", "dev-dependencies"):
                old = f'[{section}.near-sdk]\nversion = "5.2"\n'.encode()
                contents = replace_once(contents, old, old + b"default-features = false\n")
        if digest(contents) != specification["staged_files"][name]:
            raise PreparationError("staged vendor hash does not match")
        files[name] = contents
    return files


def extract_verified_archive(raw, staging, manifest):
    upstream = manifest["upstream"]
    prefix = upstream["archive_root"]
    links, regular_count, unpacked_bytes = [], 0, 0
    with tarfile.open(fileobj=io.BytesIO(raw), mode="r:gz") as archive:
        entries, seen = [], set()
        for member in archive.getmembers():
            path = safe_relative(member.name)
            if path.parts[0] != prefix:
                raise PreparationError("unexpected archive root")
            relative = PurePosixPath(*path.parts[1:])
            if len(path.parts) == 1:
                if not member.isdir():
                    raise PreparationError("invalid archive root entry")
                continue
            key = relative.as_posix()
            if key in seen:
                raise PreparationError("duplicate archive path")
            seen.add(key)
            if member.issym() or member.islnk():
                links.append({"path": key, "target": member.linkname,
                              "type": "symbolic" if member.issym() else "hard"})
                continue
            if not (member.isfile() or member.isdir()) or member.issparse():
                raise PreparationError("unsupported archive member type")
            if member.isfile():
                regular_count += 1
                unpacked_bytes += member.size
            entries.append((member, relative))
        if sorted(links, key=lambda x: x["path"]) != sorted(manifest["skipped_links"], key=lambda x: x["path"]):
            raise PreparationError("archive links do not match reviewed inputs")
        if regular_count != upstream["regular_files"] or unpacked_bytes != upstream["unpacked_file_bytes"]:
            raise PreparationError("archive contents do not match reviewed inputs")
        for member, relative in entries:
            destination = staging.joinpath(*relative.parts)
            if member.isdir():
                destination.mkdir(parents=True, exist_ok=True)
                continue
            destination.parent.mkdir(parents=True, exist_ok=True)
            stream = archive.extractfile(member)
            if stream is None:
                raise PreparationError("archive file contents are missing")
            raw_file = stream.read()
            if len(raw_file) != member.size:
                raise PreparationError("archive file length does not match")
            with destination.open("xb") as output:
                output.write(raw_file)
            destination.chmod(member.mode & 0o777)


def tree_files(root):
    files = {}
    for directory, dirs, names in os.walk(root, followlinks=False):
        for name in dirs + names:
            path = Path(directory) / name
            if path.is_symlink():
                raise PreparationError("staged symbolic link refused")
        for name in names:
            path = Path(directory) / name
            relative = path.relative_to(root).as_posix()
            if relative == MARKER:
                continue
            if not path.is_file():
                raise PreparationError("staged special file refused")
            raw = path.read_bytes()
            files[relative] = {"sha256": digest(raw), "bytes": len(raw),
                               "mode": stat.S_IMODE(path.stat().st_mode)}
    return dict(sorted(files.items()))


def write_marker(root, marker):
    raw = (json.dumps(marker, indent=2, sort_keys=True) + "\n").encode()
    temporary = root / (MARKER + ".tmp")
    with temporary.open("xb") as stream:
        stream.write(raw)
    temporary.replace(root / MARKER)


def prepare_source(archive_path, destination, *, manifest=None, package_root=None, shared_root=None):
    package_root = Path(package_root or PACKAGE_ROOT)
    if manifest is None:
        manifest = json.loads((package_root / "source-manifest.json").read_text())
    if manifest["format_version"] != 1 or manifest["recipe"] != "native-nep141-journal-overlay-v1":
        raise PreparationError("unsupported preparation manifest")
    if set(manifest["overlay_files"]) != {JOURNAL_PATH} or manifest["shared_crate_destination"] != SHARED_DESTINATION:
        raise PreparationError("unsupported source overlay selection")
    shared_root = Path(shared_root or REPOSITORY_ROOT / manifest["shared_crate_source"])
    archive_path = no_symlink_ancestors(archive_path)
    if not archive_path.is_file():
        raise PreparationError("archive must be a regular file")
    raw = archive_path.read_bytes()
    upstream = manifest["upstream"]
    if len(raw) != upstream["archive_bytes"] or digest(raw) != upstream["archive_sha256"]:
        raise PreparationError("archive size or hash does not match")
    overlay = verified_bytes(package_root / "overlay" / JOURNAL_PATH, manifest["overlay_files"][JOURNAL_PATH])
    shared = {}
    for name, expected in manifest["shared_crate_files"].items():
        relative = safe_relative(name)
        shared[name] = verified_bytes(shared_root.joinpath(*relative.parts), expected)
    vendor = verified_vendor(package_root, manifest["vendor_crate"])
    frozen_lock = no_symlink_ancestors(package_root / "Cargo.lock")
    if frozen_lock.exists() and not frozen_lock.is_file():
        raise PreparationError("frozen lock must be a regular file")
    reviewed_lock = frozen_lock.read_bytes() if frozen_lock.is_file() else None
    destination = no_symlink_ancestors(destination)
    if destination.exists() and (not destination.is_dir() or not (destination / MARKER).is_file()):
        raise PreparationError("existing unverified destination refused")
    destination.parent.mkdir(parents=True, exist_ok=True)
    staging = Path(tempfile.mkdtemp(prefix=".intents-preparation-", dir=destination.parent))
    try:
        extract_verified_archive(raw, staging, manifest)
        for name, expected in manifest["baseline_files"].items():
            verified_bytes(staging.joinpath(*safe_relative(name).parts), expected)
        if any((staging / path).exists() for path in (JOURNAL_PATH, SHARED_DESTINATION, VENDOR_DESTINATION)):
            raise PreparationError("overlay would replace unexpected upstream files")
        (staging / JOURNAL_PATH).write_bytes(overlay)
        (staging / JOURNAL_PATH).chmod(0o644)
        module = staging / MODULE_PATH
        module.write_text(module.read_text() + "mod journal;\n")
        workspace = staging / "Cargo.toml"
        text = replace_once(workspace.read_text(), '  "contracts/defuse",\n', '  "crates/mandate-withdrawal-journal",\n  "crates/mandate-near-plugins",\n  "contracts/defuse",\n')
        text = replace_once(text, "[workspace.dependencies]\n", '[workspace.dependencies]\nmandate-withdrawal-journal = { path = "crates/mandate-withdrawal-journal" }\n')
        text = replace_once(text, 'near-sdk = "5.30.0"\n', 'near-sdk = { version = "5.30.0", default-features = false }\n')
        if "[patch.crates-io]" in text:
            raise PreparationError("unexpected existing crates.io patch section")
        text += '\n[patch.crates-io]\nnear-plugins = { path = "crates/mandate-near-plugins" }\n'
        workspace.write_text(text)
        defuse = staging / "contracts/defuse/Cargo.toml"
        text = replace_once(defuse.read_text(), "[dependencies]\n", "[dependencies]\nmandate-withdrawal-journal.workspace = true\n")
        text = replace_once(text, 'defuse = { path = ".", features = ["abi"] }\n', 'defuse = { path = ".", features = ["contract"] }\n')
        text = replace_once(text, 'near-sdk = { workspace = true, features = ["non-contract-usage"] }\n', 'near-sdk = { workspace = true, features = ["non-contract-usage", "unit-testing"] }\n')
        defuse.write_text(text)
        for name, contents in shared.items():
            path = staging / SHARED_DESTINATION / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(contents)
            path.chmod(0o644)
        for name, contents in vendor.items():
            path = staging / VENDOR_DESTINATION / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(contents)
            path.chmod(0o644)
        if reviewed_lock is not None:
            (staging / "Cargo.lock").write_bytes(reviewed_lock)
        lock_hash = digest((staging / "Cargo.lock").read_bytes())
        inputs = {
            "recipe": manifest["recipe"], "archive_commit": upstream["commit"],
            "archive_sha256": upstream["archive_sha256"], "archive_bytes": len(raw),
            "overlay_files": manifest["overlay_files"], "shared_crate_files": manifest["shared_crate_files"],
            "vendor_crate": manifest["vendor_crate"],
            "lock_source": "reviewed-package-lock" if reviewed_lock is not None else "original-upstream-lock",
            "effective_lock_sha256": lock_hash,
        }
        expected_marker = {"format_version": 1, "inputs": inputs, "files": tree_files(staging)}
        reused = destination.exists()
        if reused:
            marker_path = destination / MARKER
            no_symlink_ancestors(marker_path)
            actual_marker = json.loads(marker_path.read_text())
            if tree_files(destination) != expected_marker["files"]:
                raise PreparationError("staged source differs from reviewed preparation inputs")
            # A root-reviewed frozen lock may replace the initial generated lock.
            # Only marker fields tied to that exact lock may transition; no source is overwritten.
            prior = json.loads(json.dumps(actual_marker))
            desired = json.loads(json.dumps(expected_marker))
            for item in (prior, desired):
                item.get("inputs", {}).pop("lock_source", None)
                item.get("inputs", {}).pop("effective_lock_sha256", None)
                item.get("files", {}).pop("Cargo.lock", None)
            if prior != desired:
                raise PreparationError("preparation input manifest does not match")
            if actual_marker != expected_marker:
                if reviewed_lock is None:
                    raise PreparationError("unreviewed lock transition refused")
                write_marker(destination, expected_marker)
        else:
            write_marker(staging, expected_marker)
            destination.mkdir(exist_ok=False)
            reservation = destination.stat()
            if destination.stat().st_ino != reservation.st_ino:
                raise PreparationError("destination changed during preparation")
            try:
                staging.rename(destination)
            except Exception:
                if destination.exists() and destination.stat().st_ino == reservation.st_ino:
                    try:
                        destination.rmdir()
                    except OSError:
                        pass
                raise
        return {"source_root": str(destination), "input_manifest": str(destination / MARKER),
                "effective_lock_sha256": lock_hash, "lock_source": inputs["lock_source"], "reused": reused}
    finally:
        if staging.exists():
            shutil.rmtree(staging)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--archive", required=True, type=Path)
    parser.add_argument("--destination", type=Path, default=REPOSITORY_ROOT / "target/intents-executor/source")
    args = parser.parse_args()
    try:
        result = prepare_source(args.archive, args.destination)
    except (PreparationError, OSError, ValueError, KeyError, tarfile.TarError) as error:
        parser.exit(1, f"Preparation refused: {error}\n")
    print(json.dumps(result, sort_keys=True))


if __name__ == "__main__":
    main()
