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
//!    The control for this rule counts only the manager's *production* constructions: an
//!    occurrence inside a `#[cfg(test)] mod` block (a test's expected value) does not satisfy it.
//! 3. No file aliases the enum (`use ... as`, `type X = ...DeliveryConfirmation`) or imports its
//!    variants (`use ...DeliveryConfirmation::{..}`, `::*`, or a single `::Variant`), which would
//!    let a construction hide from rules 1 and 2.
//!
//! **What counts as an occurrence.** `DeliveryConfirmation::<Variant>` anywhere, and also
//! `Self::<Variant>` in a file that defines `enum DeliveryConfirmation` (where `Self` inside an
//! `impl DeliveryConfirmation` names the enum; any other `Self::<Variant>` in that file is
//! flagged too, erring toward flagging).
//!
//! **What counts as a construction.** Comments, string contents and char-literal contents are
//! blanked first, so a variant named in a comment or a string is never counted, and a `'('` never
//! counts as a parenthesis. An occurrence in code is a *pattern*, not a construction, when:
//! - it lies inside the parentheses of a `matches!(` that opens in the same statement (since the
//!   previous `;`) and has not closed before it;
//! - scanning forward from it (stepping over a `{ .. }` or `{}` directly after it), `=>` comes
//!   before any `;`, `,`, `{`, `}` or other `=` (a match arm, including `A | B` alternatives
//!   and `if` guards); or
//! - scanning back to the previous `;`, `{` or `}`, there is a `let` with no `=` after it
//!   (`let PAT = ...`, `if let PAT = ...`, `while let PAT = ...`).
//!
//! Everything else is a construction, including a comparison operand (`x == ...::Delivered`).
//! The rule errs toward flagging: a false positive asks for a comment, a false negative hides a
//! claim. The lexer understands `//` comments, `"..."` strings with escapes and `'x'` char
//! literals; it refuses (fails the test) on `/*` block comments, raw strings (`r"`, `r#"`, and
//! the byte/C forms `br"`, `br#"`, `cr"`) and a string still open at end of file, rather than
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

/// Does a raw string literal (`r"`, `r#"`, `br"`, `br#"`, `cr"`...) start at `chars[i]`? A raw
/// identifier `r#name` does not.
fn raw_string_at(chars: &[char], i: usize) -> bool {
    if i > 0 && is_ident_char(chars[i - 1]) {
        return false;
    }
    let mut j = i;
    if matches!(chars[j], 'b' | 'c') && chars.get(j + 1) == Some(&'r') {
        j += 1;
    }
    chars[j] == 'r'
        && chars[j + 1..]
            .iter()
            .find(|&&ch| ch != '#')
            .is_some_and(|&ch| ch == '"')
}

/// Split each line into (code, comment) with comments removed from the code, and string and
/// char-literal contents replaced by spaces. String state is carried across lines. Panics on
/// constructs this lexer does not understand, and on a string still open at end of file.
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
                // `r"`, `r#"`, `br"`, `br#"`, `cr"`...: a raw string (`r#name` is fine).
                'r' | 'b' | 'c' if raw_string_at(&chars, i) => {
                    panic!("line {}: raw strings are not supported by this scan", n + 1)
                }
                '"' => {
                    in_string = true;
                    code.push(c);
                }
                '\'' => {
                    // Char literal '\x' / '\'' / '\u{..}' or 'x' (contents blanked, so '(' is
                    // not a parenthesis); otherwise a lifetime.
                    let end = if chars.get(i + 1) == Some(&'\\') {
                        (i + 3..chars.len()).find(|&j| chars[j] == '\'')
                    } else if chars.get(i + 2) == Some(&'\'') {
                        Some(i + 2)
                    } else {
                        None
                    };
                    if let Some(end) = end {
                        code.push('\'');
                        code.extend(std::iter::repeat_n(' ', end - i - 1));
                        code.push('\'');
                        i = end + 1;
                        continue;
                    }
                    code.push(c);
                }
                _ => code.push(c),
            }
            i += 1;
        }
        out.push((code, comment));
    }
    assert!(
        !in_string,
        "a string literal is still open at end of file; this scan cannot trust its lexing"
    );
    out
}

/// The code of `src` (comments and literal contents blanked) joined with `\n`, and the byte
/// offset at which each line starts.
fn joined_code(src: &str) -> (String, Vec<usize>) {
    let mut code = String::new();
    let mut line_starts = Vec::new();
    for (c, _) in split_lines(src) {
        line_starts.push(code.len());
        code.push_str(&c);
        code.push('\n');
    }
    (code, line_starts)
}

