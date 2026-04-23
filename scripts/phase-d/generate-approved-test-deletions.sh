#!/usr/bin/env bash
set -euo pipefail

# Files whose every #[test] is retired wholesale.
DELETE_FILES=(
  crates/verter_semantic/src/analysis/type_solver/project.rs
  crates/verter_semantic/src/analysis/type_solver/relate.rs
  crates/verter_session/src/resolver_core/solver_host.rs
  crates/verter_session/src/resolver_core/solver_host_tests.rs
  crates/verter_session/src/resolver_core/type_surface_db.rs
  crates/verter_session/src/resolver_core/type_surface_tests.rs
  crates/verter_session/src/dispatch_bridge.rs
)

# Surviving files where #[test]s referencing DELETE_SYMBOLS are retired
# (the tests' subject code is deleted). Tests in OTHER files that reference
# these symbols are MIGRATION targets (§5.7), not DELETION targets.
SYMBOL_SCAN_FILES=(
  crates/verter_semantic/src/analysis/type_solver/solve.rs
  crates/verter_semantic/src/analysis/type_solver/query_engine.rs
  crates/verter_semantic/src/analysis/type_solver/arena.rs
  crates/verter_semantic/src/analysis/type_solver/host.rs
)

DELETE_SYMBOLS=(
  TypeQueryEngine TypeSolverHost EvalEnvSolverHost SessionSolverHost
  TypeSurfaceDb TypeSurfaceOpResult SolverCaches
  resolve_node resolve_indexed_access resolve_conditional
  collect_structural_property_descriptors_inner resolve_prepared_ref
  resolve_type_parameters_in_body resolve_keyspace resolve_member
  project_type_parameter_refs
)

# Explicit full test IDs. Also acts as the collision escape hatch: any fn
# name whose last ::-segment appears in an EXPLICIT_TEST_IDS entry is
# skipped by the intent-map collision checks and resolved exactly via pass 3.
EXPLICIT_TEST_IDS=(
  meta::meta_tests::public_component_meta_keeps_simple_imported_alias_union_surface
  meta_resolve::meta_resolve_tests::produce_one_macro_object_shape_keeps_projection_rescue_for_indexed_access_aliases
  meta_resolve::meta_resolve_tests::resolve_component_meta_populates_compute_audit_when_enabled
  meta_resolve::meta_resolve_tests::produce_one_macro_object_shape_skips_redundant_projection_for_generic_ref_solver_shapes
  meta_resolve::meta_resolve_tests::produce_one_macro_object_shape_skips_projection_rescue_for_nested_indexed_property_types
)

BASELINE_IDS="${BASELINE_IDS:-/tmp/phase-d-baseline-ids.txt}"
if [[ ! -f "$BASELINE_IDS" ]]; then
  echo "ERROR: BASELINE_IDS '$BASELINE_IDS' missing" >&2
  exit 1
fi

