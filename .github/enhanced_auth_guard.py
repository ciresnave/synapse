#!/usr/bin/env python3
# SPDX-License-Identifier: MIT OR Apache-2.0
"""`enhanced-auth` must keep failing to compile, for the SAME recorded reason.

    python3 .github/enhanced_auth_guard.py            # check
    python3 .github/enhanced_auth_guard.py --self-test

⚠️⚠️ RED FROM THIS GATE IS GOOD NEWS, NOT A REGRESSION. That is backwards from
every other gate in this pipeline, and it is deliberate -- read the whole
docstring before touching this file, especially before "fixing" a red run.

## Why this exists

`src/auth_integration_enhanced.rs` (gated behind the `enhanced-auth` feature)
is written against an `auth-framework` API that has never been published --
see `CAPABILITY_INVENTORY.md` §5.1/§5.2. It is deliberately left in-tree,
gated, rather than deleted, because its fate (repaired, re-gated harder, or
removed) is a Synapse/FAM merge decision, not this pipeline's to make.
`COMPILATION_FIXES_COMPLETE.md` asserts `cargo check --all-features
--all-targets` succeeds; it does not, and never has since `enhanced-auth`
was wired up. See `docs/superpowers/..` history / GitHub issue #5 for the
full chain that established this.

The claim in that document was unfalsifiable by this repository's own CI:
nothing here ran `--all-features`, so the false claim could never turn red
on its own. This gate is a DETECTOR for that gap -- not a fix for the module
(CireSnave's standing FAM ruling makes that his call, not this pipeline's),
and not a fix for the document (the document is corrected separately, in the
same commit that adds this gate, using the same in-place-correction pattern
`SECURITY_AUDIT_COMPLETION_REPORT.md` already uses elsewhere in this repo).

## What "pass" and "fail" mean here, stated explicitly because they invert

- **PASS (exit 0, green)**: `--all-features --all-targets` still fails to
  compile, and the failure still involves `auth_integration_enhanced.rs`
  with (at least) the same class of rustc error codes recorded below. This
  is the EXPECTED, DOCUMENTED, DELIBERATE state. Green here means "nothing
  has changed about a known, disclosed gap."
- **FAIL -- GOOD NEWS (exit 1)**: the build now SUCCEEDS. This is not a
  regression to chase -- it means `enhanced-auth` (or its dependency,
  `auth-framework`) was fixed, updated, or the module was changed to no
  longer need the missing API. `COMPILATION_FIXES_COMPLETE.md`'s claim may
  now be true. Update the document and this script's RECORDED_ERROR_CODES
  (or delete this gate) before merging whatever fixed it.
- **FAIL -- UNRECORDED REASON (exit 1)**: the build still fails, but NOT for
  the recorded reason -- the observed error codes for
  `auth_integration_enhanced.rs` don't match what's on record below, or the
  file isn't implicated in the failure at all. ⚠️ THIS IS THE FAILURE MODE
  THIS GATE EXISTS TO CATCH: a build that keeps failing for a reason nobody
  wrote down is indistinguishable from one failing for the recorded reason
  unless something checks WHICH errors occurred, not just THAT it failed. A
  gate that only asked "did it fail" would stay green forever no matter what
  actually broke -- "a number standing in for a set cannot tell two sets
  apart" applies here exactly the way it does to a bare test count.

## Why this matches on rustc error CODES, not message text

Full diagnostic messages (and their wording) move between toolchain
versions; `E0432`/`E0599`/`E0308` are stable, documented identifiers
(`rustc --explain E0432`) that do not reword themselves. Matching on codes
survives a `rustup update` that this gate's author never ran; matching on a
frozen message string would not.

## What this refuses to do, and why

* It never patches or deletes `auth_integration_enhanced.rs`, `Cargo.toml`,
  or any source file. A gate that repairs the thing it measures reports
  success forever and proves nothing about whether the disclosed gap is
  still there. Un-gating `enhanced-auth` so this passes trivially is
  CireSnave's call (FAM merge scope), not this script's.
* It never treats "the build failed" alone as sufficient. See "UNRECORDED
  REASON" above.
* It refuses to pass on an EMPTY diagnostic stream (`cargo check` produced no
  JSON at all) -- that is a broken invocation, a missing toolchain, or a
  `cargo` version emitting a different message-format shape, not a clean
  build, and treating it as a pass would be the SPDX gate's "0 of 0 files"
  trap wearing this gate's clothes.
"""

from __future__ import annotations

import json
import subprocess  # noqa: S404 - fixed argv, no shell, no caller input
import sys

