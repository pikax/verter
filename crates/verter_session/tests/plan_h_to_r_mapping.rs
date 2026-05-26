//! Plan ↔ skill H ↔ R mapping discriminator (B1).
//!
//! The cache-runtime overhaul plan declares a new `H1–H20` rule
//! namespace and maps each H entry to the corresponding `R<n>` rule
//! in `.claude/skills/type-cache-architecture/SKILL.md`. This test
//! walks the H ↔ R cross-reference table at the head of the plan,
//! extracts each `(H<n>, R<m>)` pair, reads the corresponding `R<m>`
//! rule text from the skill, and asserts every keyword the H entry
//! depends on appears in the cited rule body.
//!
//! Pinned keyword fixture (inside this file) — updating any plan ↔
//! skill alignment requires touching this fixture alongside the
//! mapping. A synthetic remapping of `H5` → `R5 + R28` MUST fail
//! because R5/R28 do not carry the overflow / NonCacheable /
//! BudgetExceeded vocabulary that H5 declares.

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

fn read_file(rel: &str) -> String {
    let path = workspace_root().join(rel);
    fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("must read `{rel}` from workspace root: {err}"))
}

/// Pinned `(H#, R#s, required_keywords)` fixture.
///
/// Each entry asserts: when the plan maps `H<n>` to one OR more
/// `R<m>` rules, the UNION of the cited rule bodies in
/// `.claude/skills/type-cache-architecture/SKILL.md` MUST contain
/// every keyword listed here.
///
/// The fixture is internal — to remap an H entry the plan author
/// touches BOTH the plan markdown's H ↔ R table AND this fixture.
/// That dual-touch is the protection against silent semantic
/// drift between plan and skill.
///
/// Coverage rule: every non-`(new)` row in the plan's H↔R table
/// MUST appear here. The `h_to_r_fixture_covers_every_skill_mapped_row`
/// assertion at the top of `h_to_r_mapping_is_semantically_accurate`
/// enforces that — a future addition of `H15` with a skill mapping
/// is forced to add a fixture entry, not to silently slip through.
const H_TO_R_KEYWORD_FIXTURE: &[(&str, &[&str], &[&str])] = &[
    // H1 → R1 — `host.upsert` is a cache-state no-op on unchanged
    // quintuple. R1's body names `cache-state no-op` and the
    // `quintuple` explicitly.
    ("H1", &["R1"], &["cache-state no-op", "quintuple"]),
    // H2 → R2 — `upsert` means "the source changed"; cache
    // eviction is an explicit method, not an `upsert` side effect.
    ("H2", &["R2"], &["the source changed", "Cache eviction"]),
    // H3 → R6 — query-identity keys exclude `fact_dep_signature`,
    // content hashes, version hashes.
    (
        "H3",
        &["R6"],
        &["Cache keys never include", "fact_dep_signature"],
    ),
    // H4 → R3 — reverse-dependent cache invalidation is forbidden.
    ("H4", &["R3"], &["reverse-dependent", "forbidden"]),
    // H5 → R20 + R31 — empty signatures and overflowed signatures
    // are different cacheable states. R20's body explicitly names
    // `Overflow` + `NonCacheable` for the admission contract; R31
    // names `BudgetExceeded` in its degraded-result list. The
    // fixture pins the exact-case casing the skill text uses so
    // case-sensitive `.contains()` is the check.
    (
        "H5",
        &["R20", "R31"],
        &["Overflow", "NonCacheable", "BudgetExceeded"],
    ),
    // H6 → R8 — only final per-owner payloads
    // (`ComponentMetaResultDb`) are owner-keyed; slot key is
    // content-free, version info lives on the candidate.
    (
        "H6",
        &["R8"],
        &["per-owner payloads", "ComponentMetaResultDb"],
    ),
    // H7 → R9 — reuse is the default; recomputation is the
    // exception.
    (
        "H7",
        &["R9"],
        &["Reuse is the default", "recomputation is the exception"],
    ),
    // H8 → R10 — facts use stable `FactKey`s; removed facts
    // validate as misses (per-key invalidation, not vector indices).
    ("H8", &["R10"], &["stable `FactKey`", "Removed facts"]),
    // H9 → R11 — binding-naming facts carry `SymbolSpace ∈ {Type,
    // Value, Namespace}`.
    (
        "H9",
        &["R11"],
        &["SymbolSpace", "Type", "Value", "Namespace"],
    ),
    // H10 → R12 — parse-domain vs resolve-domain fact separation.
    (
        "H10",
        &["R12"],
        &["Parse-domain facts", "Resolve-domain facts"],
    ),
    // H11 → R17 — overlay/base separation; `SessionView` never
    // mutates the host; byte-identical overlay collapses to base
    // hash. (The skill text wraps `Byte-identical` onto a separate
    // line from `overlay collapses`, so the keyword fixture matches
    // each half independently.)
    (
        "H11",
        &["R17"],
        &[
            "Sessions are views",
            "Byte-identical",
            "collapses to base hash",
        ],
    ),
    // H12 → R18 — `SessionView` is passed explicitly through
    // `ResolverContext`.
    (
        "H12",
        &["R18"],
        &["`SessionView` is passed explicitly", "ResolverContext"],
    ),
    // H13 → R20 — multi-candidate storage isolates concurrent
    // overlay variants.
    (
        "H13",
        &["R20"],
        &["Multi-candidate", "concurrent overlay variants"],
    ),
    // H14 → R19 + R26 — singleflight required for every cold
    // cacheable node, validated against the ValidatedFactCache
    // substrate.
    (
        "H14",
        &["R19", "R26"],
        &["ValidatedFactCache", "concurrency oracle"],
    ),
];