/// The 1-based line holding byte `pos` of the joined code.
fn line_of(line_starts: &[usize], pos: usize) -> usize {
    line_starts.partition_point(|&s| s <= pos)
}

/// Byte offsets of every whole-word occurrence of `word` in `code`.
fn word_positions<'a>(code: &'a str, word: &'a str) -> impl Iterator<Item = usize> + 'a {
    code.match_indices(word).map(|(i, _)| i).filter(move |&i| {
        let prev_ok = !code[..i]
            .chars()
            .next_back()
            .is_some_and(|c| is_ident_char(c) || c == '#');
        let next_ok = !code[i + word.len()..]
            .chars()
            .next()
            .is_some_and(is_ident_char);
        prev_ok && next_ok
    })
}

#[derive(Debug, PartialEq)]
struct Construction {
    /// 1-based line number.
    line: usize,
    /// Byte offset in the joined code.
    offset: usize,
}

fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Is the occurrence at byte `pos` (ending at `end`) of `code` a pattern rather than a
/// construction? See the module doc for the rule.
fn is_pattern(code: &str, pos: usize, end: usize) -> bool {
    // Inside the parentheses of a `matches!(` opened in this statement and not yet closed.
    let statement = code[..pos].rfind(';').map_or(0, |i| i + 1);
    for (m, _) in code[statement..pos].match_indices("matches!(") {
        let mut depth = 0i32;
        let mut closed = false;
        for c in code[statement + m + "matches!".len()..pos].chars() {
            match c {
                '(' => depth += 1,
                ')' => depth -= 1,
                _ => {}
            }
            if depth == 0 {
                closed = true;
                break;
            }
        }
        if !closed {
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

/// Every construction of `DeliveryConfirmation::<variant>` in `src`, and of `Self::<variant>`
/// when `src` defines `enum DeliveryConfirmation`.
fn constructions(src: &str, variant: &str) -> Vec<Construction> {
    let (code, line_starts) = joined_code(src);
    let mut needles = vec![format!("{ENUM_PATH}{variant}")];
    let defines_enum = word_positions(&code, "enum").any(|i| {
        let rest = code[i + "enum".len()..].trim_start();
        rest.strip_prefix("DeliveryConfirmation")
            .is_some_and(|r| !r.chars().next().is_some_and(is_ident_char))
    });
    if defines_enum {
        needles.push(format!("Self::{variant}"));
    }
    let mut found = Vec::new();
    for needle in &needles {
        for (pos, _) in code.match_indices(needle.as_str()) {
            let end = pos + needle.len();
            if code[end..].chars().next().is_some_and(is_ident_char) {
                continue; // a longer identifier, e.g. `DeliveredLater`
            }
            if needle.starts_with("Self")
                && code[..pos]
                    .chars()
                    .next_back()
                    .is_some_and(|c| is_ident_char(c) || c == ':')
            {
                continue; // `MySelf::` or `x::Self::`, not the enum's `Self`
            }
            if is_pattern(&code, pos, end) {
                continue;
            }
            found.push(Construction {
                line: line_of(&line_starts, pos),
                offset: pos,
            });
        }
    }
    found.sort_by_key(|c| c.offset);
    found
}

/// Byte ranges of the joined code covered by `#[cfg(test)] mod NAME { ... }` blocks.
fn test_module_ranges(code: &str) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    for (at, attr) in code.match_indices("#[cfg(test)]") {
        let rest = code[at + attr.len()..].trim_start();
        let Some(rest) = rest.strip_prefix("mod") else {
            continue;
        };
        if !rest.starts_with(char::is_whitespace) {
            continue;
        }
        let Some(open_rel) = rest.find(['{', ';']) else {
            continue;
        };
        if rest.as_bytes()[open_rel] == b';' {
            continue; // `mod tests;`: the body is another file
        }
        let open = code.len() - rest.len() + open_rel;
        let mut depth = 0i32;
        let mut close = code.len();
        for (i, c) in code[open..].char_indices() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        close = open + i;
                        break;
                    }
                }
                _ => {}
            }
        }
        ranges.push((at, close));
    }
    ranges
}

/// The constructions of `variant` in `src` outside any `#[cfg(test)] mod` block.
fn production_constructions(src: &str, variant: &str) -> Vec<Construction> {
    let (code, _) = joined_code(src);
    let tests = test_module_ranges(&code);
    constructions(src, variant)
        .into_iter()
        .filter(|c| !tests.iter().any(|&(s, e)| (s..=e).contains(&c.offset)))
        .collect()
}

