//! Plan rule-namespace discriminator (B1).
//!
//! The cache-runtime overhaul plan declares its own `H1–H20` rule
//! namespace. The older `R<n>` namespace belongs to
//! `.claude/skills/type-cache-architecture/SKILL.md`.
//!
//! Plan-level `R<n>` references must take one of three recognised
//! allowlist forms — anywhere else in the plan, a bare `R<n>` token
//! mixes the two namespaces and trips this test:
//!
//!   1. **H↔R table row** — a line between the `| Plan H# |
//!      Skill R# | Semantic |` header and the table's closing
//!      blank line. Inside that table, the `R<n>` token IS the
//!      mapping and needs no prefix.
//!   2. **`skill R<n>` (or `corresponds to skill R<n>`) prose** —
//!      a line that explicitly tags the `R<n>` token as a skill
//!      reference. The `skill ` token must appear in a short
//!      pre-context window (32 bytes) before the `R<n>` token.
//!   3. **`#### Owning-doc updates` section** — a line inside an
//!      `#### Owning-doc updates` subsection that cites a skill
//!      rule by its canonical identifier. Per the plan's own
//!      paragraph at line ~111: "Per-block changes that touch the
//!      skill list the section update under their `#### Owning-doc
//!      updates` subsection and continue to cite skill rules by
//!      their canonical `R<n>` identifier."
//!
//! Discriminating fixture: a synthetic bare `R20` outside the
//! allowlist MUST trip the test; `R20` inside the H↔R table, after
//! `corresponds to skill`, or under `#### Owning-doc updates` MUST
//! NOT trip.

use std::fs;
use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    let manifest = env!("CARGO_MANIFEST_DIR");
    PathBuf::from(manifest)
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from(manifest))
}

/// The closed set of `R<n>` tokens this test scans. The plan
/// references several rules in this set; tokens outside this set
/// (e.g. `R1`, `R7`, etc.) are not in scope for the namespace ban.
///
/// This is the same set the brief calls out:
/// `\bR(20|14|17|26|19|6|5|11|28)\b`.
const SCANNED_R_TOKENS: &[&str] = &["R20", "R14", "R17", "R26", "R19", "R6", "R5", "R11", "R28"];