/// Parse rule bodies out of the skill markdown. A rule body starts at
/// the first occurrence of `**R<n>.**` (or `**R<n>` followed by
/// `<space|paren-stuff>.**`) and runs up to the FIRST of:
///   - the next `**R<m+1>` (any next R rule), OR
///   - the next `^## ` markdown section header.
///
/// Bounding by `^## ` prevents the LAST rule (R31) from absorbing
/// all post-rule SKILL.md content (the `## Cache layer key
/// composition` section, the `## Two-phase emission map` section,
/// etc.). Without that bound, R31's body would carry every later
/// section's keywords and the H↔R `union.contains(keyword)` check
/// could pass on a spurious match in unrelated trailing prose.
///
/// Defence-in-depth: every extracted `R<n>` body MUST START with
/// the literal `**R<n>` token. A misaligned parser whose first
/// matched line is NOT a rule header would silently glue an
/// adjacent rule's body into the wrong key; the start-check panics
/// loudly instead.
///
/// Header shapes the parser accepts:
///   - `**R5.** Caches divide ...`
///   - `**R14 (path-precise).** ...`
///   - `**R29 (Module augmentation).** ...`
///   - `**R31 (Exact policy identity and complete-result admission).** ...`
///
/// We do NOT require the `.**` to immediately follow the numeric
/// run — `**R<digits>` followed by either `.` (canonical form) or
/// `<whitespace>` (parenthetical-suffix form) qualifies. The
/// terminator we look for is the first `.**` after the header
/// numeric run, but for body extraction we only need to identify
/// the line index of each header, not the exact end of the header
/// text.
fn parse_rule_bodies(skill_md: &str) -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    let lines: Vec<&str> = skill_md.lines().collect();
    let mut headers: Vec<(String, usize)> = Vec::new();
    for (idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("**R") {
            // Read the numeric run.
            let bytes = rest.as_bytes();
            let mut end = 0usize;
            while end < bytes.len() && bytes[end].is_ascii_digit() {
                end += 1;
            }
            if end == 0 {
                continue;
            }
            // After the numeric run, either `.` (canonical) or
            // ` ` (parenthetical-suffix) qualifies.
            if end >= bytes.len() {
                continue;
            }
            let next = bytes[end];
            if next == b'.' || next == b' ' {
                // Confirm the line contains `.**` SOMEWHERE after
                // the numeric run — this filters out random
                // `**Rxx` prose that doesn't open a rule header.
                let suffix = &rest[end..];
                if suffix.contains(".**") {
                    let num = &rest[..end];
                    headers.push((format!("R{num}"), idx));
                }
            }
        }
    }

    // Precompute every `^## ` section-header line index so we can
    // truncate any rule body that would otherwise spill past its
    // owning section. The skill markdown's rule definitions all
    // live under `## Cache identity & validation`, `## Cache
    // identity & validation` continuation sections, etc.; the
    // first `^## ` that follows a rule header marks the rule's
    // terminating boundary if no later rule appears before it.
    let section_header_lines: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter_map(|(idx, line)| {
            if line.starts_with("## ") {
                Some(idx)
            } else {
                None
            }
        })
        .collect();

    for window in 0..headers.len() {
        let (name, start) = headers[window].clone();
        let next_rule = headers
            .get(window + 1)
            .map(|(_, n)| *n)
            .unwrap_or(lines.len());
        let next_section = section_header_lines
            .iter()
            .copied()
            .find(|n| *n > start)
            .unwrap_or(lines.len());
        let end = std::cmp::min(next_rule, next_section);
        let body = lines[start..end].join("\n");

        // Defence-in-depth: the body's first line MUST start with
        // `**R<n>` for the rule we just extracted. A misaligned
        // parser (e.g. a `**R<n>` lookalike that survived the
        // shape filter above) would store wrong-keyed content
        // under the right key; this assertion panics loudly.
        let expected_prefix = format!("**{name}");
        let first_line = body.lines().next().unwrap_or("").trim_start();
        assert!(
            first_line.starts_with(&expected_prefix),
            "parse_rule_bodies: rule body for `{name}` MUST start with `{expected_prefix}` \
             (defence-in-depth assertion against parser misalignment). Got first line: \
             `{first_line}`."
        );
        out.insert(name, body);
    }
    out
}

