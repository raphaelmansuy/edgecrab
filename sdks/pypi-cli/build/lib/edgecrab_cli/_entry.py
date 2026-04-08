"""Entry point for `edgecrab` console script installed by edgecrab-cli on PyPI."""

from __future__ import annotations

import os
import sys


def main() -> None:
    """Resolve the native binary and exec it with all CLI arguments."""
    from edgecrab_cli._binary import resolve

    try:
        binary = resolve()
    except RuntimeError as exc:
        print(f"[edgecrab-cli] {exc}", file=sys.stderr)
        sys.exit(1)

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
