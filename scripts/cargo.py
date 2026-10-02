"""Prepare GPUI, then forward all arguments (including +toolchain) to Cargo."""

import subprocess
import sys
import tarfile

from prepare_gpui import ROOT, PreparationError, prepare


def main() -> int:
    try:
        prepare(offline="--offline" in sys.argv[1:])
        return subprocess.call(["cargo", *sys.argv[1:]], cwd=ROOT)
    except (PreparationError, OSError, ValueError, tarfile.TarError) as error:
        print(str(error), file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