# State-machine scanner: consumes ALL consecutive `#[...]` / whitespace /
# line-comment lines after `#[test]` before expecting the `fn` declaration.
# Handles #[should_panic], #[ignore = "..."], #[cfg(debug_assertions)],
# doc comments, blank lines.
extract_test_fns() {
  local file="$1"
  [[ -f "$file" ]] || return 0
  awk '
    /^[[:space:]]*#\[test\][[:space:]]*$/ { in_test = 1; next }
    in_test {
      if ($0 ~ /^[[:space:]]*#\[/) next
      if ($0 ~ /^[[:space:]]*$/) next
      if ($0 ~ /^[[:space:]]*\/\//) next
      if (match($0, /(^|[[:space:]])fn[[:space:]]+([a-zA-Z_][a-zA-Z_0-9]*)/, arr)) {
        print arr[2]
      }
      in_test = 0
    }
  ' "$file"
}

extract_fns_referencing_symbol() {
  local file="$1"
  local sym="$2"
  [[ -f "$file" ]] || return 0
  awk -v sym="$sym" '
    /^[[:space:]]*#\[test\][[:space:]]*$/ { in_test = 1; next }
    in_test && !tracking {
      if ($0 ~ /^[[:space:]]*#\[/) next
      if ($0 ~ /^[[:space:]]*$/) next
      if ($0 ~ /^[[:space:]]*\/\//) next
      if (match($0, /(^|[[:space:]])fn[[:space:]]+([a-zA-Z_][a-zA-Z_0-9]*)/, arr)) {
        cur = arr[2]
        tracking = 1; depth = 0; body = ""; started = 0
      } else {
        in_test = 0
      }
    }
    tracking {
      body = body "\n" $0
      for (i = 1; i <= length($0); i++) {
        c = substr($0, i, 1)
        if (c == "{") { depth++; started = 1 }
        else if (c == "}") {
          depth--
          if (started && depth == 0) {
            if (match(body, "\\<" sym "\\>")) print cur
            tracking = 0; in_test = 0; break
          }
        }
      }
    }
  ' "$file"
}

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

# Collision escape hatch: names whose last ::-segment appears in EXPLICIT_TEST_IDS.
explicit_fn_names="$tmp/explicit-fn-names.txt"
for id in "${EXPLICIT_TEST_IDS[@]}"; do
  printf '%s\n' "${id##*::}"
done | sort -u > "$explicit_fn_names"

retiring_files="$tmp/retiring-files.txt"
printf '%s\n' "${DELETE_FILES[@]}" "${SYMBOL_SCAN_FILES[@]}" | sort -u > "$retiring_files"

truly_surviving_files="$tmp/truly-surviving-files.txt"
find crates/ -name '*.rs' -type f | sort -u > "$tmp/all-rs.txt"
comm -23 "$tmp/all-rs.txt" "$retiring_files" > "$truly_surviving_files"

truly_surviving_fns="$tmp/surviving-fns.txt"
: > "$truly_surviving_fns"
while IFS= read -r file; do
  extract_test_fns "$file" >> "$truly_surviving_fns"
done < "$truly_surviving_files"
sort -u "$truly_surviving_fns" -o "$truly_surviving_fns"

# Intent map: fn_name → pipe-delimited list of retiring files that want it deleted.
declare -A fn_to_files_intent

for file in "${DELETE_FILES[@]}"; do
  while IFS= read -r fn; do
    [[ -n "$fn" ]] || continue
    if [[ -n "${fn_to_files_intent[$fn]:-}" ]]; then
      fn_to_files_intent[$fn]="${fn_to_files_intent[$fn]}|$file"
    else
      fn_to_files_intent[$fn]="$file"
    fi
  done < <(extract_test_fns "$file")
done

for file in "${SYMBOL_SCAN_FILES[@]}"; do
  for sym in "${DELETE_SYMBOLS[@]}"; do
    while IFS= read -r fn; do
      [[ -n "$fn" ]] || continue
      if [[ -n "${fn_to_files_intent[$fn]:-}" ]]; then
        case "|${fn_to_files_intent[$fn]}|" in
          *"|$file|"*) ;;
          *) fn_to_files_intent[$fn]="${fn_to_files_intent[$fn]}|$file" ;;
        esac
      else
        fn_to_files_intent[$fn]="$file"
      fi
    done < <(extract_fns_referencing_symbol "$file" "$sym")
  done
done

approved="$tmp/approved.txt"
: > "$approved"

for fn in "${!fn_to_files_intent[@]}"; do
  # Escape hatch: explicit full IDs bypass collision checks; pass 3 resolves.
  if grep -Fx -q "$fn" "$explicit_fn_names"; then
    continue
  fi
  # Collision with truly-surviving file.
  if grep -Fx -q "$fn" "$truly_surviving_fns"; then
    echo "FAIL: #[test] fn '$fn' is in a retiring file AND a truly-surviving file." >&2
    echo "      Remediation: add specific full test ID(s) to EXPLICIT_TEST_IDS." >&2
    exit 1
  fi
  # Multi-file collision within retiring set.
  files_entry="${fn_to_files_intent[$fn]}"
  IFS='|' read -ra files_arr <<< "$files_entry"
  if [[ "${#files_arr[@]}" -gt 1 ]]; then
    echo "FAIL: #[test] fn '$fn' appears in multiple retiring files:" >&2
    for f in "${files_arr[@]}"; do echo "        $f" >&2; done
    echo "      Remediation: add every full test ID to EXPLICIT_TEST_IDS." >&2
    exit 1
  fi
  grep -E "::${fn}\$" "$BASELINE_IDS" >> "$approved" || true
done

# Pass 3: explicit full test IDs — exact match, hard-fail on missing or non-unique.
#
# `grep -c` exits 1 on zero matches (not zero), so `$(grep -c ... || echo 0)`
# would emit "0\n0". Use a safe counter that always succeeds and returns a
# single numeric line.
for id in "${EXPLICIT_TEST_IDS[@]}"; do
  count=$(awk -v target="$id" '$0 == target { n++ } END { print n + 0 }' "$BASELINE_IDS")
  if [[ "$count" -eq 0 ]]; then
    echo "FAIL: explicit test ID not in baseline: $id" >&2
    exit 1
  elif [[ "$count" -gt 1 ]]; then
    echo "FAIL: explicit test ID non-unique in baseline: $id" >&2
    exit 1
  fi
  echo "$id" >> "$approved"
done

sort -u "$approved" | grep -v '^$'
