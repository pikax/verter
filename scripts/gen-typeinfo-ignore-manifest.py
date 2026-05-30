#!/usr/bin/env python3
"""Regenerate the typeinfo ignored-test manifest rows.

The output file lives at
`crates/verter_session/tests/manifest_data/typeinfo_ignored_test_manifest_rows.rs`
and is `include!`-d by the manifest guard at
`crates/verter_session/tests/typeinfo_ignored_test_manifest.rs`.

Each row pairs one `#[ignore = "..."]` annotation in
`crates/verter_session/src/typeinfo/typeinfo_tests/**/*.rs` with:
- the test file name,
- the test function name (the `fn <name>` token next to the ignore),
- the closed-enum `TargetSubstrate` substrate that will unblock the
  test (per-file mapping below — adjust when a new test file or
  substrate lands),
- the existing ignore-reason string (the unblocker sentence).

Run after adding / removing / renaming an ignored test:

    python3 scripts/gen-typeinfo-ignore-manifest.py

Commit the regenerated rows file alongside the typeinfo-test source
changes that prompted the regeneration.
"""

from __future__ import annotations

import os
import re
import sys
from pathlib import Path

# Per-file -> substrate mapping. Adjust when adding a new test file
# or substrate variant.
FILE_TO_SUBSTRATE: dict[str, str] = {
    "apparent_types.rs": "ApparentTypes",
    "basic.rs": "MacroResolution",
    "branded_types.rs": "ApparentTypes",
    "cache_invalidation.rs": "CacheInvalidation",
    "call_resolution.rs": "CallResolution",
    "class_features.rs": "ClassFeatures",
    "conditional_infer.rs": "ConditionalInfer",
    "const_type_param.rs": "TypeParameterFeatures",
    "contextual_typing.rs": "ContextualTyping",
    "cross_file.rs": "CrossFileResolution",
    "decorators.rs": "ClassFeatures",
    "deep_path.rs": "PathProjection",
    "demand_boundary.rs": "DemandBoundary",
    "enums.rs": "EnumResolution",
    "expansion_boundaries.rs": "ExpansionBoundaries",
    "flow_invalidations.rs": "FlowNarrowing",
    "flow_return_catalog.rs": "FlowNarrowing",
    "footprint.rs": "AuditFootprint",
    "function_advanced.rs": "CallResolution",
    "index_signatures.rs": "IndexSignatures",
    "indexed_utilities.rs": "UtilityComposition",
    "jsx.rs": "JsxResolution",
    "mapped_modifiers.rs": "MappedTypes",
    "mapped_template.rs": "MappedTypes",
    "menu_like.rs": "CompositeSurfaces",
    "message_list_like.rs": "CompositeSurfaces",
    "mode_boundary_invariants.rs": "ModeBoundary",
    "modern_ts_features.rs": "ModernTsFeatures",
    "module_features.rs": "ModuleFeatures",
    "narrow_discriminated_union.rs": "FlowNarrowing",
    "narrow_equality.rs": "FlowNarrowing",
    "narrow_in_operator.rs": "FlowNarrowing",
    "narrow_instanceof.rs": "FlowNarrowing",
    "narrow_truthiness.rs": "FlowNarrowing",
    "narrow_typeof.rs": "FlowNarrowing",
    "no_infer.rs": "ConditionalInfer",
    "recursive_conditional.rs": "ConditionalInfer",
    "relation_semantics.rs": "RelationSemantics",
    "substitution_types.rs": "TypeParameterFeatures",
    "table_like.rs": "CompositeSurfaces",
    "template_literal_inference.rs": "TemplateLiteralInference",
    "tuple_labels.rs": "TupleFeatures",
    "typescript_rules.rs": "TypeScriptRules",
    "union_key_access.rs": "UnionDistribution",
    "unique_symbol.rs": "UniqueSymbol",
    "utility_composition.rs": "UtilityComposition",
    "utility_edge.rs": "UtilityComposition",
    "utility_top_bottom.rs": "UtilityComposition",
    "value_inference.rs": "ValueInference",
    "variadic_tuples.rs": "TupleFeatures",
    "wide_deep.rs": "PathProjection",
}