/// Parse the H↔R table in the plan and return a map H<n> → Vec<R<m>>.
///
/// The table is delimited by the two markdown table lines
/// `| Plan H# | Skill R# | Semantic |` and `|---|---|---|` and
/// continues until the next blank line.
fn parse_h_to_r_mapping(plan_md: &str) -> std::collections::HashMap<String, Vec<String>> {
    let mut out = std::collections::HashMap::new();
    let mut in_table = false;
    for line in plan_md.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("| Plan H#") {
            in_table = true;
            continue;
        }
        if in_table {
            if trimmed.is_empty() {
                in_table = false;
                continue;
            }
            if trimmed.starts_with("|---") {
                continue;
            }
            if !trimmed.starts_with("| H") {
                continue;
            }
            // Cells are separated by `|`.
            let cells: Vec<&str> = trimmed.split('|').map(|c| c.trim()).collect();
            // Expected: `["", "H<n>", "R<m> [+ R<m2> ...] [(new)]", "<semantic>", ""]`
            if cells.len() < 4 {
                continue;
            }
            let h = cells[1].to_string();
            let r_cell = cells[2];
            let mut r_codes: Vec<String> = Vec::new();
            for token in r_cell.split_whitespace() {
                let t = token.trim_start_matches('R');
                if t.chars().all(|c| c.is_ascii_digit()) && !t.is_empty() {
                    r_codes.push(format!("R{t}"));
                }
            }
            out.insert(h, r_codes);
        }
    }
    out
}

#[test]
fn h_to_r_mapping_is_semantically_accurate() {
    let plan_md = read_file("docs/arch/cache-runtime-overhaul-plan.md");
    let skill_md = read_file(".claude/skills/type-cache-architecture/SKILL.md");

    let mapping = parse_h_to_r_mapping(&plan_md);
    let rule_bodies = parse_rule_bodies(&skill_md);

    assert!(
        !mapping.is_empty(),
        "must extract at least one H↔R row from the plan's table"
    );
    assert!(
        !rule_bodies.is_empty(),
        "must extract at least one R<n> rule body from the skill"
    );

    // Coverage gate: every non-`(new)` row in the plan's H↔R table
    // MUST appear in `H_TO_R_KEYWORD_FIXTURE`. A future addition of
    // a skill-mapped H row WITHOUT a fixture entry fails here, not
    // silently slips through.
    let fixture_h_ids: std::collections::HashSet<&str> =
        H_TO_R_KEYWORD_FIXTURE.iter().map(|(h, _, _)| *h).collect();
    let mut missing_from_fixture: Vec<String> = Vec::new();
    for (plan_h_id, plan_r_ids) in &mapping {
        // Skip `(new)` rows — the plan parser returns an empty
        // `r_codes` Vec for rows whose Skill column is `(new)` (no
        // `R<n>` tokens to extract). Only rows that DO carry skill
        // mappings are required to be in the fixture.
        if plan_r_ids.is_empty() {
            continue;
        }
        if !fixture_h_ids.contains(plan_h_id.as_str()) {
            missing_from_fixture.push(format!("{plan_h_id} → {plan_r_ids:?}"));
        }
    }
    missing_from_fixture.sort();
    assert!(
        missing_from_fixture.is_empty(),
        "coverage gate: every non-`(new)` row in the plan's H↔R table must have a \
         `H_TO_R_KEYWORD_FIXTURE` entry. Rows missing from the fixture:\n  {}\n\n\
         Add a `(\"{h}\", &[<R-IDs>], &[<keywords>])` entry for each missing row, \
         then re-run the test.",
        missing_from_fixture.join("\n  "),
        h = "H?"
    );

    for (h_id, expected_r_ids, keywords) in H_TO_R_KEYWORD_FIXTURE {
        // The plan must declare EXACTLY the same `R<m>` set the
        // fixture pins. A drift between the plan's row and the
        // fixture surfaces here.
        let plan_rs = mapping.get(*h_id).unwrap_or_else(|| {
            panic!(
                "plan H↔R table missing row for `{h_id}` — fixture expects \
                 {expected_r_ids:?}. Either the plan row is missing or the \
                 fixture is stale."
            )
        });
        // Exact-equality on the R-set so a wrong-extra-R-ID in the
        // plan ALSO fails (the brief's F3 explicit ask).
        let plan_rs_sorted: Vec<&str> = {
            let mut v: Vec<&str> = plan_rs.iter().map(String::as_str).collect();
            v.sort();
            v
        };
        let fixture_rs_sorted: Vec<&str> = {
            let mut v: Vec<&str> = expected_r_ids.to_vec();
            v.sort();
            v
        };
        assert_eq!(
            plan_rs_sorted, fixture_rs_sorted,
            "plan H↔R table maps `{h_id}` → {plan_rs:?} but fixture expects exactly \
             {expected_r_ids:?}. Sets must be equal (not subset). Either the plan \
             changed or the fixture is stale."
        );
        // The UNION of the cited rule bodies must contain every
        // keyword.
        let mut union = String::new();
        for r_id in *expected_r_ids {
            let body = rule_bodies.get(*r_id).unwrap_or_else(|| {
                panic!("skill missing `**{r_id}.**` rule body — fixture cites it for {h_id}.")
            });
            union.push_str(body);
            union.push('\n');
        }
        for keyword in *keywords {
            assert!(
                union.contains(keyword),
                "H↔R mapping says `{h_id}` → {expected_r_ids:?} should cover keyword \
                 `{keyword}`, but the union of cited rule bodies in \
                 `.claude/skills/type-cache-architecture/SKILL.md` does NOT contain it. \
                 Either the keyword fixture is stale, the plan row points at the wrong \
                 R rules, or the skill no longer carries that vocabulary."
            );
        }
    }
}