# The module this gate watches. A compiler diagnostic "implicates" this file
# if any of its spans name a path ending in this string.
WATCHED_FILE = "auth_integration_enhanced.rs"

# The rustc error codes this module's failure actually produces, freshly
# measured at `main` `87fa701` (2026-09-27) with `cargo check --all-features
# --all-targets --message-format=json`: 14 error diagnostics, ALL of them
# attributed to WATCHED_FILE, spanning exactly these five codes --
# E0432 (nine unresolved imports), E0599 (a nonexistent method,
# `AuthConfig::enable_audit_logging`, and a nonexistent enum variant,
# `Credential::enhanced_device_flow`), E0308 (mismatched types from the
# API-shape drift), E0560 (`SecureMessage` has no field named `signature` --
# a NEWER divergence than GitHub issue #5 recorded, most likely from this
# same portfolio's router-merge work reshaping `SecureMessage`'s fields since
# that issue was filed), and E0382 (a partial-move error downstream of the
# type mismatches). This is a MEASURED FLOOR, not the complete or permanent
# set: a build that produces even one of these five codes on WATCHED_FILE is
# still failing in the same broad family this gate exists to distinguish
# from a genuinely different defect. A build that fails WITHOUT any of them
# attributed to WATCHED_FILE is failing for an unrecorded reason -- see the
# module docstring's "UNRECORDED REASON" case. If a future measurement adds
# or drops codes here, say so in the commit message with the ref and date,
# the same way this comment does.
RECORDED_ERROR_CODES = frozenset({"E0308", "E0382", "E0432", "E0560", "E0599"})

CARGO_CMD = [
    "cargo",
    "check",
    "--all-features",
    "--all-targets",
    "--message-format=json",
]


def run_cargo_check() -> tuple[int, list[dict]]:
    """Run the documented command, parse its JSON diagnostics.

    Returns (exit_code, list of compiler-message error diagnostics). Never
    raises on a non-zero exit -- a non-zero exit is the expected case this
    gate exists to classify, not an error in running the gate itself.
    """
    proc = subprocess.run(
        CARGO_CMD,
        capture_output=True,
        text=True,
        check=False,
    )
    diagnostics = []
    for line in proc.stdout.splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            obj = json.loads(line)
        except json.JSONDecodeError:
            # cargo's own non-JSON progress lines interleave on some
            # versions; only `--message-format=json` lines matter here.
            continue
        if obj.get("reason") != "compiler-message":
            continue
        message = obj.get("message", {})
        if message.get("level") != "error":
            continue
        diagnostics.append(message)
    return proc.returncode, diagnostics


def codes_for_watched_file(diagnostics: list[dict]) -> set[str]:
    """The distinct error codes among diagnostics whose spans name WATCHED_FILE."""
    codes: set[str] = set()
    for message in diagnostics:
        spans = message.get("spans", [])
        implicated = any(
            span.get("file_name", "").endswith(WATCHED_FILE) for span in spans
        )
        if not implicated:
            continue
        code = message.get("code")
        if code and code.get("code"):
            codes.add(code["code"])
    return codes


