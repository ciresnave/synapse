# SPDX-License-Identifier: MIT OR Apache-2.0
"""Measure how many method names called in a doc's ```rust blocks exist nowhere in the source.

Usage: python .github/docs_staleness.py [--min-rate 0.5]

For each tracked doc (`README.md` and `docs/*.md`, minus the changelog and `docs/superpowers/`),
collect the names called as `.name(` or `::name(` inside ```rust fenced blocks, drop a small list of
std/ubiquitous names, and count those that are not defined by any `fn name` in a tracked `.rs` file
under `src/` or `crates/`.

This is an UPPER BOUND on staleness: a name from an external crate (`builder`, `from_secs`) counts as
unknown. It is a screen for pages worth a look, not a verdict on any single call. Files are
enumerated from the index (`git ls-files`), never by walking the disk.
"""

import argparse
import collections
import re
import subprocess
import sys

IGNORED = set(
    "new clone unwrap expect await map iter collect len push insert get contains to_string into "
    "from as_str is_empty join send recv lock write read open close start stop run build default "
    "parse trim split format println display debug info warn error ok err some none cloned take "
    "any all filter find first last next sort min max with timeout sleep spawn".split()
)


def tracked() -> list[str]:
    out = subprocess.run(
        ["git", "ls-files"], capture_output=True, text=True, encoding="utf-8", check=True
    ).stdout
    return out.splitlines()


def read(path: str) -> str:
    with open(path, encoding="utf-8", errors="replace") as handle:
        return handle.read()


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--min-rate", type=float, default=0.0)
    args = parser.parse_args()

    files = tracked()
    sources = [f for f in files if f.endswith(".rs") and f.startswith(("src/", "crates/"))]
    defined: set[str] = set()
    for path in sources:
        defined |= set(re.findall(r"\bfn\s+([A-Za-z_][A-Za-z0-9_]*)", read(path)))
    docs = ["README.md"] + [
        f
        for f in files
        if f.startswith("docs/")
        and f.endswith(".md")
        and "superpowers" not in f
        and not f.endswith("CHANGELOG.md")
    ]
    print(f"sources: {len(sources)} .rs files, {len(defined)} distinct fn names; docs: {len(docs)}")
    print(f"{'doc':46s} {'names':>5s} {'unknown':>7s} {'rate':>5s}  examples")
    for doc in docs:
        code = "\n".join(re.findall(r"```rust\n(.*?)```", read(doc), re.S))
        calls = collections.Counter(re.findall(r"(?:\.|::)([a-z_][a-z0-9_]*)\s*\(", code))
        names = [n for n in calls if n not in IGNORED]
        unknown = sorted(n for n in names if n not in defined)
        rate = len(unknown) / len(names) if names else 0.0
        if rate >= args.min_rate and names:
            print(f"{doc:46s} {len(names):5d} {len(unknown):7d} {rate:5.2f}  {', '.join(unknown[:4])}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