#[test]
fn h_to_r_mapping_discriminator_rejects_synthetic_remap() {
    // Negative case: the brief calls out specifically that remapping
    // `H5` → `R5 + R28` MUST fail because R5/R28's vocabulary
    // diverges from H5's `{overflow, NonCacheable, BudgetExceeded}`
    // keyword set. This self-test proves the rule-body extractor
    // and keyword-union check actually discriminate.
    let skill_md = read_file(".claude/skills/type-cache-architecture/SKILL.md");
    let rule_bodies = parse_rule_bodies(&skill_md);

    let r5 = rule_bodies.get("R5").expect("skill must define R5");
    let r28 = rule_bodies.get("R28").expect("skill must define R28");
    let union = format!("{r5}\n{r28}");

    // At least one of the H5 keywords MUST be absent from the
    // R5 ∪ R28 union — otherwise the discriminator is broken.
    let h5_keywords = ["Overflow", "NonCacheable", "BudgetExceeded"];
    let missing: Vec<&str> = h5_keywords
        .iter()
        .filter(|kw| !union.contains(**kw))
        .copied()
        .collect();
    assert!(
        !missing.is_empty(),
        "discriminator self-test: a synthetic remap of H5 → R5 + R28 MUST fail. \
         All of {h5_keywords:?} were present in the R5 ∪ R28 union — that means \
         the discriminator cannot distinguish the correct H5 → R20 + R31 mapping \
         from a wrong H5 → R5 + R28 mapping, and the H↔R test is a stub."
    );
}

#[test]
fn parse_rule_bodies_bounds_last_rule_to_owning_section() {
    // F5/F6 discriminator. The last rule (R31) MUST NOT absorb the
    // post-rule SKILL.md content (e.g. the `## Cache layer key
    // composition` table or the `## Two-phase emission map`
    // section). Without the section-header bound, R31's body would
    // carry every later section's keywords, and the H↔R
    // `union.contains(keyword)` check could match on spurious
    // trailing prose.
    let skill_md = read_file(".claude/skills/type-cache-architecture/SKILL.md");
    let rule_bodies = parse_rule_bodies(&skill_md);

    let r31 = rule_bodies
        .get("R31")
        .expect("skill must define R31 (last rule in the rule namespace)");

    // R31's body MUST NOT contain the `## Cache layer key
    // composition` header — that section sits AFTER R31 and must
    // not be glued into R31's body.
    assert!(
        !r31.contains("## Cache layer key composition"),
        "parse_rule_bodies: R31's body must NOT absorb the following \
         `## Cache layer key composition` section. Without the section-header \
         bound, the last rule's body would leak post-rule prose. R31's body \
         (truncated to 200 chars) starts: `{}`",
        &r31.chars().take(200).collect::<String>()
    );

    // R31's body MUST NOT contain the `## Two-phase emission map`
    // header — same rationale.
    assert!(
        !r31.contains("## Two-phase emission map"),
        "parse_rule_bodies: R31's body must NOT absorb the following \
         `## Two-phase emission map` section."
    );

    // R31's body MUST start with `**R31` — the defence-in-depth
    // assertion in `parse_rule_bodies` would have panicked already
    // if not. Re-check here to document the expected behaviour at
    // the call site.
    let first_line = r31.lines().next().unwrap_or("");
    assert!(
        first_line.trim_start().starts_with("**R31"),
        "parse_rule_bodies: R31's body must start with `**R31` — got `{first_line}`"
    );
}
