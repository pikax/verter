//! Confluent `TypeExpr` normalizer for the TS7 TypeExpr-projection oracle.
//!
//! This is the comparison engine of the oracle harness: both Verter's own
//! `TypeExpr` projection and the `TypeExpr` lowered (at generation time) from
//! TS7's hover are reduced to a canonical normal form before a byte-equal
//! structural compare. The two sides are spelled DIFFERENTLY by construction
//! (Verter's projection vs tsgo's display normalization), so the real soundness
//! obligation is **confluence over the admissible set** — `a ≡ b ⟹
//! normalize(a) == normalize(b)` for differently-spelled equal inputs — not mere
//! idempotence. The rewrite system is the closed, enumerated set of
//! neutral-element / absorption / canonicalization rules locked in the oracle
//! design; it is locally confluent AND terminating, so by Newman's lemma it has
//! a unique normal form regardless of rule-application order, which is why the
//! pipeline is run to a FIXPOINT rather than a single ordered pass (a step-5
//! reduction can re-expose a step-2 obligation, e.g. `true | false | boolean →
//! boolean | boolean` which the fixpoint re-dedups).
//!
//! The normalizer is DEFAULT-SAFE, symmetric to the admission allowlist's
//! default-REJECT: any construct whose two-sided spelling is not on the closed
//! confluent rule set, and any identity-bearing cosmetic-name axis not on the
//! closed canonicalization list, REJECTS the `(row, query)` rather than risk a
//! silent false divergence. A missed equal-spelling axis therefore cannot become
//! a false divergence — it rejects.
//!
//! Scope: this module is the normalization + canonical-JSON layer only. Probe
//! synthesis, the positive-allowlist admission gate, the tsgo generation driver,
//! and the snapshot store are separate concerns of the harness.

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::Value;
use verter_type_expr::{
    FunctionExpr, FunctionParam, IndexSignature, LiteralValue, MethodSignature, ObjectExpr,
    ObjectMember, ObjectProperty, PrimitiveName, TupleElement, TypeExpr, TypeParam,
};

/// Version of the normalization + canonical-encoding algorithm in this module.
///
/// Enters `snapshot_id` (so a normalizer change forces snapshot regeneration).
/// Bump on ANY change to the rewrite rules, the cosmetic-name canonicalization,
/// the literal-spelling canonicalization, or the canonical-JSON encoding.
///
/// Consumed by the snapshot-`identity` / `snapshot_id` derivation, which lands
/// in the schema increment of the harness (see the module-level handoff); the
/// constant is the pinned version contract that increment reads.
#[allow(dead_code)]
pub(crate) const NORMALIZER_VERSION: u32 = 1;

/// Why a `TypeExpr` could not be normalized into a comparable canonical form.
///
/// A reject is the default-safe outcome for a construct the normalizer cannot
/// prove confluent or cannot canonicalize — it defers the `(row, query)` rather
/// than producing a possibly-false-diverging comparison value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NormalizeReject {
    /// `TypeExpr::Unknown { raw }` — the lowering could not represent the type;
    /// a comparison value can never be derived from it (step 7).
    UnknownNode,
    /// `TypeExpr::SyntheticSlotBinding` — a display-only carrier with no
    /// hover-representable structural form.
    SyntheticSlotBinding,
    /// A `TemplateLiteral` — its cosmetic quasi/expression axis has no enumerated
    /// canonicalization rule in the initial scope, so it is default-rejected
    /// until the spike proves + versions a lossless template-literal
    /// canonicalization (a `normalizer_version` bump).
    TemplateLiteralCosmetic,
}

/// Normalize `expr` to its canonical form, then emit the pinned canonical JSON
/// string used for the byte-equal structural compare.
///
/// `mode` is the snapshot's `projection_mode`. It governs alias display
/// expectations at admission time; the normalizer itself performs no
/// resolver-side alias expansion (it holds no resolver), so a `Ref` stays a
/// `Ref` on both sides in every mode and the parameter is accepted for
/// completeness + future mode-specific handling.
// `allow(dead_code)`: consumed by the spike + the `#[cfg(test)]` normalize guards
// and a convenience for callers; in the non-test `oracle-gen` bin build (where
// the generator builds its value via `normalize` + `to_json_value` directly) it
// is unreferenced.
#[allow(dead_code)]
pub(crate) fn normalized_canonical_json(
    expr: &TypeExpr,
    mode: ProjectionModeKind,
) -> Result<String, NormalizeReject> {
    let normal = normalize(expr, mode)?;
    Ok(canonical_json_string(&normal.to_json_value()))
}

