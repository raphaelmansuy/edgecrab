"""Entry point for `edgecrab` console script installed by edgecrab-cli on PyPI."""

from __future__ import annotations

import os
import sys


def main() -> None:
    """Resolve the native binary and exec it with all CLI arguments."""
    os.environ["EDGECRAB_INSTALL_METHOD"] = "pypi"
    from edgecrab_cli._binary import BINARY_VERSION, resolve
    from edgecrab_cli._version import __version__

    try:
        binary = resolve()
    except RuntimeError as exc:
        print(f"[edgecrab-cli] {exc}", file=sys.stderr)
        sys.exit(1)

    os.environ["EDGECRAB_WRAPPER_VERSION"] = __version__
    os.environ["EDGECRAB_BINARY_VERSION"] = BINARY_VERSION
    os.environ["EDGECRAB_PYTHON_EXECUTABLE"] = sys.executable
    if "pipx" in sys.executable.lower() or os.environ.get("PIPX_HOME"):
        os.environ["EDGECRAB_PYPI_INSTALLER"] = "pipx"
    else:
        os.environ["EDGECRAB_PYPI_INSTALLER"] = "pip"

    # Replace the current process with the native binary (Unix) or spawn it (Windows).
    args = [str(binary)] + sys.argv[1:]

    if os.name == "nt":
        import subprocess
        result = subprocess.run(args, env=os.environ)
        sys.exit(result.returncode)
    else:
        os.execv(str(binary), args)


if __name__ == "__main__":
    main()
