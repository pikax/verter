use super::*;

#[test]
fn no_implicit_public_member_constructors_in_production() {
    let violations = member_visibility_constructor_violations();
    assert!(
        violations.is_empty(),
        "Member-visibility guard violations: production source uses an\n\
             implicit-Public member constructor. `ObjectProperty::synthetic` /\n\
             `MethodSignature::synthetic` / `ObjectProperty::with_spans` /\n\
             `MethodSignature::with_spans` silently mint members as `Public`,\n\
             which is the recurring non-public-member leak class. Use the\n\
             intent-explicit constructors instead:\n\
             - source-LESS public origin (interface / type-literal /\n\
               object-literal / enum / framework member): `synthetic_public_key` /\n\
               `with_key_spans_public`.\n\
             - source-DERIVED reconstruction (member already carries a\n\
               visibility — member-path / Pick / indexed-access):\n\
               `synthetic_key_with_visibility` / `with_key_visibility`.\n\n\
             Violations:\n  {}",
        violations
            .iter()
            .map(|(rel, lineno, line)| format!("{rel}:{lineno}: {}", line.trim()))
            .collect::<Vec<_>>()
            .join("\n  "),
    );
}

#[test]
fn member_visibility_constructor_predicate_discriminates() {
    // BANNED — every implicit-Public construction form, including the
    // fully-qualified path and embedded call shapes.
    let banned = [
        "let p = ObjectProperty::synthetic(\"a\".into(), ty, false, false);",
        "ObjectMember::Method(MethodSignature::synthetic(\"m\".into(), f, false))",
        "Some(ObjectProperty::with_spans(name, ty, false, false, spans))",
        "ObjectMember::Method(MethodSignature::with_spans(n, f, false, spans))",
        "verter_type_expr::ObjectProperty::synthetic(name, ty, false, false)",
    ];
    for line in banned {
        assert!(
            line_has_banned_visibility_constructor(line),
            "guard must FLAG banned constructor line: {line:?}",
        );
    }

    // ALLOWED — the explicit replacements, `IndexSignature` (no
    // accessibility concept), and prose that merely names the methods.
    let allowed = [
        "ObjectProperty::synthetic_public_key(\"a\".into(), ty, false, false)",
        "MethodSignature::synthetic_public_key(\"m\".into(), f, false)",
        "ObjectProperty::with_key_spans_public(name, ty, false, false, spans)",
        "MethodSignature::with_key_spans_public(n, f, false, spans)",
        "ObjectProperty::synthetic_key_with_visibility(name, ty, false, false, vis)",
        "MethodSignature::synthetic_key_with_visibility(n, f, false, vis)",
        "ObjectProperty::with_key_visibility(name, ty, false, false, vis, spans)",
        "MethodSignature::with_key_visibility(n, f, false, vis, spans)",
        "IndexSignature::synthetic(key, kty, vty, false)",
        "IndexSignature::with_spans(key, kty, vty, false, spans)",
        "/// Source-DERIVED reconstructions MUST use `Self::with_key_visibility`.",
    ];
    for line in allowed {
        assert!(
            !line_has_banned_visibility_constructor(line),
            "guard must NOT flag allowed line: {line:?}",
        );
    }
}

#[test]
fn no_same_crate_member_struct_literals_in_verter_type_expr() {
    let violations = same_crate_member_struct_literal_violations();
    assert!(
        violations.is_empty(),
        "Same-crate member struct-literal guard violations: a file in\n\
             `crates/verter_type_expr/src/**` constructs `ObjectProperty` /\n\
             `MethodSignature` with a NAMED struct literal. `#[non_exhaustive]`\n\
             does not block same-crate struct literals, so this would let a\n\
             member be minted with an unconsidered `visibility` — the recurring\n\
             non-public-member leak class. Construct through the\n\
             visibility-threading constructors instead (`synthetic_public_key` /\n\
             `synthetic_key_with_visibility` / `with_key_spans_public` /\n\
             `with_key_visibility`), whose bodies use `Self {{ .. }}`.\n\n\
             Violations:\n  {}",
        violations
            .iter()
            .map(|(rel, lineno, line)| format!("{rel}:{lineno}: {}", line.trim()))
            .collect::<Vec<_>>()
            .join("\n  "),
    );
}

#[test]
fn same_crate_member_struct_literal_predicate_discriminates() {
    // BANNED — every named struct-literal construction form.
    let banned = [
            "let p = ObjectProperty { name, ty, optional: false, readonly: false, visibility, spans };",
            "ObjectMember::Property(ObjectProperty { name, ty, optional, readonly, visibility, spans })",
            "        MethodSignature { name, function, optional, visibility, spans }",
            "Some(MethodSignature { name, function, optional, visibility, spans })",
        ];
    for line in banned {
        assert!(
            line_has_same_crate_member_struct_literal(line),
            "guard must FLAG same-crate struct literal: {line:?}",
        );
    }

    // ALLOWED — the type definitions (the sole `<Name> {` occurrence), the
    // `Self { .. }` constructor bodies, constructor CALLS (`::synthetic*`),
    // and prose / field accesses that merely name the types.
    let allowed = [
        "pub struct ObjectProperty {",
        "pub struct MethodSignature {",
        "impl ObjectProperty {",
        "impl MethodSignature {",
        "        Self {",
        "        ObjectProperty::synthetic_public_key(name, ty, false, false)",
        "        MethodSignature::with_key_visibility(n, f, false, vis, spans)",
        "    pub visibility: MemberVisibility,",
        "/// Construct an `ObjectProperty` carrying its declared visibility.",
        "let names: Vec<ObjectProperty> = members.clone();",
        // Function RETURN type with an inline body brace names, not
        // constructs, the type.
        "    fn rebuild(&self) -> ObjectProperty {",
        "fn make_method() -> MethodSignature {",
    ];
    for line in allowed {
        assert!(
            !line_has_same_crate_member_struct_literal(line),
            "guard must NOT flag allowed line: {line:?}",
        );
    }
}