/// Normalize `expr` to its canonical `TypeExpr` normal form (steps 0–7).
pub(crate) fn normalize(
    expr: &TypeExpr,
    mode: ProjectionModeKind,
) -> Result<TypeExpr, NormalizeReject> {
    let mut scope = ScopeStack::default();
    normalize_node(expr, mode, &mut scope)
}

/// The projection-mode axis the normalizer is parameterized over.
///
/// A local mirror of the resolver's `ProjectionMode` so the normalizer module
/// stays decoupled from the resolver's query types (it only needs the four
/// discriminants for the alias-display rule). The driver maps the resolver's
/// `ProjectionMode` onto this when it normalizes Verter's projection.
///
/// Only `Shallow` / `Navigate` are admissible in the first harness block;
/// `Expanded` / `Skeleton` stay deferred until their probe-form spikes land, so
/// those discriminants are carried for schema totality but not yet constructed
/// outside tests.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProjectionModeKind {
    Shallow,
    Navigate,
    Expanded,
    Skeleton,
}

// ---------------------------------------------------------------------------
// Scope-tracked binder resolution (§Q2 step 6)
// ---------------------------------------------------------------------------

/// A stack of in-scope binder frames mapping a binder's SOURCE name to its
/// assigned positional placeholder. Each construct that introduces
/// type-parameter / mapped-key / `infer` binders pushes a frame; a `Ref` /
/// `TypeParameter` use site resolves its name against the stack innermost-first
/// (so shadowing is respected and an unrelated same-named top-level alias is
/// never captured).
#[derive(Default)]
struct ScopeStack {
    frames: Vec<HashMap<String, String>>,
}

impl ScopeStack {
    fn resolve(&self, name: &str) -> Option<String> {
        for frame in self.frames.iter().rev() {
            if let Some(placeholder) = frame.get(name) {
                return Some(placeholder.clone());
            }
        }
        None
    }
}

/// Build a frame assigning positional `T0,T1,…` placeholders to a binder name
/// list in declaration order. `start` lets a conditional's `infer` binders
/// continue the type-parameter placeholder space.
fn frame_for(
    names: impl IntoIterator<Item = String>,
    start: usize,
) -> (HashMap<String, String>, usize) {
    let mut frame = HashMap::new();
    let mut next = start;
    for name in names {
        // First binding of a given source name wins; a duplicate name in one
        // construct is degenerate and keeps the first placeholder.
        frame.entry(name).or_insert_with(|| {
            let placeholder = format!("T{next}");
            next += 1;
            placeholder
        });
    }
    (frame, next)
}

