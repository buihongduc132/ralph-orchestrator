#!/usr/bin/env python3
"""
Ralph Orchestrator - DEPRECATED

This Python package has been retired. Please migrate to the Rust-based version.
"""

import sys

_TOMBSTONE_MESSAGE = """
╔══════════════════════════════════════════════════════════════════════════════╗
║                    RALPH ORCHESTRATOR HAS MOVED                              ║
╠══════════════════════════════════════════════════════════════════════════════╣
║                                                                              ║
║  The Python (PyPI) version of ralph-orchestrator is no longer maintained.   ║
║  Please uninstall this package and install the new Rust-based version.      ║
║                                                                              ║
╠══════════════════════════════════════════════════════════════════════════════╣
║  UNINSTALL THIS PACKAGE                                                      ║
╠══════════════════════════════════════════════════════════════════════════════╣
║                                                                              ║
║    pip uninstall ralph-orchestrator                                          ║
║    # or: pipx uninstall ralph-orchestrator                                   ║
║                                                                              ║
╠══════════════════════════════════════════════════════════════════════════════╣
║  INSTALL THE NEW VERSION                                                     ║
╠══════════════════════════════════════════════════════════════════════════════╣
║                                                                              ║
║  Option 1: Homebrew (macOS and Linux)                                        ║
║    brew install mikeyobrien/homebrew-tap/ralph                               ║
║                                                                              ║
║  Option 2: Cargo (cross-platform, requires Rust)                             ║
║    cargo install ralph-orchestrator                                          ║
║                                                                              ║
║  Option 3: Download binary from GitHub Releases                              ║
║    https://github.com/mikeyobrien/ralph-orchestrator/releases/latest         ║
║                                                                              ║
╠══════════════════════════════════════════════════════════════════════════════╣
║  COMPATIBILITY                                                               ║
╠══════════════════════════════════════════════════════════════════════════════╣
║                                                                              ║
║  Your existing ralph.yml configuration files will continue to work with     ║
║  the new Rust version. No configuration changes are required.               ║
║                                                                              ║
╠══════════════════════════════════════════════════════════════════════════════╣
║  QUESTIONS OR ISSUES?                                                        ║
╠══════════════════════════════════════════════════════════════════════════════╣
║                                                                              ║
║  GitHub: https://github.com/mikeyobrien/ralph-orchestrator                   ║
║  Issues: https://github.com/mikeyobrien/ralph-orchestrator/issues            ║
║                                                                              ║
╚══════════════════════════════════════════════════════════════════════════════╝
"""


def main():
    """Print migration message and exit."""
    print(_TOMBSTONE_MESSAGE, file=sys.stderr)
    sys.exit(1)


if __name__ == "__main__":
    main()