#[test]
fn d14_predicate_rejects_deliberate_violation_and_passes_clean_source() {
    // Discriminating-violation: a fabricated production source
    // string that uses `std::fs::` MUST be flagged by the
    // predicate. A counter-fixture that does NOT use `std::fs::`
    // must NOT be flagged.
    let bad = "use std::fs::File;\nfn read() { let _ = std::fs::read_to_string(\"foo\"); }";
    assert!(
        d14_file_uses_std_fs(bad),
        "D14 predicate must flag direct `std::fs::` references",
    );

    let clean = "use crate::workspace::NativeFs;\nfn read(fs: &NativeFs) { let _ = fs.read_file(\"foo\"); }";
    assert!(
        !d14_file_uses_std_fs(clean),
        "D14 predicate must NOT flag code that goes through NativeFs",
    );
}

#[test]
fn d14_each_allow_list_entry_is_a_real_walker_hit() {
    // Discriminator: the D14 invariant lock is meaningful ONLY
    // when each `D14_ALLOW_LIST` entry actually corresponds to a
    // production walker hit. If an entry mapped to a path the
    // walker never reaches (wrong directory, typo'd path, file
    // moved without updating the entry), the entry has zero
    // protective effect and the lock can drift silently.
    //
    // This test runs the violation walker with an empty
    // allow-list (only `native_fs.rs` exempt) and asserts:
    //   1. The walker produces SOMETHING — proving the lock is
    //      non-trivial and the production tree contains real
    //      escapes from NativeFs that the allow-list is paying
    //      for.
    //   2. EVERY entry in `D14_ALLOW_LIST` shows up in that
    //      empty-allow-list violation set — proving each entry
    //      actually maps to a real `std::fs::` callsite the
    //      walker would otherwise flag.
    //
    // Removing the live ALLOW_LIST entries would, by transitive
    // implication, make the live `no_std_fs_outside_native_fs_or_allow_list`
    // test fail with violations equal to (this empty-allow-list
    // set) minus (any newly-migrated callsites). That is the
    // pre-change failure the brief requires this guard to
    // exhibit.
    let mut empty_permitted: BTreeSet<String> = BTreeSet::new();
    empty_permitted.insert(D14_NATIVE_FS_PATH.to_string());
    let violations = d14_violations(&empty_permitted);
    assert!(
        !violations.is_empty(),
        "D14 walker must detect at least one production `std::fs::` callsite outside\n\
             `native_fs.rs` when the allow-list is empty. If this fails, either the walker is\n\
             scoped to the wrong tree, or every previous escape from NativeFs has been migrated\n\
             (in which case `D14_ALLOW_LIST` should also be empty and this discriminator test\n\
             should be deleted along with it).",
    );
    let violations_set: BTreeSet<String> = violations.iter().cloned().collect();
    let mut entries_without_walker_hits: Vec<String> = Vec::new();
    for (path, _justification) in D14_ALLOW_LIST {
        if !violations_set.contains(*path) {
            entries_without_walker_hits.push((*path).to_string());
        }
    }
    assert!(
        entries_without_walker_hits.is_empty(),
        "D14 ALLOW_LIST entries must each represent a real walker hit. The following\n\
             entries are NOT detected as violations even when the allow-list is empty:\n  {}\n\n\
             A non-violating entry has no protective effect; either delete it or fix the path\n\
             so the entry actually maps to a production `std::fs::` callsite.",
        entries_without_walker_hits.join("\n  "),
    );
}

#[test]
fn origin_fence_guard_predicate_rejects_deliberate_violations() {
    // Each fabricated line models a real reintroduction of the
    // retired origin-edge-into-fence merge.
    let forbidden = [
        // The retired API by name — a re-declaration or a call.
        "    pub fn origins_with_fence(&self, node: SemanticNodeId) -> Vec<OriginEdge> {",
        "        let visited = store.origins_with_fence(result, &fence);",
        "/// merges each edge's dep-signature via `origins_with_fence`.",
        // The forbidden merge shape — `merge_signature` folding an
        // origin edge's `edge_dep_signature` snapshot into a fence.
        "            fence.merge_signature(&edge.edge_dep_signature);",
        "    active_fence.merge_signature(&origin.edge_dep_signature);",
    ];
    for line in forbidden {
        assert!(
            line_reconstructs_fence_from_origin_edge(line),
            "origin-fence guard predicate must reject deliberate-violation line: {line:?}",
        );
    }
    // Lines that look superficially similar but are NOT violations:
    // a `merge_signature` of a memo entry's OWN carrier (never names
    // `edge_dep_signature`), and an `edge_dep_signature` touched for
    // a purpose other than a fence merge (interning, dedup probe).
    let allowed = [
            // Legitimate fence merge of a cached read's own carrier.
            "    crate::fact_signature_helpers::merge_dep_signature_into_local_fence(local_fence, &read.dep_signature);",
            "        fence.merge_signature(&read.dep_signature);",
            // `edge_dep_signature` touched for interning / dedup — no
            // fence merge on the line.
            "        let interned = self.intern_signature(edge.edge_dep_signature.clone());",
            "            && Arc::ptr_eq(&existing.edge_dep_signature, &candidate.edge_dep_signature)",
            // Plain prose about origin edges that does not name the
            // retired API.
            "// Origin edges are bounded best-effort provenance, not an invalidation source.",
        ];
    for line in allowed {
        assert!(
            !line_reconstructs_fence_from_origin_edge(line),
            "origin-fence guard predicate must NOT flag legitimate line: {line:?}",
        );
    }
}
