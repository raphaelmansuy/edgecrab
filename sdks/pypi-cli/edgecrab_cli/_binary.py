"""
Binary downloader and resolver for edgecrab-cli.

Downloads the correct pre-built Rust binary for the current platform from
GitHub Releases on first use, caches it in the package directory, and
provides a resolve() helper to get the absolute path.
"""

from __future__ import annotations

import os
import platform
import re
import shutil
import stat
import subprocess
import sys
import tarfile
import tempfile
import zipfile
from pathlib import Path

import httpx

from edgecrab_cli._version import __version__

REPO = "raphaelmansuy/edgecrab"
# First principle: the wrapper package version is the binary release version.
# Release automation changes edgecrab_cli._version.__version__ only.
BINARY_VERSION = __version__

# ── Platform → asset name mapping ────────────────────────────────────────────
_PLATFORM_MAP: dict[tuple[str, str], str] = {
    ("darwin",  "arm64"):   "edgecrab-aarch64-apple-darwin.tar.gz",
    ("darwin",  "x86_64"):  "edgecrab-x86_64-apple-darwin.tar.gz",
    ("linux",   "x86_64"):  "edgecrab-x86_64-unknown-linux-gnu.tar.gz",
    ("linux",   "aarch64"): "edgecrab-aarch64-unknown-linux-gnu.tar.gz",
    ("linux",   "arm64"):   "edgecrab-aarch64-unknown-linux-gnu.tar.gz",
    ("windows", "amd64"):   "edgecrab-x86_64-pc-windows-msvc.zip",
    ("windows", "x86_64"):  "edgecrab-x86_64-pc-windows-msvc.zip",
}

_BIN_NAME = "edgecrab.exe" if sys.platform == "win32" else "edgecrab"

# Cache binary alongside this package
_CACHE_DIR = Path(__file__).parent / "_bin"
_VERSION_RE = re.compile(r"\bedgecrab\s+([0-9]+\.[0-9]+\.[0-9]+)\b", re.IGNORECASE)


def _asset_name() -> str:
    system  = sys.platform.lower()
    machine = platform.machine().lower()
    if system.startswith("darwin"):
        key = ("darwin", machine)
    elif system.startswith("linux"):
        key = ("linux", machine)
    elif system.startswith("win"):
        key = ("windows", machine)
    else:
        key = (system, machine)
    asset = _PLATFORM_MAP.get(key)
    if not asset:
        raise RuntimeError(
            f"Unsupported platform: {system}/{machine}. "
            f"Please install from source: cargo install edgecrab-cli"
        )
    return asset


def _download(url: str, dest: Path) -> None:
    print(f"[edgecrab-cli] Downloading binary from {url} …", file=sys.stderr)
    with httpx.stream("GET", url, follow_redirects=True) as resp:
        resp.raise_for_status()
        with open(dest, "wb") as fh:
            for chunk in resp.iter_bytes(chunk_size=65536):
                fh.write(chunk)


def _extract(archive: Path, target_dir: Path) -> None:
    name = archive.name
    if name.endswith(".tar.gz") or name.endswith(".tgz"):
        with tarfile.open(archive, "r:gz") as tf:
            # Extract only the binary (may be at root or in a subdirectory)
            for member in tf.getmembers():
                if Path(member.name).name == _BIN_NAME:
                    member.name = _BIN_NAME  # flatten
                    tf.extract(member, path=target_dir)
                    return
            # Fallback: extract everything
            tf.extractall(path=target_dir)
    elif name.endswith(".zip"):
        with zipfile.ZipFile(archive) as zf:
            for info in zf.infolist():
                if Path(info.filename).name == _BIN_NAME:
                    info.filename = _BIN_NAME  # flatten
                    zf.extract(info, path=target_dir)
                    return
            zf.extractall(path=target_dir)
    else:
        raise RuntimeError(f"Unknown archive format: {name}")


def ensure_binary() -> Path:
    """Return the path to the edgecrab binary, downloading it if necessary."""
    _CACHE_DIR.mkdir(parents=True, exist_ok=True)
    dest = _CACHE_DIR / _BIN_NAME

    if dest.exists() and _binary_version(dest) == BINARY_VERSION:
        return dest
    if dest.exists():
        print(
            f"[edgecrab-cli] Replacing cached binary {dest} with version {BINARY_VERSION}",
            file=sys.stderr,
        )
        dest.unlink(missing_ok=True)

    asset = _asset_name()
    url = f"https://github.com/{REPO}/releases/download/v{BINARY_VERSION}/{asset}"

    tmp_suffix = ".tar.gz" if asset.endswith(".tar.gz") else ".zip"
    with tempfile.NamedTemporaryFile(suffix=tmp_suffix, delete=False) as tmp:
        tmp_path = Path(tmp.name)

    try:
        _download(url, tmp_path)
        _extract(tmp_path, _CACHE_DIR)
    finally:
        tmp_path.unlink(missing_ok=True)

    if not dest.exists():
        raise RuntimeError(
            f"Binary {_BIN_NAME} not found in extracted archive. "
            f"Please report this at https://github.com/{REPO}/issues"
        )

    # Ensure executable
    dest.chmod(dest.stat().st_mode | stat.S_IEXEC | stat.S_IXGRP | stat.S_IXOTH)
    print(f"[edgecrab-cli] Installed to: {dest}", file=sys.stderr)
    return dest


def _binary_version(path: Path) -> str | None:
    """Return the native binary's semantic version, or None if unreadable."""
    try:
        env = os.environ.copy()
        env.setdefault("EDGECRAB_INSTALL_METHOD", "pypi")
        env.setdefault("EDGECRAB_WRAPPER_VERSION", BINARY_VERSION)
        env.setdefault("EDGECRAB_BINARY_VERSION", BINARY_VERSION)
        result = subprocess.run(
            [str(path), "--version"],
            capture_output=True,
            check=False,
            env=env,
            text=True,
            timeout=5,
        )
    except (OSError, subprocess.SubprocessError):
        return None

    output = f"{result.stdout}\n{result.stderr}"
    match = _VERSION_RE.search(output)
    return match.group(1) if match else None


def _is_native_binary(path: str) -> bool:
    """Return True only if *path* is a native executable, not a script wrapper."""
    try:
        with open(path, "rb") as fh:
            magic = fh.read(4)
        # ELF (Linux), Mach-O 64-bit LE (macOS arm64/x86_64), PE (Windows)
        return magic[:4] in (
            b"\x7fELF",           # Linux ELF
            b"\xcf\xfa\xed\xfe",  # Mach-O 64-bit LE
            b"\xce\xfa\xed\xfe",  # Mach-O 32-bit LE
            b"\xca\xfe\xba\xbe",  # Mach-O fat binary
            b"MZ\x90\x00",        # PE (Windows .exe)
        ) or magic[:2] == b"MZ"   # PE short header
    except OSError:
        return False


def resolve() -> Path:
    """
    Return the path to the edgecrab binary.

    By default, the package-managed binary is authoritative so upgrades cannot
    be shadowed by an older system install lingering on PATH.

    Set `EDGECRAB_USE_SYSTEM_BINARY=1` to opt into a system-wide native
    `edgecrab`, but only when its version matches the wrapper package version.
    """
    if os.environ.get("EDGECRAB_USE_SYSTEM_BINARY") in {"1", "true", "TRUE", "yes", "YES"}:
        system_binary = shutil.which("edgecrab")
        if system_binary and _is_native_binary(system_binary):
            system_path = Path(system_binary)
            if _binary_version(system_path) == BINARY_VERSION:
                return system_path
    return ensure_binary()
