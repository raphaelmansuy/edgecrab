"""
Binary downloader and resolver for edgecrab-cli.

Downloads the correct pre-built Rust binary for the current platform from
GitHub Releases on first use, caches it in the package directory, and
provides a resolve() helper to get the absolute path.
"""

from __future__ import annotations

import platform
import shutil
import stat
import sys
import tarfile
import tempfile
import zipfile
from pathlib import Path

import httpx

from edgecrab_cli._version import __version__

REPO = "raphaelmansuy/edgecrab"
# BINARY_VERSION controls which GitHub Release tag is used for binary downloads.
# It is intentionally decoupled from __version__ so the package can be patched
# independently of binary releases.
BINARY_VERSION = "0.1.1"

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

    if dest.exists():
        return dest

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

    First checks for a system-wide native `edgecrab` on PATH (e.g. installed
    via cargo or brew).  Skips any Python wrapper scripts (like the one
    installed by this very package) to avoid infinite-exec loops.
    """
    system_binary = shutil.which("edgecrab")
    if system_binary and _is_native_binary(system_binary):
        return Path(system_binary)
    return ensure_binary()
