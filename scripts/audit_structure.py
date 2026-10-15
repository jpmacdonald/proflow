#!/usr/bin/env python3
"""Check the agreed structural limits for handwritten Proflow Rust files."""

from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[1]


def line_limit(path: Path) -> int:
    """Return the limit for the responsibility represented by this file."""
    if "tests" in path.parts or path.name == "tests.rs":
        return 800
    if path.name in {"lib.rs", "main.rs"} or path.parent == Path("src/bin"):
        return 250
    if any(part in {"bin", "cli", "mcp"} for part in path.parts):
        return 350
    if path.name in {"cli.rs", "mcp.rs"}:
        return 350
    return 500


def main() -> int:
    """Report every size violation in source and tests."""
    violations = []
    for directory in (ROOT / "src", ROOT / "tests"):
        for path in sorted(directory.rglob("*.rs")):
            relative = path.relative_to(ROOT)
            size = len(path.read_text(encoding="utf-8").splitlines())
            limit = line_limit(relative)
            if size > limit:
                violations.append(f"{relative}: {size} lines exceeds {limit}")
    if violations:
        print("\n".join(violations), file=sys.stderr)
        return 1
    print("Rust source and test files are within the structural limits.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