/// 1-based lines on which `src` aliases the enum or imports its variants (rule 3).
fn alias_lines(src: &str) -> Vec<usize> {
    let (code, line_starts) = joined_code(src);
    let statement_at = |k: usize| {
        let end = code[k..].find(';').map_or(code.len(), |e| k + e);
        &code[k..end]
    };
    let mut lines = Vec::new();
    // `use ...DeliveryConfirmation::...` (variants: `{..}`, `*` or one) or `... as ...`.
    for k in word_positions(&code, "use") {
        let statement = statement_at(k);
        let squashed: String = statement.split_whitespace().collect();
        let spaced = statement.split_whitespace().collect::<Vec<_>>().join(" ");
        if squashed.contains("DeliveryConfirmation::")
            || spaced.contains("DeliveryConfirmation as ")
        {
            lines.push(line_of(&line_starts, k));
        }
    }
    // `type X = ...DeliveryConfirmation...`.
    for k in word_positions(&code, "type") {
        let statement = statement_at(k);
        if let Some(eq) = statement.find('=')
            && word_positions(&statement[eq..], "DeliveryConfirmation")
                .next()
                .is_some()
        {
            lines.push(line_of(&line_starts, k));
        }
    }
    lines.sort_unstable();
    lines.dedup();
    lines
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
                // The control counts production code only: a test's expected value
                // (`assert_eq!(.., Some(DeliveryConfirmation::Acknowledged))`) must not satisfy it.
                let production = production_constructions(&src, variant).len();
                match variant {
                    "Acknowledged" => manager_acknowledged += production,
                    _ => manager_expired += production,
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
    // Control: pointed at manager.rs, the finder does find the manager's own production
    // constructions (outside its `#[cfg(test)] mod`).
    assert!(
        manager_acknowledged >= 1,
        "the scan should find the manager's production Acknowledged construction, found \
         {manager_acknowledged}"
    );
    assert!(
        manager_expired >= 1,
        "the scan should find the manager's production Expired construction, found \
         {manager_expired}"
    );
}

#[test]
fn no_file_aliases_the_enum_or_its_variants() {
    let mut offenders = Vec::new();
    for path in transport_files() {
        let src = read(&path);
        for line in alias_lines(&src) {
            offenders.push(format!("{}:{line}", name(&path)));
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

/// Runs `split_lines` on `src` and returns its panic message, or `None` if it did not refuse.
fn refusal(src: &str) -> Option<String> {
    let src = src.to_string();
    std::panic::catch_unwind(move || {
        split_lines(&src);
    })
    .err()
    .map(|e| {
        e.downcast_ref::<String>()
            .cloned()
            .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
            .unwrap_or_default()
    })
}

#[test]
fn control_raw_strings_and_an_open_string_at_eof_are_refused() {
    for src in [
        "let s = r\"x\";\n",
        "let s = r#\"x\"#;\n",
        "let s = br\"x\";\n",
        "let s = br#\"x\"#;\n",
        "let s = cr\"x\";\n",
    ] {
        for text in [src.to_string(), src.replace('\n', "\r\n")] {
            let why = refusal(&text).unwrap_or_else(|| panic!("{text:?} must be refused"));
            assert!(why.contains("raw strings"), "{text:?}: {why}");
        }
    }
    for text in ["let s = \"abc;\n", "let s = \"abc;\r\n"] {
        let why = refusal(text).unwrap_or_else(|| panic!("{text:?} must be refused"));
        assert!(why.contains("end of file"), "{text:?}: {why}");
    }
    // Controls: the same lexer accepts byte strings, raw identifiers, identifiers ending in `br`,
    // a string spanning lines, and char literals including '"'.
    for ok in [
        "let s = b\"x\";\n",
        "let r#type = 1;\n",
        "let abr = 1; let x = abr;\n",
        "let s = \"a\nb\";\n",
        "let q = '\"'; let e = '\\''; let b = b'\"';\n",
    ] {
        assert_eq!(refusal(ok), None, "{ok:?} must be accepted");
        assert_eq!(refusal(&ok.replace('\n', "\r\n")), None, "{ok:?} (CRLF)");
    }
}

#[test]
fn control_self_variants_count_in_the_file_that_defines_the_enum() {
    let src = "\
pub enum DeliveryConfirmation { Sent, Delivered, Acknowledged, Expired }
impl DeliveryConfirmation {
    pub fn best() -> Self {
        Self::Acknowledged
    }
    pub fn d() -> Self { Self::Delivered }
    pub fn is_d(&self) -> bool { matches!(self, Self::Delivered) }
    pub fn e(&self) -> u8 { match self { Self::Expired => 1, _ => 0 } }
}
";
    for text in [src.to_string(), src.replace('\n', "\r\n")] {
        let ack: Vec<usize> = constructions(&text, "Acknowledged")
            .iter()
            .map(|c| c.line)
            .collect();
        assert_eq!(ack, vec![4]);
        assert_eq!(uncommented_delivered(&text), vec![6]);
        assert_eq!(constructions(&text, "Expired"), vec![]);
    }
    // Control: the same impl in a file that does not define the enum is not scanned for `Self::`.
    let elsewhere = src.replacen("pub enum DeliveryConfirmation", "pub enum Other", 1);
    assert_eq!(constructions(&elsewhere, "Acknowledged"), vec![]);
}

#[test]
fn control_aliases_and_variant_imports_are_flagged() {
    let cases: [(&str, Vec<usize>); 13] = [
        ("type DC = DeliveryConfirmation;\n", vec![1]),
        (
            "pub type DC = crate::transport::abstraction::DeliveryConfirmation;\n",
            vec![1],
        ),
        (
            "use crate::transport::abstraction::DeliveryConfirmation::Delivered;\n",
            vec![1],
        ),
        ("use x::DeliveryConfirmation::{Delivered, Sent};\n", vec![1]),
        ("use x::DeliveryConfirmation::*;\n", vec![1]),
        ("use x::DeliveryConfirmation as DC;\n", vec![1]),
        ("use x::{\n    DeliveryConfirmation as DC,\n};\n", vec![1]),
        (
            "fn f() {}\nuse x::{\n    DeliveryConfirmation::Delivered,\n};\n",
            vec![2],
        ),
        // Controls: ordinary imports, other aliases, uses and comments are not flagged.
        ("use x::{DeliveryConfirmation, DeliveryReceipt};\n", vec![]),
        ("type Receipt = DeliveryReceipt;\n", vec![]),
        ("let t = DeliveryConfirmation::Sent;\n", vec![]),
        (
            "// type X = DeliveryConfirmation\n// use x::DeliveryConfirmation::Sent;\n",
            vec![],
        ),
        ("let s = \"use x::DeliveryConfirmation::Sent;\";\n", vec![]),
    ];
    for (src, want) in cases {
        assert_eq!(alias_lines(src), want, "{src:?}");
        assert_eq!(
            alias_lines(&src.replace('\n', "\r\n")),
            want,
            "{src:?} (CRLF)"
        );
    }
}

#[test]
fn control_matches_is_scoped_to_its_own_parentheses_and_statement() {
    let cases: [(&str, Vec<usize>); 5] = [
        // A closed `matches!` earlier in the expression does not hide a later construction.
        (
            "let x = matches!(a, B) && f(DeliveryConfirmation::Delivered);\n",
            vec![1],
        ),
        // Nor does one in an earlier statement, even with a '(' char literal in it.
        (
            "let p = matches!(c, '(');\nlet d = DeliveryConfirmation::Delivered;\n",
            vec![2],
        ),
        (
            "if matches!(c, '(') { return DeliveryConfirmation::Delivered; }\n",
            vec![1],
        ),
        // Controls: inside an open `matches!(`, nested parentheses included, it is a pattern.
        (
            "let b = matches!(s, DeliveryConfirmation::Delivered);\n",
            vec![],
        ),
        (
            "let b = matches!(f(x), Some(DeliveryConfirmation::Delivered));\n",
            vec![],
        ),
    ];
    for (src, want) in cases {
        for text in [src.to_string(), src.replace('\n', "\r\n")] {
            let lines: Vec<usize> = constructions(&text, "Delivered")
                .iter()
                .map(|c| c.line)
                .collect();
            assert_eq!(lines, want, "{text:?}");
        }
    }
}

#[test]
fn control_the_manager_control_counts_production_code_only() {
    let src = "\
fn ack(entry: &mut Entry) {
    entry.status = DeliveryConfirmation::Acknowledged;
}

#[cfg(test)]
mod tests {
    #[test]
    fn t() {
        assert_eq!(s, Some(DeliveryConfirmation::Acknowledged));
    }
}
";
    for text in [src.to_string(), src.replace('\n', "\r\n")] {
        assert_eq!(constructions(&text, "Acknowledged").len(), 2);
        let prod: Vec<usize> = production_constructions(&text, "Acknowledged")
            .iter()
            .map(|c| c.line)
            .collect();
        assert_eq!(prod, vec![2]);
        // With the production line changed, only the test's assertion remains, and it does not
        // count.
        let mutated = text.replacen(
            "entry.status = DeliveryConfirmation::Acknowledged",
            "entry.status = DeliveryConfirmation::Sent",
            1,
        );
        assert_eq!(constructions(&mutated, "Acknowledged").len(), 1);
        assert_eq!(production_constructions(&mutated, "Acknowledged"), vec![]);
    }
}
