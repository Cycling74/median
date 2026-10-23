#!/usr/bin/env python3
"""Regenerate every supported target from the pinned max-sdk-base, on macOS."""

import argparse
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--windows-include", required=True, type=Path,
        help="MinGW-w64 x86_64 include directory (containing windows.h)",
    )
    args = parser.parse_args()
    if sys.platform != "darwin":
        parser.error("run on macOS with Xcode/Command Line Tools and MinGW-w64 installed")
    include = args.windows_include.resolve()
    if not (include / "windows.h").is_file():
        parser.error("--windows-include must contain windows.h")
    root = Path(__file__).resolve().parent.parent
    crate = root / "max-sys"
    targets = [
        ("aarch64-apple-darwin", "ffi-macos-aarch64.rs"),
        ("x86_64-apple-darwin", "ffi-macos-x86_64.rs"),
        ("x86_64-pc-windows-gnu", "ffi-windows-x86_64.rs"),
    ]
    with tempfile.TemporaryDirectory(prefix="median-bindings-") as tmp:
        for target, _ in targets:
            env = os.environ.copy()
            env.update(MAX_SYS_BINDINGS_TARGET=target, MAX_SYS_BINDINGS_DIR=tmp,
                       MAX_SYS_WINDOWS_INCLUDE=str(include))
            subprocess.run(["cargo", "build", "--features", "build-bindings"],
                           cwd=crate, env=env, check=True)
        for _, name in targets:
            if not (Path(tmp) / name).is_file():
                raise RuntimeError(f"generator did not produce {name}")
        for _, name in targets:
            shutil.copyfile(Path(tmp) / name, crate / "src" / name)
    print("Regenerated macOS ARM64, macOS x64 and Windows x64. Review and commit all three together.")


if __name__ == "__main__":
    main()
