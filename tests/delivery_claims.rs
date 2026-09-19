// SPDX-License-Identifier: MIT OR Apache-2.0
//! Transport contract, Task 6 (spec §4): no transport claims a delivery it did not observe.
//!
//! A source scan over every file in `src/transport/`. The rules it enforces:
//!
//! 1. Every **construction** of `DeliveryConfirmation::Delivered` has a `//` comment containing
//!    `protocol event:` followed by the event's name, on the same line or within the three lines
//!    before it. (A test double writes `protocol event: none -- ...` and says why.)
//! 2. No file other than `manager.rs` constructs `DeliveryConfirmation::Acknowledged` or
//!    `DeliveryConfirmation::Expired`; those are the manager's claims, never a transport's.
//! 3. No file aliases the enum or glob-imports its variants, which would let a construction hide
//!    from rules 1 and 2.
//!
//! **What counts as a construction.** Comments and string contents are blanked first, so a
//! variant named in a comment or a string is never counted. An occurrence in code is a
//! *pattern*, not a construction, when:
//! - it lies inside an unclosed `matches!(`;
//! - scanning forward from it (stepping over a `{ .. }` or `{}` directly after it), `=>` comes
//!   before any `;`, `,`, `{`, `}` or other `=` (a match arm, including `A | B` alternatives
//!   and `if` guards); or
//! - scanning back to the previous `;`, `{` or `}`, there is a `let` with no `=` after it
//!   (`let PAT = ...`, `if let PAT = ...`, `while let PAT = ...`).
//!
//! Everything else is a construction, including a comparison operand (`x == ...::Delivered`).
//! The rule errs toward flagging: a false positive asks for a comment, a false negative hides a
//! claim. The lexer understands `//` comments, `"..."` strings with escapes and `'x'` char
//! literals; it refuses (fails the test) on `/*` block comments and raw strings rather than
//! mis-scan them.

use std::path::{Path, PathBuf};

const ENUM_PATH: &str = "DeliveryConfirmation::";
const EVENT_MARKER: &str = "protocol event:";
const COMMENT_WINDOW: usize = 3;

/// The files under `src/transport/`, enumerated from the git index (`git ls-files`), per the
/// portfolio rule that enumeration comes from the index rather than a disk walk. If git is not
/// available (a packaged crate), it falls back to `read_dir` of the directory and says so.
fn transport_files() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let git = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["ls-files", "--", "src/transport"])
        .output();
    let mut files: Vec<PathBuf> = match git {
        Ok(out) if out.status.success() => {
            println!("enumerated src/transport via git ls-files");
            String::from_utf8(out.stdout)
                .expect("git ls-files output is UTF-8")
                .lines()
                .filter(|l| l.ends_with(".rs"))
                .map(|l| root.join(l))
                .collect()
        }
        other => {
            println!("git ls-files unavailable ({other:?}); FALLBACK: read_dir of src/transport");
            std::fs::read_dir(root.join("src/transport"))
                .expect("src/transport exists")
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "rs"))
                .collect()
        }
    };
    files.sort();
    assert!(
        files.iter().any(|p| p.ends_with("manager.rs")),
        "the enumeration must include manager.rs (control), got {files:?}"
    );
    files
}

/// Split each line into (code, comment) with comments removed from the code. String state is
/// carried across lines. Panics on constructs this lexer does not understand.
fn split_lines(src: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut in_string = false;
    for (n, line) in src.lines().enumerate() {
        // `str::lines` strips "\r\n" as well as "\n"; strip a stray '\r' defensively.
        let line = line.strip_suffix('\r').unwrap_or(line);
        let chars: Vec<char> = line.chars().collect();
        let mut code = String::new();
        let mut comment = String::new();
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            if in_string {
                // String contents are blanked, so a variant named in a string is not code.
                if c == '\\' && i + 1 < chars.len() {
                    code.push_str("  ");
                    i += 2;
                    continue;
                }
                if c == '"' {
                    in_string = false;
                    code.push(c);
                } else {
                    code.push(' ');
                }
                i += 1;
                continue;
            }
            match c {
                '/' if chars.get(i + 1) == Some(&'/') => {
                    comment = chars[i..].iter().collect();
                    break;
                }
                '/' if chars.get(i + 1) == Some(&'*') => {
                    panic!(
                        "line {}: block comments are not supported by this scan",
                        n + 1
                    )
                }
                // `r"`, `r#"`, `r##"`...: a raw string (a raw identifier `r#name` is fine).
                'r' if (i == 0 || !is_ident_char(chars[i - 1]))
                    && chars[i + 1..]
                        .iter()
                        .find(|&&ch| ch != '#')
                        .is_some_and(|&ch| ch == '"') =>
                {
                    panic!("line {}: raw strings are not supported by this scan", n + 1)
                }
                '"' => {
                    in_string = true;
                    code.push(c);
                }
                '\'' => {
                    // Char literal 'x' or '\x' / '\u{..}'; otherwise a lifetime.
                    if chars.get(i + 2) == Some(&'\'') {
                        code.extend(&chars[i..i + 3]);
                        i += 3;
                        continue;
                    }
                    if chars.get(i + 1) == Some(&'\\') {
                        let end = (i + 2..chars.len()).find(|&j| chars[j] == '\'');
                        if let Some(end) = end {
                            code.extend(&chars[i..=end]);
                            i = end + 1;
                            continue;
                        }
                    }
                    code.push(c);
                }
                _ => code.push(c),
            }
            i += 1;
        }
        out.push((code, comment));
    }
    out
}