def escape_rust_string_literal(s: str) -> str:
    return s.replace("\\", "\\\\").replace("\"", "\\\"")


def extract_sites(source: str) -> list[tuple[str, str]]:
    """Return `(reason, fn_name)` for every literal-string
    `#[ignore = "..."]` site in `source`.

    The reason regex recognises Rust string-literal escape syntax — a
    character in the string is either a non-quote / non-backslash
    char OR a backslash followed by any char. The naive
    `"([^"]*)"` pattern truncates reasons containing escaped quotes
    (`\\"`) at the first internal quote.
    """
    sites: list[tuple[str, str]] = []
    lines = source.splitlines()
    for i, raw in enumerate(lines):
        line = raw.strip()
        if not line.startswith("#[ignore"):
            continue
        rest = line[len("#[ignore"):].lstrip()
        if not rest.startswith("=") or "\"" not in rest:
            continue
        m = re.search(r'"((?:[^"\\]|\\.)*)"', rest)
        if not m:
            continue
        reason = m.group(1)
        fn_name: str | None = None
        for j in range(i + 1, min(i + 6, len(lines))):
            fm = re.search(r"fn\s+(\w+)", lines[j])
            if fm:
                fn_name = fm.group(1)
                break
        if fn_name:
            sites.append((reason, fn_name))
    return sites


def main() -> int:
    repo_root = Path(__file__).resolve().parent.parent
    src_dir = (
        repo_root
        / "crates"
        / "verter_session"
        / "src"
        / "typeinfo"
        / "typeinfo_tests"
    )
    if not src_dir.is_dir():
        print(f"typeinfo_tests dir missing: {src_dir}", file=sys.stderr)
        return 2

    out_path = (
        repo_root
        / "crates"
        / "verter_session"
        / "tests"
        / "manifest_data"
        / "typeinfo_ignored_test_manifest_rows.rs"
    )
    out_path.parent.mkdir(parents=True, exist_ok=True)

    lines: list[str] = []
    lines.append("// Auto-generated manifest rows. Regenerate by running")
    lines.append("// `python3 scripts/gen-typeinfo-ignore-manifest.py` and")
    lines.append("// committing the diff alongside the typeinfo-test source")
    lines.append("// changes that prompted the regeneration. Each row pairs")
    lines.append("// one `#[ignore = \"...\"]` annotation with the closed-enum")
    lines.append("// `TargetSubstrate` substrate that will lift the ignore.")
    lines.append("")
    lines.append("#[rustfmt::skip]")
    lines.append("const EXPECTED_IGNORE_MANIFEST: &[IgnoredTestRow] = &[")

    total = 0
    missing_mappings: list[str] = []
    for fn in sorted(os.listdir(src_dir)):
        if not fn.endswith(".rs"):
            continue
        source = (src_dir / fn).read_text()
        sites = extract_sites(source)
        if not sites:
            # Files without any literal-string `#[ignore = "..."]`
            # site never contribute manifest rows and do not need a
            # substrate mapping.
            continue
        substrate = FILE_TO_SUBSTRATE.get(fn)
        if substrate is None:
            missing_mappings.append(fn)
            continue
        for reason, fn_name in sites:
            escaped = escape_rust_string_literal(reason)
            lines.append(
                f"    IgnoredTestRow {{ file: \"{fn}\", "
                f"function: \"{fn_name}\", "
                f"substrate: TargetSubstrate::{substrate}, "
                f"unblocker: \"{escaped}\" }},"
            )
            total += 1

    lines.append("];")

    if missing_mappings:
        # Unknown typeinfo-test files MUST be mapped to a substrate
        # before manifest regeneration succeeds — silently defaulting
        # to a fallback substrate would let drift slip into the
        # generated rows file.
        print(
            "error: the following typeinfo-test files have no "
            "FILE_TO_SUBSTRATE mapping; add an entry for each before "
            "regenerating the manifest:",
            file=sys.stderr,
        )
        for fn in missing_mappings:
            print(f"  - {fn}", file=sys.stderr)
        return 3

    out_path.write_text("\n".join(lines) + "\n")
    print(f"wrote {out_path} with {total} rows", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