/// Collect the `infer X` binder names declared anywhere inside a conditional's
/// `extends` clause (they are in scope for the conditional's `true_type`).
fn collect_infer_names(node: &TypeExpr, out: &mut Vec<String>) {
    match node {
        TypeExpr::Infer { name } => out.push(name.clone()),
        TypeExpr::Parenthesized(inner) | TypeExpr::Rest(inner) | TypeExpr::KeyOf(inner) => {
            collect_infer_names(inner, out)
        }
        TypeExpr::Array { element, .. } => collect_infer_names(element, out),
        TypeExpr::Union(arms) | TypeExpr::Intersection(arms) => {
            arms.iter().for_each(|a| collect_infer_names(a, out))
        }
        TypeExpr::Tuple { elements, .. } => elements
            .iter()
            .for_each(|e| collect_infer_names(&e.ty, out)),
        TypeExpr::Ref { type_arguments, .. } => type_arguments
            .iter()
            .for_each(|a| collect_infer_names(a, out)),
        TypeExpr::IndexedAccess { object, index } => {
            collect_infer_names(object, out);
            collect_infer_names(index, out);
        }
        // A nested conditional binds its OWN infer names in its own extends; we
        // only collect this conditional's immediately-visible infer binders.
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Recursive normalization
// ---------------------------------------------------------------------------

fn normalize_node(
    node: &TypeExpr,
    mode: ProjectionModeKind,
    scope: &mut ScopeStack,
) -> Result<TypeExpr, NormalizeReject> {
    match node {
        // Step 1: strip Parenthesized (transparent to evaluation).
        TypeExpr::Parenthesized(inner) => normalize_node(inner, mode, scope),

        // Step 7: Unknown is a comparison failure, not normalized away.
        TypeExpr::Unknown { .. } => Err(NormalizeReject::UnknownNode),
        TypeExpr::SyntheticSlotBinding(_) => Err(NormalizeReject::SyntheticSlotBinding),

        // Step 6: TemplateLiteral cosmetic axis is default-rejected in the
        // initial scope (un-enumerated canonicalization).
        TypeExpr::TemplateLiteral { .. } => Err(NormalizeReject::TemplateLiteralCosmetic),

        // Step 0: a RecursiveRef back-edge is an opaque leaf — canonicalized to
        // a stable token, NEVER followed. This keeps the walked term finite.
        TypeExpr::RecursiveRef {
            name,
            type_arguments,
            ..
        } => {
            let args = normalize_each(type_arguments, mode, scope)?;
            // Drop the (cosmetic, possibly-cyclic) conditional context — the
            // opaque leaf is identified by name + applied args only.
            Ok(TypeExpr::RecursiveRef {
                name: Arc::clone(name),
                type_arguments: Arc::from(args),
                conditional_context: Arc::from(Vec::new()),
            })
        }

        TypeExpr::Primitive(p) => Ok(TypeExpr::Primitive(*p)),
        TypeExpr::Literal(lit) => Ok(TypeExpr::Literal(canonical_literal(lit))),

        TypeExpr::Union(arms) => {
            let normalized = normalize_each(arms, mode, scope)?;
            Ok(canonicalize_set(normalized, true))
        }
        TypeExpr::Intersection(arms) => {
            let normalized = normalize_each(arms, mode, scope)?;
            Ok(canonicalize_set(normalized, false))
        }

        TypeExpr::Array { element, readonly } => Ok(TypeExpr::Array {
            element: Arc::new(normalize_node(element, mode, scope)?),
            readonly: *readonly,
        }),

        TypeExpr::Tuple { elements, readonly } => {
            // Tuple element ORDER is semantic — never sorted. Element label is a
            // cosmetic axis the initial admissible set excludes (labelled/optional
            // tuple members are a REJECT construct), so a label is canonicalized
            // away here for confluence over the admitted (unlabelled) set.
            let mut out = Vec::with_capacity(elements.len());
            for e in elements.iter() {
                out.push(TupleElement {
                    label: None,
                    ty: normalize_node(&e.ty, mode, scope)?,
                    optional: e.optional,
                    rest: e.rest,
                });
            }
            Ok(TypeExpr::Tuple {
                elements: Arc::from(out),
                readonly: *readonly,
            })
        }

        TypeExpr::Object(obj) => Ok(TypeExpr::Object(Arc::new(normalize_object(
            obj, mode, scope,
        )?))),

        TypeExpr::Function(f) => Ok(TypeExpr::Function(Arc::new(normalize_function(
            f, mode, scope,
        )?))),
        TypeExpr::ConstructorType(f) => Ok(TypeExpr::ConstructorType(Arc::new(
            normalize_function(f, mode, scope)?,
        ))),

        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            // A use-site Ref: if its name binds to an in-scope type-parameter /
            // infer binder, rewrite to that binder's positional placeholder;
            // otherwise it is a free reference (top-level alias / package /
            // builtin) and is left UNCHANGED.
            let args = normalize_each(type_arguments, mode, scope)?;
            if args.is_empty() {
                if let Some(placeholder) = scope.resolve(name) {
                    return Ok(TypeExpr::Ref {
                        name: Arc::from(placeholder.as_str()),
                        type_arguments: Arc::from(Vec::new()),
                    });
                }
            }
            Ok(TypeExpr::Ref {
                name: Arc::clone(name),
                type_arguments: Arc::from(args),
            })
        }

        TypeExpr::TypeParameter(param) => {
            // A bare type-parameter use site. Rename to the in-scope placeholder
            // if bound; else keep the source name (a free type-parameter ref).
            if let Some(placeholder) = scope.resolve(&param.name) {
                Ok(TypeExpr::TypeParameter(TypeParam {
                    name: placeholder,
                    constraint: None,
                    default: None,
                }))
            } else {
                Ok(TypeExpr::TypeParameter(TypeParam {
                    name: param.name.clone(),
                    constraint: normalize_opt(&param.constraint, mode, scope)?,
                    default: normalize_opt(&param.default, mode, scope)?,
                }))
            }
        }

        TypeExpr::KeyOf(operand) => Ok(TypeExpr::KeyOf(Arc::new(normalize_node(
            operand, mode, scope,
        )?))),

        TypeExpr::TypeOf(vr) => Ok(TypeExpr::TypeOf(vr.clone())),

        TypeExpr::IndexedAccess { object, index } => Ok(TypeExpr::IndexedAccess {
            object: Arc::new(normalize_node(object, mode, scope)?),
            index: Arc::new(normalize_node(index, mode, scope)?),
        }),

        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            // The conditional's `extends` clause binds `infer X` names that are
            // in scope for the `true_type` branch only.
            let check_n = normalize_node(check, mode, scope)?;
            let extends_n = normalize_node(extends, mode, scope)?;
            let mut infer_names = Vec::new();
            collect_infer_names(extends, &mut infer_names);
            let (frame, _) = frame_for(infer_names, scope_next_index(scope));
            scope.frames.push(frame);
            let true_n = normalize_node(true_type, mode, scope);
            scope.frames.pop();
            let true_n = true_n?;
            let false_n = normalize_node(false_type, mode, scope)?;
            Ok(TypeExpr::Conditional {
                check: Arc::new(check_n),
                extends: Arc::new(extends_n),
                true_type: Arc::new(true_n),
                false_type: Arc::new(false_n),
            })
        }

        TypeExpr::Mapped {
            parameter,
            source,
            value,
            optional,
            readonly,
            name_type,
        } => {
            let source_n = normalize_node(source, mode, scope)?;
            // The mapped key binder is in scope for `value` and `name_type`.
            let (frame, _) = frame_for([parameter.clone()], scope_next_index(scope));
            let placeholder = frame
                .get(parameter)
                .cloned()
                .unwrap_or_else(|| "T0".to_string());
            scope.frames.push(frame);
            let value_n = normalize_node(value, mode, scope);
            let name_type_n = normalize_opt(name_type, mode, scope);
            scope.frames.pop();
            Ok(TypeExpr::Mapped {
                parameter: placeholder,
                source: Arc::new(source_n),
                value: Arc::new(value_n?),
                optional: *optional,
                readonly: *readonly,
                name_type: name_type_n?,
            })
        }

        TypeExpr::Infer { name } => {
            // An infer use/binding site: rename to its in-scope placeholder if
            // bound (it normally is, since its conditional pushed the frame).
            let renamed = scope.resolve(name).unwrap_or_else(|| name.clone());
            Ok(TypeExpr::Infer { name: renamed })
        }

        TypeExpr::Rest(inner) => Ok(TypeExpr::Rest(Arc::new(normalize_node(
            inner, mode, scope,
        )?))),
    }
}