#[derive(Debug, PartialEq)]
struct Construction {
    /// 1-based line number.
    line: usize,
}

fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Is the occurrence at byte `pos` (ending at `end`) of `code` a pattern rather than a
/// construction? See the module doc for the rule.
fn is_pattern(code: &str, pos: usize, end: usize) -> bool {
    // Inside an unclosed `matches!(`.
    if let Some(m) = code[..pos].rfind("matches!(") {
        let depth: i32 = code[m + "matches!".len()..pos]
            .chars()
            .map(|c| match c {
                '(' => 1,
                ')' => -1,
                _ => 0,
            })
            .sum();
        if depth > 0 {
            return true;
        }
    }
    // Forward: `=>` before any expression terminator means a match arm. The variants are unit
    // variants, so a brace group straight after one can only be the pattern `{ .. }` or `{}`;
    // step over exactly those.
    let mut rest = &code[end..];
    let trimmed = rest.trim_start();
    if let Some(inner) = trimmed.strip_prefix('{')
        && let Some(close) = inner.find('}')
        && matches!(inner[..close].trim(), "" | "..")
    {
        rest = &inner[close + 1..];
    }
    let rest = rest.as_bytes();
    for (i, &b) in rest.iter().enumerate() {
        match b {
            b';' | b',' | b'{' | b'}' => break,
            b'=' => {
                if rest.get(i + 1) == Some(&b'>') {
                    return true;
                }
                break;
            }
            _ => {}
        }
    }
    // Backward: `let` with no `=` after it, since the previous statement boundary.
    let start = code[..pos].rfind([';', '{', '}']).map_or(0, |i| i + 1);
    let before = &code[start..pos];
    before.match_indices("let").any(|(i, _)| {
        let prev_ok = i == 0 || !before[..i].chars().next_back().is_some_and(is_ident_char);
        let next_ok = !before[i + 3..].chars().next().is_some_and(is_ident_char);
        prev_ok && next_ok && !before[i + 3..].contains('=')
    })
}

/// Every construction of `DeliveryConfirmation::<variant>` in `src`.
fn constructions(src: &str, variant: &str) -> Vec<Construction> {
    let lines = split_lines(src);
    let mut code = String::new();
    let mut line_starts = Vec::new();
    for (c, _) in &lines {
        line_starts.push(code.len());
        code.push_str(c);
        code.push('\n');
    }
    let needle = format!("{ENUM_PATH}{variant}");
    let mut found = Vec::new();
    for (pos, _) in code.match_indices(&needle) {
        let end = pos + needle.len();
        if code[end..].chars().next().is_some_and(is_ident_char) {
            continue; // a longer identifier, e.g. `DeliveredLater`
        }
        if is_pattern(&code, pos, end) {
            continue;
        }
        let line = line_starts.partition_point(|&s| s <= pos);
        found.push(Construction { line });
    }
    found
}

/// Does the construction on 1-based `line` have a `protocol event:` comment on that line or
/// within the `COMMENT_WINDOW` lines before it?
fn names_its_protocol_event(src: &str, line: usize) -> bool {
    let lines = split_lines(src);
    let first = line.saturating_sub(COMMENT_WINDOW).max(1);
    (first..=line).any(|n| {
        let comment = lines[n - 1].1.to_lowercase();
        comment
            .find(EVENT_MARKER)
            .is_some_and(|i| !comment[i + EVENT_MARKER.len()..].trim().is_empty())
    })
}