/// True when `tok` appears in `line` at a word boundary AND the
/// pre-context contains `skill ` or `Skill ` within a short window
/// (32 bytes). Matches `corresponds to skill R<n>`, the shorter
/// `skill R<n>` form, and the sentence-start `Skill R<n>` /
/// `Skill rule R<n>` forms.
fn token_is_in_skill_citation_form(line: &str, tok: &str) -> bool {
    let bytes = line.as_bytes();
    let mut search_from = 0usize;
    while let Some(rel) = line[search_from..].find(tok) {
        let abs = search_from + rel;
        let leading_ok =
            abs == 0 || !(bytes[abs - 1].is_ascii_alphanumeric() || bytes[abs - 1] == b'_');
        let end = abs + tok.len();
        let trailing_ok =
            end == bytes.len() || !(bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_');
        if leading_ok && trailing_ok {
            // Look at the 32 bytes before the token for the
            // literal `skill ` (or `Skill ` at sentence start).
            // A short window prevents matching an earlier
            // unrelated `skill` reference that happens to live on
            // the same line.
            let window_start = abs.saturating_sub(32);
            let window = &line[window_start..abs];
            // Case-insensitive `skill` prefix in any of the four
            // recognised forms.
            if window.contains("skill ")
                || window.contains("Skill ")
                || window.contains("skill `")
                || window.contains("Skill `")
            {
                return true;
            }
        }
        search_from = abs + tok.len();
    }
    false
}

/// True when `line` contains `tok` at a Rust-identifier word
/// boundary, ignoring context.
fn line_contains_token_at_word_boundary(line: &str, tok: &str) -> bool {
    let bytes = line.as_bytes();
    let mut search_from = 0usize;
    while let Some(rel) = line[search_from..].find(tok) {
        let abs = search_from + rel;
        let leading_ok =
            abs == 0 || !(bytes[abs - 1].is_ascii_alphanumeric() || bytes[abs - 1] == b'_');
        let end = abs + tok.len();
        let trailing_ok =
            end == bytes.len() || !(bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_');
        if leading_ok && trailing_ok {
            return true;
        }
        search_from = abs + tok.len();
    }
    false
}

/// Per-line classification of the plan markdown so we can decide
/// whether a given `R<n>` occurrence sits inside the H↔R table or
/// an `#### Owning-doc updates` subsection. Both contexts are
/// derived from preceding markdown structure; per-line scanning
/// alone cannot tell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineContext {
    Other,
    HToRTable,
    OwningDocUpdates,
}

/// Walk the plan's markdown structure and return a per-line
/// classification.
///
/// - `HToRTable`: every line between the `| Plan H# |` header line
///   (inclusive) and the first blank line that follows it (the
///   table's terminator).
/// - `OwningDocUpdates`: every line under a `#### Owning-doc
///   updates` header (inclusive of the header itself), up to the
///   NEXT header line at any depth (`# `, `## `, `### `, `#### `,
///   `##### `, `###### `).
/// - `Other`: everything else.
fn classify_lines(plan: &str) -> Vec<LineContext> {
    let lines: Vec<&str> = plan.lines().collect();
    let mut out: Vec<LineContext> = vec![LineContext::Other; lines.len()];

    // Pass 1: H↔R table.
    let mut in_table = false;
    for (idx, line) in lines.iter().enumerate() {
        if !in_table {
            if line.trim_start().starts_with("| Plan H#") {
                in_table = true;
                out[idx] = LineContext::HToRTable;
            }
        } else {
            // Stay in the table until a blank line.
            if line.trim().is_empty() {
                in_table = false;
            } else {
                out[idx] = LineContext::HToRTable;
            }
        }
    }

    // Pass 2: `#### Owning-doc updates`.
    let mut in_section = false;
    for (idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("#### Owning-doc updates") {
            in_section = true;
            // Preserve any earlier classification (the H↔R table
            // shouldn't overlap with Owning-doc, but be defensive).
            if out[idx] == LineContext::Other {
                out[idx] = LineContext::OwningDocUpdates;
            }
            continue;
        }
        if in_section {
            // Any other header at any depth ends the section. We
            // detect a header line by a leading `# ` after
            // trimming.
            if trimmed.starts_with('#') {
                let after_hashes = trimmed.trim_start_matches('#');
                if after_hashes.starts_with(' ') {
                    in_section = false;
                    // Fall through — this header line itself is
                    // NOT part of the Owning-doc section.
                    continue;
                }
            }
            if out[idx] == LineContext::Other {
                out[idx] = LineContext::OwningDocUpdates;
            }
        }
    }

    out
}

#[test]
fn plan_rule_namespace_uses_h_not_r() {
    let plan_path = workspace_root().join("docs/arch/cache-runtime-overhaul-plan.md");
    let plan = fs::read_to_string(&plan_path)
        .unwrap_or_else(|err| panic!("must read plan markdown: {err}"));

    let contexts = classify_lines(&plan);
    let mut violations: Vec<(usize, String, String)> = Vec::new();
    for (idx, line) in plan.lines().enumerate() {
        // Allowlist context (1): H↔R table.
        if contexts[idx] == LineContext::HToRTable {
            continue;
        }
        // Allowlist context (3): `#### Owning-doc updates` section.
        if contexts[idx] == LineContext::OwningDocUpdates {
            continue;
        }
        for tok in SCANNED_R_TOKENS {
            if line_contains_token_at_word_boundary(line, tok)
                // Allowlist context (2): `skill R<n>` prose.
                && !token_is_in_skill_citation_form(line, tok)
            {
                violations.push((idx + 1, tok.to_string(), line.to_string()));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "plan rule-namespace ban: plan-level R<n> references must appear in one of three \
         allowlisted contexts: (1) inside the H↔R table; (2) after `skill ` (or \
         `corresponds to skill `) prose; (3) inside an `#### Owning-doc updates` \
         subsection. A bare `R<n>` anywhere else mixes the plan's `H<n>` namespace with \
         the skill's `R<n>` namespace. Either rewrite the citation to use the `skill \
         R<n>` form, or move the prose into one of the allowlisted contexts.\n\n\
         Violations:\n  {}",
        violations
            .iter()
            .map(|(line, tok, body)| format!("L{line} [{tok}]: {}", body.trim()))
            .collect::<Vec<_>>()
            .join("\n  ")
    );
}

#[test]
fn plan_rule_namespace_discriminator_rejects_synthetic_violation() {
    // F4 discriminator suite. The brief mandates that each
    // allowlist context discriminate — a bare `R20` in design prose
    // MUST trip, but `R20` inside the H↔R table, after `skill ` ,
    // or under `#### Owning-doc updates` MUST NOT trip.

    // (a) Bare `R20` in plain prose outside the allowlist trips.
    let synthetic_violation_plan = "\
Block 4 design context.

R20 sits in plain prose here.

## Next section
";
    let contexts = classify_lines(synthetic_violation_plan);
    let mut tripped = false;
    for (idx, line) in synthetic_violation_plan.lines().enumerate() {
        if contexts[idx] == LineContext::HToRTable || contexts[idx] == LineContext::OwningDocUpdates
        {
            continue;
        }
        if line_contains_token_at_word_boundary(line, "R20")
            && !token_is_in_skill_citation_form(line, "R20")
        {
            tripped = true;
        }
    }
    assert!(
        tripped,
        "self-test (a): a bare `R20` in plain design prose outside the H↔R table, \
         `skill R<n>` citation, and `#### Owning-doc updates` allowlists MUST trip \
         the guard. Without this discriminator the test could pass on a permissive \
         context check."
    );

    // (b) `R20` inside the H↔R table does NOT trip.
    let synthetic_table_plan = "\
Some intro.

| Plan H# | Skill R# | Semantic |
|---|---|---|
| H99 | R20 | synthetic mapping |

Outside the table.
";
    let contexts = classify_lines(synthetic_table_plan);
    let mut tripped = false;
    for (idx, line) in synthetic_table_plan.lines().enumerate() {
        if contexts[idx] == LineContext::HToRTable || contexts[idx] == LineContext::OwningDocUpdates
        {
            continue;
        }
        if line_contains_token_at_word_boundary(line, "R20")
            && !token_is_in_skill_citation_form(line, "R20")
        {
            tripped = true;
        }
    }
    assert!(
        !tripped,
        "self-test (b): `R20` inside the H↔R table (between the `| Plan H# |` header \
         and the trailing blank line) MUST NOT trip — the table IS a citation context."
    );

    // (c) `corresponds to skill R20` does NOT trip.
    let synthetic_citation = "Design prose: this corresponds to skill `R20` per the H↔R table.";
    assert!(
        token_is_in_skill_citation_form(synthetic_citation, "R20"),
        "self-test (c): a `corresponds to skill `R20`` reference MUST be recognised \
         as the canonical citation form."
    );

    // (c2) `skill R20` (shorter form) does NOT trip.
    let synthetic_citation_short = "See skill `R20` for the canonical statement.";
    assert!(
        token_is_in_skill_citation_form(synthetic_citation_short, "R20"),
        "self-test (c2): the shorter `skill `R20`` form MUST also be recognised."
    );

    // (d) `R20` inside `#### Owning-doc updates` does NOT trip.
    let synthetic_owning_doc_plan = "\
Block design discussion.

#### Owning-doc updates

- `.claude/skills/type-cache-architecture/SKILL.md` — update R20 section.

#### Next subsection
";
    let contexts = classify_lines(synthetic_owning_doc_plan);
    let mut tripped = false;
    for (idx, line) in synthetic_owning_doc_plan.lines().enumerate() {
        if contexts[idx] == LineContext::HToRTable || contexts[idx] == LineContext::OwningDocUpdates
        {
            continue;
        }
        if line_contains_token_at_word_boundary(line, "R20")
            && !token_is_in_skill_citation_form(line, "R20")
        {
            tripped = true;
        }
    }
    assert!(
        !tripped,
        "self-test (d): `R20` inside an `#### Owning-doc updates` subsection MUST NOT \
         trip — Owning-doc citations are the canonical form for plan-to-skill updates."
    );

    // (e) The `#### Owning-doc updates` allowlist ends at the
    // next header — a bare `R20` AFTER the section closes trips.
    let synthetic_owning_doc_end_plan = "\
Block design discussion.

#### Owning-doc updates

- Updates R20.

#### Next subsection

R20 here is bare design prose and trips.
";
    let contexts = classify_lines(synthetic_owning_doc_end_plan);
    let mut tripped = false;
    for (idx, line) in synthetic_owning_doc_end_plan.lines().enumerate() {
        if contexts[idx] == LineContext::HToRTable || contexts[idx] == LineContext::OwningDocUpdates
        {
            continue;
        }
        if line_contains_token_at_word_boundary(line, "R20")
            && !token_is_in_skill_citation_form(line, "R20")
        {
            tripped = true;
        }
    }
    assert!(
        tripped,
        "self-test (e): a bare `R20` in design prose AFTER the `#### Owning-doc \
         updates` subsection ends (i.e. after the next header) MUST trip. The \
         allowlist must not bleed past the section boundary."
    );
}