/// Next free positional index across all in-scope frames (so nested binder
/// frames assign disjoint `T{n}` placeholders, keeping a use site's resolution
/// unambiguous under the canonical compare).
fn scope_next_index(scope: &ScopeStack) -> usize {
    scope.frames.iter().map(|f| f.len()).sum()
}

fn normalize_each(
    nodes: &[TypeExpr],
    mode: ProjectionModeKind,
    scope: &mut ScopeStack,
) -> Result<Vec<TypeExpr>, NormalizeReject> {
    nodes
        .iter()
        .map(|n| normalize_node(n, mode, scope))
        .collect()
}

fn normalize_opt(
    node: &Option<Arc<TypeExpr>>,
    mode: ProjectionModeKind,
    scope: &mut ScopeStack,
) -> Result<Option<Arc<TypeExpr>>, NormalizeReject> {
    match node {
        Some(inner) => Ok(Some(Arc::new(normalize_node(inner, mode, scope)?))),
        None => Ok(None),
    }
}

fn normalize_object(
    obj: &ObjectExpr,
    mode: ProjectionModeKind,
    scope: &mut ScopeStack,
) -> Result<ObjectExpr, NormalizeReject> {
    // Step 3: separate genuinely-UNORDERED members (property / indexSignature)
    // from ORDERED ones (call/construct/method overload groups). Sort only the
    // unordered set by a TOTAL structural key (the recursively-normalized JSON
    // of the whole member). Overload order is semantic and is PRESERVED.
    let mut unordered: Vec<ObjectMember> = Vec::new();
    let mut ordered: Vec<ObjectMember> = Vec::new();

    for m in &obj.properties {
        match m {
            ObjectMember::Property(p) => {
                unordered.push(ObjectMember::Property(
                    ObjectProperty::synthetic_with_visibility(
                        p.name.clone(),
                        normalize_node(&p.ty, mode, scope)?,
                        p.optional,
                        p.readonly,
                        p.visibility,
                    ),
                ));
            }
            ObjectMember::IndexSignature(idx) => {
                // Step 6: the index-signature parameter name is cosmetic →
                // canonicalize to the fixed token `key`.
                unordered.push(ObjectMember::IndexSignature(IndexSignature::synthetic(
                    "key".to_string(),
                    normalize_node(&idx.key_type, mode, scope)?,
                    normalize_node(&idx.value_type, mode, scope)?,
                    idx.readonly,
                )));
            }
            ObjectMember::CallSignature(f) => {
                ordered.push(ObjectMember::CallSignature(normalize_function(
                    f, mode, scope,
                )?));
            }
            ObjectMember::ConstructSignature(f) => {
                ordered.push(ObjectMember::ConstructSignature(normalize_function(
                    f, mode, scope,
                )?));
            }
            ObjectMember::Method(msig) => {
                ordered.push(ObjectMember::Method(
                    MethodSignature::synthetic_with_visibility(
                        msig.name.clone(),
                        normalize_function(&msig.function, mode, scope)?,
                        msig.optional,
                        msig.visibility,
                    ),
                ));
            }
        }
    }

    unordered.sort_by_cached_key(|m| canonical_json_string(&member_to_json(m)));

    let mut properties = unordered;
    properties.extend(ordered);
    Ok(ObjectExpr { properties })
}