/// Every uncommented `Delivered` construction in `src`, as 1-based line numbers.
fn uncommented_delivered(src: &str) -> Vec<usize> {
    constructions(src, "Delivered")
        .into_iter()
        .map(|c| c.line)
        .filter(|&l| !names_its_protocol_event(src, l))
        .collect()
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

fn name(path: &Path) -> String {
    path.file_name().unwrap().to_string_lossy().into_owned()
}

#[test]
fn every_delivered_construction_names_its_protocol_event() {
    let files = transport_files();
    let mut total = 0;
    let mut offenders = Vec::new();
    for path in &files {
        let src = read(path);
        total += constructions(&src, "Delivered").len();
        for line in uncommented_delivered(&src) {
            offenders.push(format!("{}:{line}", name(path)));
        }
    }
    println!(
        "scanned {} files; {total} Delivered construction(s)",
        files.len()
    );
    assert!(
        offenders.is_empty(),
        "`DeliveryConfirmation::Delivered` constructed without a `{EVENT_MARKER}` comment \
         within {COMMENT_WINDOW} lines (spec §4): {offenders:?}"
    );
    // Control: the scan finds at least one construction (the test double in providers.rs), so
    // an empty offender list is not an empty population.
    assert!(
        total >= 1,
        "expected at least one Delivered construction (the mock), found {total}"
    );
}

#[test]
fn only_the_manager_constructs_acknowledged_or_expired() {
    let files = transport_files();
    let mut offenders = Vec::new();
    let mut manager_acknowledged = 0;
    let mut manager_expired = 0;
    for path in &files {
        let src = read(path);
        let is_manager = name(path) == "manager.rs";
        for variant in ["Acknowledged", "Expired"] {
            let found = constructions(&src, variant);
            if is_manager {
                match variant {
                    "Acknowledged" => manager_acknowledged += found.len(),
                    _ => manager_expired += found.len(),
                }
            } else {
                for c in found {
                    offenders.push(format!("{}:{} constructs {variant}", name(path), c.line));
                }
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "only manager.rs may construct Acknowledged or Expired (spec §4): {offenders:?}"
    );
    // Control: pointed at manager.rs, the finder does find the manager's own constructions.
    assert!(
        manager_acknowledged >= 1,
        "the scan should find the manager's Acknowledged construction, found {manager_acknowledged}"
    );
    assert!(
        manager_expired >= 1,
        "the scan should find the manager's Expired construction, found {manager_expired}"
    );
}

#[test]
fn no_file_aliases_the_enum_or_its_variants() {
    let mut offenders = Vec::new();
    for path in transport_files() {
        let src = read(&path);
        for (i, (code, _)) in split_lines(&src).iter().enumerate() {
            let squashed: String = code.split_whitespace().collect();
            if squashed.contains("DeliveryConfirmation::{")
                || squashed.contains("DeliveryConfirmation::*")
                || code.contains("DeliveryConfirmation as ")
            {
                offenders.push(format!("{}:{}", name(&path), i + 1));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "aliasing DeliveryConfirmation or importing its variants hides constructions from this \
         scan: {offenders:?}"
    );
}

// ---- Controls on the scanner itself, on synthetic sources ----

const UNCOMMENTED: &str = "fn f() -> Receipt {\n    Receipt {\n        \
    confirmation: DeliveryConfirmation::Delivered,\n    }\n}\n";

#[test]
fn control_an_uncommented_delivered_is_flagged() {
    assert_eq!(uncommented_delivered(UNCOMMENTED), vec![3]);
    // The same source with CRLF line endings is flagged on the same line.
    let crlf = UNCOMMENTED.replace('\n', "\r\n");
    assert_eq!(uncommented_delivered(&crlf), vec![3]);
}

#[test]
fn control_a_commented_delivered_passes_and_the_window_is_three_lines() {
    let ok =
        "// protocol event: HTTP 2xx from the peer\n\n\nlet c = DeliveryConfirmation::Delivered;\n";
    assert_eq!(constructions(ok, "Delivered").len(), 1);
    assert!(uncommented_delivered(ok).is_empty());
    assert!(uncommented_delivered(&ok.replace('\n', "\r\n")).is_empty());
    // Four lines back is outside the window.
    let far = "// protocol event: HTTP 2xx\n\n\n\nlet c = DeliveryConfirmation::Delivered;\n";
    assert_eq!(uncommented_delivered(far), vec![5]);
    // A marker with no event named does not count.
    let empty = "let c = DeliveryConfirmation::Delivered; // protocol event:\n";
    assert_eq!(uncommented_delivered(empty), vec![1]);
    // The marker inside a string is not a comment.
    let in_string =
        "let s = \"// protocol event: fake\"; let c = DeliveryConfirmation::Delivered;\n";
    assert_eq!(uncommented_delivered(in_string), vec![1]);
}

#[test]
fn control_patterns_and_comments_are_not_constructions() {
    let patterns = "\
match s {
    DeliveryConfirmation::Delivered => 1,
    DeliveryConfirmation::Delivered { .. } => 1,
    Some(DeliveryConfirmation::Sent | DeliveryConfirmation::Delivered) if expired => 2,
    DeliveryConfirmation::Sent
    | DeliveryConfirmation::Delivered => 3,
}
let b = matches!(s, DeliveryConfirmation::Delivered);
if let Some(DeliveryConfirmation::Delivered) = x { }
while let DeliveryConfirmation::Delivered = next() { }
// a comment naming DeliveryConfirmation::Delivered
let t = \"DeliveryConfirmation::Delivered is only a string\";
";
    assert_eq!(constructions(patterns, "Delivered"), vec![]);
    // Control: constructions in the same shapes of code are found.
    let built = "\
let c = DeliveryConfirmation::Delivered;
x => DeliveryConfirmation::Delivered,
return Receipt { confirmation: DeliveryConfirmation::Delivered };
let d = if ok { DeliveryConfirmation::Delivered } else { DeliveryConfirmation::Sent };
";
    let lines: Vec<usize> = constructions(built, "Delivered")
        .iter()
        .map(|c| c.line)
        .collect();
    assert_eq!(lines, vec![1, 2, 3, 4]);
}
