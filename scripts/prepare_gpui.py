"""Prepare pinned GPUI sources without changing Cargo's shared registry cache."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import uuid
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
CACHE = ROOT / ".cache"
DESTINATION = CACHE / "gpui"
PATCHES = ROOT / "patches" / "gpui"


class PreparationError(Exception):
    """An actionable build preparation failure."""


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def require_cache_path(path: Path) -> None:
    resolved = path.resolve()
    if not CACHE.resolve().is_relative_to(ROOT) or not resolved.is_relative_to(CACHE.resolve()):
        raise PreparationError(f"GPUI_CACHE_OUTSIDE_WORKSPACE: {path}")
    if resolved == CACHE.resolve():
        raise PreparationError("GPUI_CACHE_ROOT_OPERATION_REFUSED")


def tree_hashes(directory: Path) -> dict[str, str]:
    files = {}
    for path in sorted(directory.rglob("*")):
        if path.is_symlink():
            raise PreparationError(f"GPUI_CACHE_SYMLINK: {path}")
        if path.is_file() and path != directory / ".prepared.json":
            files[path.relative_to(directory).as_posix()] = digest(path)
    return files


def source_archive(source: dict, offline: bool) -> Path:
    name, version, expected = source["name"], source["version"], source["sha256"]
    if any(not part or any(c not in "abcdefghijklmnopqrstuvwxyz0123456789.-" for c in part)
           for part in (name, version)):
        raise PreparationError("GPUI_INVALID_SOURCE_NAME")
    filename = f"{name}-{version}.crate"
    cargo_home = Path(os.environ.get("CARGO_HOME", Path.home() / ".cargo"))
    candidates = sorted((cargo_home / "registry" / "cache").glob(f"*/{filename}"))
    downloaded = CACHE / "gpui-archives" / filename
    candidates.append(downloaded)
    for candidate in candidates:
        if candidate.is_file() and digest(candidate) == expected:
            return candidate
    if offline:
        raise PreparationError(f"GPUI_ARCHIVE_MISSING_OR_HASH_MISMATCH: {filename}; run preparation without --offline once")
    require_cache_path(downloaded)
    downloaded.parent.mkdir(parents=True, exist_ok=True)
    url = f"https://static.crates.io/crates/{name}/{filename}"
    with urllib.request.urlopen(url, timeout=60) as response:
        archive_bytes = response.read()
    if hashlib.sha256(archive_bytes).hexdigest() != expected:
        raise PreparationError(f"GPUI_SOURCE_HASH_MISMATCH: {filename}")
    downloaded.write_bytes(archive_bytes)
    return downloaded


def apply_patch(staging: Path, patch: Path) -> None:
    command = ["git", "-C", str(ROOT), "apply", "--whitespace=error",
               f"--directory={staging.relative_to(ROOT).as_posix()}"]
    for arguments in (["--check"], []):
        result = subprocess.run([*command, *arguments, str(patch)], capture_output=True, text=True)
        if result.returncode:
            raise PreparationError(f"GPUI_PATCH_FAILED: {patch.name}\n{result.stderr.strip()}")


def prepare(offline: bool = False) -> None:
    if sys.version_info < (3, 12):
        raise PreparationError("GPUI_PYTHON_VERSION: Python 3.12 or newer is required")
    require_cache_path(DESTINATION)
    CACHE.mkdir(parents=True, exist_ok=True)
    manifest = json.loads((PATCHES / "manifest.json").read_text(encoding="utf-8"))
    if manifest["schema"] != 1:
        raise PreparationError("GPUI_MANIFEST_SCHEMA_UNSUPPORTED")
    patch_files = []
    for entry in manifest["patches"]:
        if Path(entry["file"]).name != entry["file"]:
            raise PreparationError("GPUI_PATCH_PATH_INVALID")
        patch = PATCHES / entry["file"]
        if digest(patch) != entry["sha256"]:
            raise PreparationError(f"GPUI_PATCH_HASH_MISMATCH: {patch.name}; update manifest after reviewing the patch")
        patch_files.append(patch)
    input_hash = hashlib.sha256((digest(PATCHES / "manifest.json") + digest(Path(__file__))).encode()).hexdigest()
    lock = CACHE / "gpui-prepare.lock"
    try:
        lock_handle = lock.open("x", encoding="utf-8")
    except FileExistsError as error:
        raise PreparationError("GPUI_PREPARATION_BUSY: another preparation may be running; check .cache/gpui-prepare.lock") from error
    staging = None
    try:
        with lock_handle:
            lock_handle.write(str(os.getpid()))
        stamp = DESTINATION / ".prepared.json"
        if stamp.is_file():
            try:
                saved = json.loads(stamp.read_text(encoding="utf-8"))
            except (ValueError, OSError):
                saved = {}
            if saved.get("input_hash") == input_hash and saved.get("files") == tree_hashes(DESTINATION):
                print("GPUI_PATCH_READY: verified cached sources", flush=True)
                return
        # Inherit workspace access: Python's private tempfile directory on Windows
        # can exclude the interactive user when a build runs under a sandbox account.
        # GPUI debug shaders are read later by the desktop application's user.
        staging = CACHE / f"gpui-prepare-{uuid.uuid4().hex}"
        require_cache_path(staging)
        staging.mkdir()
        for source in manifest["sources"]:
            archive = source_archive(source, offline)
            with tarfile.open(archive, "r:gz") as package:
                # Official crates are hash-pinned; reject links as an additional extraction boundary.
                for member in package.getmembers():
                    if member.issym() or member.islnk():
                        raise PreparationError("GPUI_SOURCE_ARCHIVE_LINK_REFUSED")
                package.extractall(staging, filter="data")
            extracted = staging / f"{source['name']}-{source['version']}"
            require_cache_path(extracted)
            extracted.rename(staging / source["name"])
        for patch in patch_files:
            apply_patch(staging, patch)
        stamp_data = {"input_hash": input_hash, "files": tree_hashes(staging)}
        (staging / ".prepared.json").write_text(json.dumps(stamp_data, indent=2) + "\n", encoding="utf-8")
        # Publish only after every checksum and patch succeeds. This directory is generated.
        require_cache_path(DESTINATION)
        if DESTINATION.exists():
            shutil.rmtree(DESTINATION)
        staging.rename(DESTINATION)
        staging = None
        print("GPUI_PATCH_READY: extracted and patched pinned sources", flush=True)
    finally:
        if staging is not None and staging.exists():
            require_cache_path(staging)
            shutil.rmtree(staging)
        lock.unlink(missing_ok=True)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--offline", action="store_true", help="Use only verified local crate archives")
    args = parser.parse_args()
    try:
        prepare(args.offline)
    except (PreparationError, OSError, ValueError, tarfile.TarError) as error:
        print(str(error), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