fn normalize_function(
    f: &FunctionExpr,
    mode: ProjectionModeKind,
    scope: &mut ScopeStack,
) -> Result<FunctionExpr, NormalizeReject> {
    // Step 6: a function/method/call/construct signature introduces
    // type-parameter binders. Push a frame mapping each to a positional
    // placeholder BEFORE normalizing the signature body (params + return), so
    // every Ref/TypeParameter use site inside binds to the placeholder.
    let (frame, _) = frame_for(
        f.type_parameters.iter().map(|tp| tp.name.clone()),
        scope_next_index(scope),
    );
    scope.frames.push(frame);

    let result = (|| {
        // Value-parameter names are cosmetic → positional `p0,p1,…` in
        // declaration order, local to this signature.
        let mut parameters = Vec::with_capacity(f.parameters.len());
        for (i, param) in f.parameters.iter().enumerate() {
            parameters.push(FunctionParam::synthetic(
                Some(format!("p{i}")),
                normalize_node(&param.ty, mode, scope)?,
                param.optional,
                param.rest,
            ));
        }
        let return_type = match &f.return_type {
            Some(rt) => Some(Arc::new(normalize_node(rt, mode, scope)?)),
            None => None,
        };
        // Type-parameter binder declarations are renamed to their positional
        // placeholders; constraint/default are normalized in the same scope.
        let mut type_parameters = Vec::with_capacity(f.type_parameters.len());
        for tp in &f.type_parameters {
            let placeholder = scope.resolve(&tp.name).unwrap_or_else(|| tp.name.clone());
            type_parameters.push(TypeParam {
                name: placeholder,
                constraint: normalize_opt(&tp.constraint, mode, scope)?,
                default: normalize_opt(&tp.default, mode, scope)?,
            });
        }
        Ok(FunctionExpr::synthetic(
            parameters,
            return_type,
            type_parameters,
        ))
    })();

    scope.frames.pop();
    result
}

// ---------------------------------------------------------------------------
// Union / intersection canonicalization (steps 2 + 5), run to a fixpoint
// ---------------------------------------------------------------------------

