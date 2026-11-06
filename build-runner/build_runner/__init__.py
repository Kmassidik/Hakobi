"""Standalone, rootless-capable image build runner for Hakobi.

The runner is deliberately separate from the Rust control plane: it accepts a
small, typed HTTP API, clones a repository into an isolated temporary
workspace, and invokes ``git``/``docker``/``nixpacks`` with argv arrays.
"""

from __future__ import annotations

__all__ = ["__version__"]

__version__ = "0.1.0"