def classify(exit_code: int, diagnostics: list[dict]) -> tuple[bool, str]:
    """Returns (passed, message). `passed` is True only for the documented,
    deliberate failure state -- see the module docstring."""
    if exit_code == 0:
        return False, (
            "\n".join(
                [
                    "=" * 78,
                    "FAIL -- GOOD NEWS, NOT A REGRESSION.",
                    "=" * 78,
                    "`cargo check --all-features --all-targets` now SUCCEEDS.",
                    "",
                    f"That means {WATCHED_FILE} (behind the `enhanced-auth` feature)",
                    "now compiles -- something got fixed, updated, or removed.",
                    "COMPILATION_FIXES_COMPLETE.md's claim that this command succeeds",
                    "may now be TRUE. Before merging whatever caused this:",
                    "  1. Update COMPILATION_FIXES_COMPLETE.md's correction to say so.",
                    "  2. Update or delete RECORDED_ERROR_CODES / this gate --",
                    "     it has nothing left to detect once the gap it watches closes.",
                    "  3. Update CAPABILITY_INVENTORY.md §5.1/§5.2.",
                    "This is NOT synapse breaking. Do not revert whatever fixed it.",
                ]
            )
        )

    if not diagnostics:
        return False, (
            "\n".join(
                [
                    "=" * 78,
                    "FAIL -- EMPTY DIAGNOSTIC STREAM.",
                    "=" * 78,
                    f"`{' '.join(CARGO_CMD)}` exited {exit_code} but produced no",
                    "parseable compiler-message error diagnostics at all. That is a",
                    "broken invocation (missing toolchain, changed `cargo` JSON shape,",
                    "wrong working directory) -- not the documented enhanced-auth",
                    "failure. Investigate the raw cargo output; do not treat this as",
                    "'still broken, as expected'.",
                ]
            )
        )

    found = codes_for_watched_file(diagnostics)
    if found & RECORDED_ERROR_CODES:
        return True, (
            "\n".join(
                [
                    f"PASS (this means '{WATCHED_FILE}' is still known-broken, as recorded).",
                    f"Recorded codes seen: {sorted(found & RECORDED_ERROR_CODES)}",
                    "See CAPABILITY_INVENTORY.md §5.1/§5.2 and",
                    "COMPILATION_FIXES_COMPLETE.md's correction for the full context.",
                ]
            )
        )

    return False, (
        "\n".join(
            [
                "=" * 78,
                "FAIL -- UNRECORDED REASON. THIS IS THE CASE THIS GATE EXISTS TO CATCH.",
                "=" * 78,
                f"`cargo check --all-features --all-targets` failed (exit {exit_code}),",
                f"but not for the recorded reason. Codes attributed to {WATCHED_FILE}: "
                f"{sorted(found) or 'none -- the file was not implicated at all'}.",
                f"Recorded (expected) codes: {sorted(RECORDED_ERROR_CODES)}.",
                "",
                "This may be a NEW, DIFFERENT defect unrelated to the documented",
                "auth-framework API gap -- do not assume it is the known issue and",
                "wave it through. Investigate the actual diagnostics before deciding",
                "whether this is a genuine regression or a reason to update",
                "RECORDED_ERROR_CODES.",
            ]
        )
    )


# ---------------------------------------------------------------------------
# Self-test: the gate's own logic, checked against synthetic diagnostics, so
# this file is never merged having been seen only to pass. No real `cargo`
# invocation here -- that would make the self-test as slow as the real gate
# and unable to run offline.
# ---------------------------------------------------------------------------


def _diag(code: str, file_name: str) -> dict:
    return {
        "level": "error",
        "code": {"code": code},
        "spans": [{"file_name": file_name}],
    }


def self_test() -> None:
    # Good news: build succeeds.
    passed, msg = classify(0, [])
    assert not passed, "exit 0 must never pass -- that is the good-news case"
    assert "GOOD NEWS" in msg

    # Empty diagnostics on a nonzero exit: broken invocation, not a pass.
    passed, msg = classify(101, [])
    assert not passed, "a nonzero exit with zero diagnostics must not pass"
    assert "EMPTY DIAGNOSTIC STREAM" in msg

    # The documented, expected failure: recorded codes, on the watched file.
    diagnostics = [
        _diag("E0432", "src/auth_integration_enhanced.rs"),
        _diag("E0599", "src/auth_integration_enhanced.rs"),
        _diag("E0308", "src/auth_integration_enhanced.rs"),
    ]
    passed, msg = classify(101, diagnostics)
    assert passed, f"the documented failure shape must pass, got: {msg}"

    # Only a SUBSET of recorded codes present is still a pass -- the codes are
    # a floor ("at least this"), not an exact-set match, so a toolchain that
    # stops reporting one redundant code (E0308 downstream of the E0432s,
    # say) does not spuriously fail this gate.
    passed, _ = classify(101, [_diag("E0432", "src/auth_integration_enhanced.rs")])
    assert passed, "a subset of recorded codes on the watched file must still pass"

    # Fails, but the watched file isn't implicated at all: unrecorded reason.
    passed, msg = classify(101, [_diag("E0432", "src/some_other_module.rs")])
    assert not passed, "an unimplicated watched file must not pass"
    assert "UNRECORDED REASON" in msg

    # Fails, watched file implicated, but with a code that isn't on record:
    # unrecorded reason. This is the exact "different failure, same file"
    # case the module docstring calls out as this gate's whole purpose.
    passed, msg = classify(101, [_diag("E9999", "src/auth_integration_enhanced.rs")])
    assert not passed, "a different error code on the same file must not pass"
    assert "UNRECORDED REASON" in msg

    print("self-test: 6/6 classify() cases correct")


def main() -> int:
    if "--self-test" in sys.argv:
        self_test()
        return 0

    exit_code, diagnostics = run_cargo_check()
    passed, message = classify(exit_code, diagnostics)
    print(message)
    return 0 if passed else 1


if __name__ == "__main__":
    sys.exit(main())