/// Canonicalize a flattened arm multiset to its unique normal form. Runs the
/// closed enumerated rewrite set to a FIXPOINT so a step-5 reduction that
/// re-exposes an earlier obligation (e.g. boolean co-presence producing a
/// duplicate) re-converges. `is_union` selects the union vs intersection
/// neutral/absorbing semantics.
fn canonicalize_set(arms: Vec<TypeExpr>, is_union: bool) -> TypeExpr {
    let mut arms = arms;
    loop {
        let before = serialize_arms(&arms);

        // (a) flatten nested same-kind arms (associativity).
        arms = flatten_same_kind(arms, is_union);

        // (d) neutral / absorbing elements per TS set semantics.
        if is_union {
            // X | unknown → unknown (absorbing).
            if arms.iter().any(is_unknown_primitive) {
                return TypeExpr::Primitive(PrimitiveName::Unknown);
            }
            // X | never → X (never is the union identity).
            arms.retain(|a| !is_never_primitive(a));
        } else {
            // X & never → never (absorbing).
            if arms.iter().any(is_never_primitive) {
                return TypeExpr::Primitive(PrimitiveName::Never);
            }
            // X & unknown → X (unknown is the intersection identity).
            arms.retain(|a| !is_unknown_primitive(a));
        }

        // boolean co-presence (union only): {true,false} ⊆ arms ⟹ replace the
        // pair with `boolean`, keep every other arm. Confluent over ≥3 arms.
        if is_union {
            let has_true = arms.iter().any(|a| is_bool_literal(a, true));
            let has_false = arms.iter().any(|a| is_bool_literal(a, false));
            if has_true && has_false {
                arms.retain(|a| !is_bool_literal(a, true) && !is_bool_literal(a, false));
                arms.push(TypeExpr::Primitive(PrimitiveName::Boolean));
            }
        }

        // bounded literal subsumption (union only): drop a literal arm absorbed
        // by a CO-PRESENT primitive of its own base type.
        if is_union {
            let bases: Vec<PrimitiveName> = arms
                .iter()
                .filter_map(|a| match a {
                    TypeExpr::Primitive(p) => Some(*p),
                    _ => None,
                })
                .collect();
            arms.retain(|a| match a {
                TypeExpr::Literal(lit) => !bases.contains(&literal_base(lit)),
                _ => true,
            });
        }

        // dedup exact duplicates (by canonical key — catches mixed-spelling
        // duplicates after literal canonicalization, which the recursive
        // normalize already applied at the leaves).
        dedup_by_key(&mut arms);

        // sort by the cosmetic-name-neutralized structural key (the arms are
        // already cosmetic-name-neutralized by the recursive normalize, so the
        // canonical JSON is the neutralized key — sort and rename are not
        // circular).
        arms.sort_by_cached_key(|a| canonical_json_string(&a.to_json_value()));

        if serialize_arms(&arms) == before {
            break;
        }
    }

    match arms.len() {
        0 => {
            // An empty union is `never`; an empty intersection is `unknown`.
            if is_union {
                TypeExpr::Primitive(PrimitiveName::Never)
            } else {
                TypeExpr::Primitive(PrimitiveName::Unknown)
            }
        }
        1 => arms.into_iter().next().unwrap(),
        _ => {
            if is_union {
                TypeExpr::Union(Arc::from(arms))
            } else {
                TypeExpr::Intersection(Arc::from(arms))
            }
        }
    }
}

fn flatten_same_kind(arms: Vec<TypeExpr>, is_union: bool) -> Vec<TypeExpr> {
    let mut out = Vec::with_capacity(arms.len());
    for arm in arms {
        match (&arm, is_union) {
            (TypeExpr::Union(inner), true) | (TypeExpr::Intersection(inner), false) => {
                out.extend(inner.iter().cloned());
            }
            _ => out.push(arm),
        }
    }
    out
}

fn dedup_by_key(arms: &mut Vec<TypeExpr>) {
    let mut seen = std::collections::HashSet::new();
    arms.retain(|a| seen.insert(canonical_json_string(&a.to_json_value())));
}

fn serialize_arms(arms: &[TypeExpr]) -> String {
    let mut s = String::new();
    for a in arms {
        s.push_str(&canonical_json_string(&a.to_json_value()));
        s.push('\u{1f}');
    }
    s
}

fn is_unknown_primitive(t: &TypeExpr) -> bool {
    matches!(t, TypeExpr::Primitive(PrimitiveName::Unknown))
}

fn is_never_primitive(t: &TypeExpr) -> bool {
    matches!(t, TypeExpr::Primitive(PrimitiveName::Never))
}

fn is_bool_literal(t: &TypeExpr, value: bool) -> bool {
    matches!(t, TypeExpr::Literal(LiteralValue::Boolean(b)) if *b == value)
}

fn literal_base(lit: &LiteralValue) -> PrimitiveName {
    match lit {
        LiteralValue::String(_) => PrimitiveName::String,
        LiteralValue::Number(_) => PrimitiveName::Number,
        LiteralValue::Boolean(_) => PrimitiveName::Boolean,
        LiteralValue::BigInt(_) => PrimitiveName::BigInt,
    }
}

// ---------------------------------------------------------------------------
// Literal-value spelling canonicalization (step 5)
// ---------------------------------------------------------------------------

/// Canonicalize a literal's SPELLING (never its VALUE). Numeric literals are
/// already canonical at the `f64` level (radix/exponent spelling is gone once
/// parsed), and string literals already hold their decoded value, so the only
/// active rewrite is the `BigInt` decimal spelling.
fn canonical_literal(lit: &LiteralValue) -> LiteralValue {
    match lit {
        LiteralValue::BigInt(raw) => LiteralValue::BigInt(canonical_bigint(raw)),
        other => other.clone(),
    }
}

/// Canonical decimal spelling of a bigint literal: drop a trailing `n` suffix,
/// drop a leading `+`, collapse leading zeros (keeping one for zero), preserve a
/// leading `-`. A spelling this function does not understand is returned
/// verbatim (it will simply compare by its raw bytes — never silently equated to
/// a different value).
fn canonical_bigint(raw: &str) -> String {
    let s = raw.strip_suffix('n').unwrap_or(raw);
    let (sign, digits) = match s.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", s.strip_prefix('+').unwrap_or(s)),
    };
    // Only canonicalize a plain decimal-digit run; leave radix/other spellings
    // verbatim (the admissible set is plain decimals).
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return raw.to_string();
    }
    let trimmed = digits.trim_start_matches('0');
    let value = if trimmed.is_empty() { "0" } else { trimmed };
    if value == "0" {
        // Zero is unsigned-canonical.
        "0".to_string()
    } else {
        format!("{sign}{value}")
    }
}

// ---------------------------------------------------------------------------
// Canonical JSON (§Q1 canonical-encoding rules)
// ---------------------------------------------------------------------------

/// Serialize a `serde_json::Value` under the pinned canonical encoding: object
/// keys sorted lexicographically by their UTF-8 bytes, no insignificant
/// whitespace, minimal string escaping (serde's default), arrays in semantic
/// order. Feature-independent (does not rely on `serde_json`'s map ordering).
pub(crate) fn canonical_json_string(value: &Value) -> String {
    let mut out = String::new();
    write_canonical(value, &mut out);
    out
}

fn write_canonical(value: &Value, out: &mut String) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => out.push_str(&n.to_string()),
        Value::String(s) => out.push_str(&escape_json_string(s)),
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_canonical(item, out);
            }
            out.push(']');
        }
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort_unstable_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
            out.push('{');
            for (i, key) in keys.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&escape_json_string(key));
                out.push(':');
                write_canonical(&map[key], out);
            }
            out.push('}');
        }
    }
}

/// Minimal JSON string escaping via serde (only the mandatory `"`, `\`, and
/// control escapes; no gratuitous `\uXXXX` for printable characters).
fn escape_json_string(s: &str) -> String {
    serde_json::Value::String(s.to_string()).to_string()
}

/// Serialize one object member to its canonical JSON (used as the total
/// structural sort key for unordered members). Mirrors `TypeExpr::to_json_value`
/// member encoding so the key is the same shape the snapshot stores.
fn member_to_json(member: &ObjectMember) -> Value {
    // Wrap the single member in a one-member object and pull out the member
    // value, reusing the production encoder so the key never drifts from the
    // stored encoding.
    let obj = TypeExpr::Object(Arc::new(ObjectExpr {
        properties: vec![member.clone()],
    }));
    obj.to_json_value()["properties"][0].clone()
}

#[cfg(test)]
mod tests;
