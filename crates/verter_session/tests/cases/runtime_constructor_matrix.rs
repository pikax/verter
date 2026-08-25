//! Vue runtime-constructor prop resolution, proven at the PUBLIC boundary
//! across the full cross of authoring forms and invocation shapes.
//!
//! The cross is:
//!
//! - runtime form — shorthand (`label: String`), expanded
//!   (`{ type: String }`), `required: true`, optional, with `default`,
//!   constructor array (`[String, Number]`), nullable (`[String, null]`),
//!   three-element array, and a MIXED array whose elements do not all carry
//!   a primitive fact;
//! - constructor kind — `String`/`Number`/`Boolean` positive; and, as
//!   NEGATIVE CONTROLS, the seven recognised constructors with no primitive
//!   fact (`Array`/`Object`/`Function`/`Symbol`/`Date`/`RegExp`/`Promise`),
//!   a custom class (module-declared and imported), and a locally SHADOWED
//!   `String`/`Number` spelling;
//! - macro (`defineProps`) AND Options-API (`props:`) extraction, which
//!   share ONE producer-owned typed runtime-constructor fact;
//! - invocation — cold, warm (same session, resolved twice), sequential,
//!   `Promise.all`-equivalent concurrent, and batch
//!   (`get_component_meta_output_batch`);
//! - request-view scope — base session and overlay, each running the whole
//!   invocation cross.
//!
//! Analyzer- and fold-level coverage lives in `verter_semantic`
//! (`component_meta_tests.rs`, `root_binding_index_tests.rs`); this file is
//! the PUBLIC-BOUNDARY half — every cell is resolved through the real
//! `VerterHost`/`MetaSession` output surface, never a hand-built analysis.
//!
//! Two independent properties are asserted per cell, in every mode:
//!
//! 1. **Exactness** — each published prop's materialized type is classified
//!    STRUCTURALLY into a [`ShapeTag`] and compared against the cell's
//!    required shape. A positive cell must land on [`ShapeTag::Fold`] with
//!    exactly the required primitives in authored order; a negative control
//!    must publish a type, must NOT be a `Fold`, and must equal its own
//!    required shape exactly. `required`/`has_default` are pinned too.
//! 2. **Invocation invariance** — every invocation mode publishes an
//!    IDENTICAL result for the same cell. A mode-dependent answer is a
//!    defect even when every individual mode satisfies (1).
//!
//! Both halves are asserted for every mode: agreement alone passes on a
//! uniformly wrong answer, and per-mode exactness alone passes on a
//! mode-dependent-but-individually-plausible one.

use std::sync::Arc;

use verter_session::meta::MetaProject;
use verter_session::{AnalysisLevel, HostConfig, VerterHost};
use verter_type_expr::{ObjectMember, PrimitiveName, TypeAuthoredPropertyKey, TypeExpr};

fn make_project() -> Arc<MetaProject> {
    let host = VerterHost::new_standalone_with_scheduler_config(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        // Single-threaded scheduler: avoids CPU oversubscription across the
        // many parallel test-binary threads each spinning up their own pool
        // (the same rationale `verter_session::meta_tests` documents).
        verter_scheduler::scheduler::SchedulerConfig {
            cpu_threads: 1,
            ..verter_scheduler::scheduler::SchedulerConfig::default()
        },
    );
    MetaProject::new(host)
}

// ─────────────────────────────────────────────────────────────────────────
// Structural classification of a published type
// ─────────────────────────────────────────────────────────────────────────

/// A published materialized type reduced to the structural facts this matrix
/// discriminates on. Deriving the classification STRUCTURALLY (rather than
/// comparing a `Debug` rendering) keeps the assertions independent of span
/// offsets and formatting, so a fixture reflow cannot turn a correct result
/// into a false regression, while still failing on any real shape change.
#[derive(Clone, Debug, PartialEq, Eq)]
enum ShapeTag {
    /// Nothing was published at all.
    Absent,
    /// EXACTLY the shape the runtime-constructor fold produces: a single
    /// foldable primitive, or a union built only from foldable primitives,
    /// in authored order. This is the positive outcome — and, for a
    /// negative control, the forbidden one.
    Fold(Vec<PrimitiveName>),
    /// `TypeExpr::Primitive(Unknown)` — the materialized side of the
    /// display-text-only route the seven non-primitive recognised
    /// constructors take (their text rides on `type_annotation` instead).
    UnknownPrimitive,
    /// `TypeExpr::Unknown(..)` — the typed missing-output carrier.
    UnknownCarrier,
    /// An object type whose named property keys are exactly these, in
    /// order. A shadowed local's own resolved shape lands here.
    ObjectKeys(Vec<String>),
    /// An object type carrying exactly one construct signature returning
    /// this named type — a class value's `typeof` shape.
    ConstructorOf(String),
    /// Any other structure. Carries the `Debug` rendering purely so a
    /// failure message can say what was actually published.
    Other(String),
}

/// The four primitives the runtime-constructor fold can produce. Anything
/// outside this set is by definition not a fold result.
fn foldable(name: PrimitiveName) -> bool {
    matches!(
        name,
        PrimitiveName::String
            | PrimitiveName::Number
            | PrimitiveName::Boolean
            | PrimitiveName::Null
    )
}

fn classify(ty: Option<&TypeExpr>) -> ShapeTag {
    let Some(ty) = ty else {
        return ShapeTag::Absent;
    };
    match ty {
        TypeExpr::Primitive(PrimitiveName::Unknown) => ShapeTag::UnknownPrimitive,
        TypeExpr::Primitive(name) if foldable(*name) => ShapeTag::Fold(vec![*name]),
        TypeExpr::Union(arms) if !arms.is_empty() => {
            let folded: Option<Vec<PrimitiveName>> = arms
                .iter()
                .map(|arm| match arm {
                    TypeExpr::Primitive(name) if foldable(*name) => Some(*name),
                    _ => None,
                })
                .collect();
            folded.map_or_else(|| ShapeTag::Other(format!("{ty:?}")), ShapeTag::Fold)
        }
        TypeExpr::Unknown(_) => ShapeTag::UnknownCarrier,
        TypeExpr::Object(object) => {
            if let [ObjectMember::ConstructSignature(signature)] = object.properties.as_slice() {
                if let Some(TypeExpr::Ref { name, .. }) = signature.return_type.as_deref() {
                    return ShapeTag::ConstructorOf(name.to_string());
                }
            }
            let keys: Vec<String> = object
                .properties
                .iter()
                .filter_map(|member| match member {
                    ObjectMember::Property(property) => match &property.key {
                        TypeAuthoredPropertyKey::String(name) => Some(name.to_string()),
                        _ => None,
                    },
                    _ => None,
                })
                .collect();
            if keys.len() == object.properties.len() && !keys.is_empty() {
                ShapeTag::ObjectKeys(keys)
            } else {
                ShapeTag::Other(format!("{ty:?}"))
            }
        }
        other => ShapeTag::Other(format!("{other:?}")),
    }
}

// ─────────────────────────────────────────────────────────────────────────
// The matrix
// ─────────────────────────────────────────────────────────────────────────

/// The required shape of a NEGATIVE CONTROL — a position the
/// runtime-constructor fold must NOT capture. Every arm names a real,
/// distinguishable published shape, so "not folded" is never satisfied by
/// the position having quietly lost its type: a control that degraded from
/// [`Self::ObjectKeys`] to [`Self::UnknownPrimitive`] fails, and so does one
/// that degraded the other way.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NegShape {
    /// The display-text-only route: materialized `unknown`, text on
    /// `type_annotation`. What the seven non-primitive recognised
    /// constructors, and an unresolvable macro-path payload, publish.
    UnknownPrimitive,
    /// The typed missing-output carrier.
    UnknownCarrier,
    /// A resolved object type with exactly these property keys.
    ObjectKeys(&'static [&'static str]),
    /// A class value's `typeof` shape: one construct signature returning
    /// this named type.
    ConstructorOf(&'static str),
}

impl NegShape {
    fn tag(self) -> ShapeTag {
        match self {
            Self::UnknownPrimitive => ShapeTag::UnknownPrimitive,
            Self::UnknownCarrier => ShapeTag::UnknownCarrier,
            Self::ObjectKeys(keys) => {
                ShapeTag::ObjectKeys(keys.iter().map(|k| (*k).to_string()).collect())
            }
            Self::ConstructorOf(name) => ShapeTag::ConstructorOf(name.to_string()),
        }
    }
}

/// What one matrix cell requires of ONE published prop's type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Expect {
    /// A `Global`-resolved runtime constructor whose producer-minted
    /// identity carries a closed primitive fact: the published type is
    /// EXACTLY that primitive.
    Primitive(PrimitiveName),
    /// A constructor ARRAY: the published type is EXACTLY the union of
    /// these primitives, in authored element order.
    Union(&'static [PrimitiveName]),
    /// NEGATIVE CONTROL. Three INDEPENDENT properties must hold, and the
    /// second and third are what make this a control rather than a
    /// tautology:
    ///
    /// 1. The position still PUBLISHES a materialized type. "Not a
    ///    primitive fold" alone is satisfied by publishing nothing at all.
    /// 2. That type is NOT a [`ShapeTag::Fold`] — the exact shape the
    ///    runtime-constructor fold produces. This is the fold-capture guard.
    /// 3. It equals the named [`NegShape`] EXACTLY. Without this, a control
    ///    stays green while silently degrading into a different non-fold
    ///    shape, which is how a "not a primitive" assertion passes on a
    ///    position that has lost all its meaning.
    NotFolded(NegShape),
}

/// One published prop's required outcome.
struct PropExpectation {
    name: &'static str,
    ty: Expect,
    required: bool,
    has_default: bool,
}

/// One matrix cell: a `.vue` source, any sibling modules it imports, and
/// every prop's required outcome.
struct Cell {
    id: &'static str,
    source: &'static str,
    /// Sibling modules this cell's source imports, upserted alongside it.
    deps: &'static [(&'static str, &'static str)],
    props: &'static [PropExpectation],
}

/// Load one cell and everything it imports into a project.
fn load(project: &Arc<MetaProject>, cell: &Cell) {
    for (id, source) in cell.deps {
        project.upsert_base(id, source).unwrap();
    }
    project.upsert_base(cell.id, cell.source).unwrap();
}

const fn prop(
    name: &'static str,
    ty: Expect,
    required: bool,
    has_default: bool,
) -> PropExpectation {
    PropExpectation {
        name,
        ty,
        required,
        has_default,
    }
}

const fn string(name: &'static str, required: bool, has_default: bool) -> PropExpectation {
    prop(
        name,
        Expect::Primitive(PrimitiveName::String),
        required,
        has_default,
    )
}

const fn number(name: &'static str, required: bool, has_default: bool) -> PropExpectation {
    prop(
        name,
        Expect::Primitive(PrimitiveName::Number),
        required,
        has_default,
    )
}

const fn boolean(name: &'static str, required: bool, has_default: bool) -> PropExpectation {
    prop(
        name,
        Expect::Primitive(PrimitiveName::Boolean),
        required,
        has_default,
    )
}

/// A negative control that takes the display-text-only route.
const fn display_only(name: &'static str, required: bool) -> PropExpectation {
    prop(
        name,
        Expect::NotFolded(NegShape::UnknownPrimitive),
        required,
        false,
    )
}

/// The module the imported-constructor cells import from. `String` shadows a
/// recognised runtime-constructor spelling; `Shadow` does not. Their member
/// keys differ so a cell cannot pass by resolving the wrong one.
const IMPORTED_CTOR_MODULE: &[(&str, &str)] = &[(
    "/rcm/imported-ctors.ts",
    "export const String = { from: 1 }\nexport const Shadow = { via: 2 }\n",
)];

/// The same values reached through a BARREL that only re-exports them. The
/// barrel declares nothing itself, so an anchor placed on it addresses no
/// body — the export route has to be walked to the real declaring module.
const IMPORTED_BARREL_MODULES: &[(&str, &str)] = &[
    (
        "/rcm/imported-ctors.ts",
        "export const String = { from: 1 }\nexport const Shadow = { via: 2 }\n",
    ),
    (
        "/rcm/imported-barrel.ts",
        "export { String, Shadow } from './imported-ctors'\n",
    ),
];

const STRING_NUMBER: &[PrimitiveName] = &[PrimitiveName::String, PrimitiveName::Number];
const STRING_NULL: &[PrimitiveName] = &[PrimitiveName::String, PrimitiveName::Null];
const NUMBER_NULL: &[PrimitiveName] = &[PrimitiveName::Number, PrimitiveName::Null];
const STRING_NUMBER_BOOLEAN: &[PrimitiveName] = &[
    PrimitiveName::String,
    PrimitiveName::Number,
    PrimitiveName::Boolean,
];

/// The matrix: every runtime form × constructor kind, across BOTH the macro
/// and Options-API extraction paths.
const CELLS: &[Cell] = &[
    // ── macro: shorthand form, all three positive constructor kinds ──
    Cell {
        id: "/rcm/Shorthand.vue",
        source: r#"<script setup lang="ts">
defineProps({ label: String, count: Number, flag: Boolean })
</script>
<template><div /></template>"#,
        deps: &[],
        props: &[
            string("label", false, false),
            number("count", false, false),
            boolean("flag", false, false),
        ],
    },
    // ── macro: expanded form × required / optional / default ──
    Cell {
        id: "/rcm/Expanded.vue",
        source: r#"<script setup lang="ts">
defineProps({
  label: { type: String },
  count: { type: Number, required: true },
  flag: { type: Boolean },
  size: { type: String, default: 'md' },
})
</script>
<template><div /></template>"#,
        deps: &[],
        props: &[
            string("label", false, false),
            number("count", true, false),
            boolean("flag", false, false),
            string("size", false, true),
        ],
    },
    // ── macro: constructor arrays — two-element, nullable (both orders),
    //    and three-element ──
    Cell {
        id: "/rcm/Arrays.vue",
        source: r#"<script setup lang="ts">
defineProps({
  value: [String, Number],
  maybe: [String, null],
  nn: [Number, null],
  three: [String, Number, Boolean],
})
</script>
<template><div /></template>"#,
        deps: &[],
        props: &[
            prop("value", Expect::Union(STRING_NUMBER), false, false),
            prop("maybe", Expect::Union(STRING_NULL), false, false),
            prop("nn", Expect::Union(NUMBER_NULL), false, false),
            prop("three", Expect::Union(STRING_NUMBER_BOOLEAN), false, false),
        ],
    },
    // ── macro: the seven recognised constructors that carry NO primitive
    //    fact. They resolve `Global` through the SAME producer-minted
    //    identity and the SAME fold site as `String`, so they are the
    //    sharpest available control on `RuntimeConstructorIdentity::
    //    primitive()` returning `None`: any of them acquiring a primitive
    //    fact shows up here immediately. ──
    Cell {
        id: "/rcm/NonPrimitiveCtors.vue",
        source: r#"<script setup lang="ts">
defineProps({
  list: Array,
  obj: Object,
  fn: Function,
  sym: Symbol,
  when: Date,
  re: { type: RegExp, required: true },
  p: Promise,
})
</script>
<template><div /></template>"#,
        deps: &[],
        props: &[
            display_only("list", false),
            display_only("obj", false),
            display_only("fn", false),
            display_only("sym", false),
            display_only("when", false),
            display_only("re", true),
            display_only("p", false),
        ],
    },
    // ── macro: a MIXED constructor array. `Date` carries no primitive
    //    fact, so `primitives.collect()` must short-circuit and leave the
    //    WHOLE array off the fold — publishing `string` here (silently
    //    dropping the `Date` arm) would narrow the author's surface. The
    //    fully-foldable sibling proves the cell resolves at all. ──
    Cell {
        id: "/rcm/MixedArray.vue",
        source: r#"<script setup lang="ts">
defineProps({ mixed: [String, Date], pure: [String, Number] })
</script>
<template><div /></template>"#,
        deps: &[],
        props: &[
            display_only("mixed", false),
            prop("pure", Expect::Union(STRING_NUMBER), false, false),
        ],
    },
    // ── macro: a module-owned SHADOW of the `String` spelling. This is the
    //    control on the `Global`/`Local` binding gate itself: `String` here
    //    is a real local value, so the fold must not fire and the general
    //    authored-value-reference route must publish the local's OWN shape.
    //    A gate regression publishes `string` and this cell goes red.
    //
    //    (A setup-local declaration deliberately does NOT shadow — Vue
    //    relocates the `defineProps` runtime argument out of `setup()`
    //    before it runs. That asymmetry is owned by
    //    `root_binding_index_tests.rs`; the module-owned form is the one
    //    with a public-boundary consequence.) ──
    Cell {
        id: "/rcm/ShadowModule.vue",
        source: r#"<script lang="ts">
const String = { from: 1 }
</script>
<script setup lang="ts">
defineProps({ label: String, count: Number })
</script>
<template><div /></template>"#,
        deps: &[],
        props: &[
            prop(
                "label",
                Expect::NotFolded(NegShape::ObjectKeys(&["from"])),
                false,
                false,
            ),
            number("count", false, false),
        ],
    },
    // ── macro: an IMPORTED runtime-constructor binding. The binding index
    //    resolves an import to `Local` (an import always lands at Program
    //    root), so the fold must not fire and the value must be raised
    //    CROSS-FILE from the exporting module.
    //
    //    Two spellings, deliberately: `String` is a recognised constructor
    //    name, `Shadow` is not. Both are imported values, and both must
    //    publish the imported value's own shape. That pair discriminates
    //    binding ORIGIN from constructor-NAME classification — a repair
    //    that special-cased the constructor spellings would leave `Shadow`
    //    broken, and one that keyed on the name would leave `String`
    //    broken. The locally-declared `count` proves the cell resolves.
    Cell {
        id: "/rcm/ImportedCtor.vue",
        source: r#"<script setup lang="ts">
import { String, Shadow } from './imported-ctors'
defineProps({ label: String, thing: Shadow, count: Number })
</script>
<template><div /></template>"#,
        deps: IMPORTED_CTOR_MODULE,
        props: &[
            prop(
                "label",
                Expect::NotFolded(NegShape::ObjectKeys(&["from"])),
                false,
                false,
            ),
            prop(
                "thing",
                Expect::NotFolded(NegShape::ObjectKeys(&["via"])),
                false,
                false,
            ),
            number("count", false, false),
        ],
    },
    // ── macro: the imported binding reached through a BARREL re-export.
    //    The direct-hop table stops at the barrel, which declares nothing;
    //    only walking the export route reaches the real declaration. This
    //    cell is what distinguishes routing through the shared value-export
    //    authority from resolving one hop locally. ──
    Cell {
        id: "/rcm/ImportedBarrel.vue",
        source: r#"<script setup lang="ts">
import { String, Shadow } from './imported-barrel'
defineProps({ label: String, thing: Shadow, count: Number })
</script>
<template><div /></template>"#,
        deps: IMPORTED_BARREL_MODULES,
        props: &[
            prop(
                "label",
                Expect::NotFolded(NegShape::ObjectKeys(&["from"])),
                false,
                false,
            ),
            prop(
                "thing",
                Expect::NotFolded(NegShape::ObjectKeys(&["via"])),
                false,
                false,
            ),
            number("count", false, false),
        ],
    },
    // ── Options API: the same imported-binding origin on the other
    //    extraction path. ──
    Cell {
        id: "/rcm/ImportedCtorOptions.vue",
        source: r#"<script lang="ts">
import { String, Shadow } from './imported-ctors'
export default {
  props: { label: String, thing: Shadow, count: Number },
}
</script>
<template><div /></template>"#,
        deps: IMPORTED_CTOR_MODULE,
        props: &[
            prop(
                "label",
                Expect::NotFolded(NegShape::ObjectKeys(&["from"])),
                false,
                false,
            ),
            prop(
                "thing",
                Expect::NotFolded(NegShape::ObjectKeys(&["via"])),
                false,
                false,
            ),
            number("count", false, false),
        ],
    },
    // ── macro: a custom class beside a folding sibling. The class is
    //    declared in the MODULE script, which is the route that resolves:
    //    the authored value reference publishes the class's real `typeof`
    //    shape. Pinning that shape is the point — a control pinned to
    //    `unknown` would assert a loss as if it were the contract. ──
    // Deferred forms are captured by
    // `prop_type_assertion_publishes_its_object_shape_on_both_prop_routes` and
    // `script_setup_local_class_publishes_the_class_constructor_shape`.
    Cell {
        id: "/rcm/Negatives.vue",
        source: r#"<script lang="ts">
class Thing {
  id = 1
}
export { Thing }
</script>
<script setup lang="ts">
defineProps({
  thing: Thing,
  label: String,
})
</script>
<template><div /></template>"#,
        deps: &[],
        props: &[
            prop(
                "thing",
                Expect::NotFolded(NegShape::ConstructorOf("Thing")),
                false,
                false,
            ),
            string("label", false, false),
        ],
    },
    // ── macro: the same control reached by IMPORT, which is the other
    //    route that resolves a class to its real shape. ──
    Cell {
        id: "/rcm/ImportedClass.vue",
        source: r#"<script setup lang="ts">
import { Thing } from './thing-src'
defineProps({ thing: Thing, label: String })
</script>
<template><div /></template>"#,
        deps: &[("/rcm/thing-src.ts", "export class Thing { id = 1 }\n")],
        props: &[
            prop(
                "thing",
                Expect::NotFolded(NegShape::ConstructorOf("Thing")),
                false,
                false,
            ),
            string("label", false, false),
        ],
    },
    // ─────────────────────────────────────────────────────────────────
    // The literal runtime-form x constructor-kind cross.
    //
    // FORMS (rows): shorthand `k: C`; expanded `{ type: C }`; required
    // `{ type: C, required: true }`; with-default `{ type: C, default: v }`.
    // KINDS (columns): the three folding constructors `String`/`Number`/
    // `Boolean`; `Date`, standing for the seven recognised constructors that
    // carry no closed primitive fact and take the display-text-only route;
    // and a module-declared custom class, standing for the authored-value
    // route. Five kinds x four forms = twenty cells, one row per file.
    //
    // Every kind appears in every form, so a fold that works in the
    // shorthand form but not under `required`, or a class that resolves
    // bare but not behind `{ type: ... }`, is caught here rather than
    // inferred from a single representative cell.
    //
    // The array forms are a separate axis — an array is not a `{ type: }`
    // object and has no `required`/`default` of its own — and are crossed in
    // `/rcm/Arrays.vue` and `/rcm/MixedArray.vue`.
    // ─────────────────────────────────────────────────────────────────
    Cell {
        id: "/rcm/CrossShorthand.vue",
        source: r#"<script lang="ts">
class Thing {
  id = 1
}
export { Thing }
</script>
<script setup lang="ts">
defineProps({ s: String, n: Number, b: Boolean, d: Date, k: Thing })
</script>
<template><div /></template>"#,
        deps: &[],
        props: &[
            string("s", false, false),
            number("n", false, false),
            boolean("b", false, false),
            display_only("d", false),
            prop(
                "k",
                Expect::NotFolded(NegShape::ConstructorOf("Thing")),
                false,
                false,
            ),
        ],
    },
    Cell {
        id: "/rcm/CrossExpanded.vue",
        source: r#"<script lang="ts">
class Thing {
  id = 1
}
export { Thing }
</script>
<script setup lang="ts">
defineProps({
  s: { type: String },
  n: { type: Number },
  b: { type: Boolean },
  d: { type: Date },
  k: { type: Thing },
})
</script>
<template><div /></template>"#,
        deps: &[],
        props: &[
            string("s", false, false),
            number("n", false, false),
            boolean("b", false, false),
            display_only("d", false),
            prop(
                "k",
                Expect::NotFolded(NegShape::ConstructorOf("Thing")),
                false,
                false,
            ),
        ],
    },
    Cell {
        id: "/rcm/CrossRequired.vue",
        source: r#"<script lang="ts">
class Thing {
  id = 1
}
export { Thing }
</script>
<script setup lang="ts">
defineProps({
  s: { type: String, required: true },
  n: { type: Number, required: true },
  b: { type: Boolean, required: true },
  d: { type: Date, required: true },
  k: { type: Thing, required: true },
})
</script>
<template><div /></template>"#,
        deps: &[],
        props: &[
            string("s", true, false),
            number("n", true, false),
            boolean("b", true, false),
            display_only("d", true),
            prop(
                "k",
                Expect::NotFolded(NegShape::ConstructorOf("Thing")),
                true,
                false,
            ),
        ],
    },
    Cell {
        id: "/rcm/CrossDefault.vue",
        source: r#"<script lang="ts">
class Thing {
  id = 1
}
export { Thing }
</script>
<script setup lang="ts">
defineProps({
  s: { type: String, default: 'md' },
  n: { type: Number, default: 0 },
  b: { type: Boolean, default: false },
  d: { type: Date, default: () => new Date() },
  k: { type: Thing, default: () => new Thing() },
})
</script>
<template><div /></template>"#,
        deps: &[],
        props: &[
            string("s", false, true),
            number("n", false, true),
            boolean("b", false, true),
            prop(
                "d",
                Expect::NotFolded(NegShape::UnknownPrimitive),
                false,
                true,
            ),
            prop(
                "k",
                Expect::NotFolded(NegShape::ConstructorOf("Thing")),
                false,
                true,
            ),
        ],
    },
    // ── Options API: the SAME forms through `extract_props_from_options`,
    //    proving both extraction paths share one producer-owned fact. ──
    Cell {
        id: "/rcm/Options.vue",
        source: r#"<script lang="ts">
class Thing {
  id = 1
}
export default {
  props: {
    label: String,
    count: { type: Number, required: true },
    flag: { type: Boolean },
    value: [String, Number],
    maybe: [String, null],
    size: { type: String, default: 'md' },
    when: Date,
    mixed: [String, Date],
    thing: Thing,
  },
}
</script>
<template><div /></template>"#,
        deps: &[],
        props: &[
            string("label", false, false),
            number("count", true, false),
            boolean("flag", false, false),
            prop("value", Expect::Union(STRING_NUMBER), false, false),
            prop("maybe", Expect::Union(STRING_NULL), false, false),
            string("size", false, true),
            prop(
                "when",
                Expect::NotFolded(NegShape::UnknownCarrier),
                false,
                false,
            ),
            prop(
                "mixed",
                Expect::NotFolded(NegShape::UnknownCarrier),
                false,
                false,
            ),
            // The Options path resolves the class value to its real
            // `typeof` shape rather than degrading to `unknown` — pinned
            // exactly, so a regression in EITHER direction fails.
            // Deferred forms are captured by
            // `prop_type_assertion_publishes_its_object_shape_on_both_prop_routes` and
            // `script_setup_local_class_publishes_the_class_constructor_shape`.
            prop(
                "thing",
                Expect::NotFolded(NegShape::ConstructorOf("Thing")),
                false,
                false,
            ),
        ],
    },
    // ── Options API: the shadow control, proving the binding gate is
    //    applied on this extraction path too and not only on the macro
    //    path. `Number` is a module-owned local here; `String` is not. ──
    Cell {
        id: "/rcm/ShadowOptions.vue",
        source: r#"<script lang="ts">
const Number = { parse: 1 }
export default {
  props: { count: Number, label: String },
}
</script>
<template><div /></template>"#,
        deps: &[],
        props: &[
            prop(
                "count",
                Expect::NotFolded(NegShape::ObjectKeys(&["parse"])),
                false,
                false,
            ),
            string("label", false, false),
        ],
    },
];

// ─────────────────────────────────────────────────────────────────────────
// Reading the public surface
// ─────────────────────────────────────────────────────────────────────────

/// One published prop, reduced to the facts this matrix compares. `shape` is
/// the structural classification; comparing `Published` values across
/// invocation modes therefore compares STRUCTURE, not a rendering.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Published {
    name: String,
    shape: ShapeTag,
    required: bool,
    has_default: bool,
}

type Output = verter_session::meta_resolve::ComponentMetaOutput;

fn rows(output: Output) -> Vec<Published> {
    let (analysis, _resolution, types) = output.into_parts();
    let lanes = types.into_lanes();
    analysis
        .props
        .iter()
        .enumerate()
        .map(|(index, p)| Published {
            name: p.name.clone(),
            shape: classify(lanes.props[index].materialized_type()),
            required: p.required,
            has_default: p.has_default,
        })
        .collect()
}

/// Read every published prop of one component through the BASE output
/// surface.
fn publish(host: &VerterHost, id: &str) -> Vec<Published> {
    let output = host
        .get_component_meta_output(id)
        .unwrap_or_else(|error| {
            panic!(
                "{id}: output materialization must not fail — a runtime-constructor \
                 position must never surface a typed materialization error: {error:?}"
            )
        })
        .unwrap_or_else(|| panic!("{id}: the component must resolve, not be absent"));
    rows(output)
}

/// Read one component's published props through a SESSION (overlay) view.
fn publish_overlay(session: &verter_session::meta::MetaSession, id: &str) -> Vec<Published> {
    let output = session
        .get_component_meta_output(id)
        .unwrap_or_else(|error| panic!("{id} [overlay]: {error:?}"))
        .unwrap_or_else(|| panic!("{id} [overlay]: absent"));
    rows(output)
}

fn published_shape(published: &[Published], name: &str) -> ShapeTag {
    published
        .iter()
        .find(|prop| prop.name == name)
        .unwrap_or_else(|| {
            panic!(
                "prop `{name}` is absent from the published surface; published: {:?}",
                published.iter().map(|prop| &prop.name).collect::<Vec<_>>()
            )
        })
        .shape
        .clone()
}

// ─────────────────────────────────────────────────────────────────────────
// Type-bearing runtime props publish their authored shapes
// ─────────────────────────────────────────────────────────────────────────

/// Correction owner: the maintainer's post-plan type-correction work, per
/// `MAINTAINER-RULING-BUGS-AND-TYPES` rule 3.
#[test]
#[ignore = "captured authored runtime-prop `as PropType<T>` assertion defect on BOTH prop routes: the authored payload never reaches publication. The MACRO arm is the mechanism shared with the `as () => T` capture - the props normalizer selects the runtime object member's closed `Unknown` leaf before consulting the authored payload. The OPTIONS arm loses it separately and publishes an unknown CARRIER, a different shape, so the macro mechanism is not claimed for it. Deferred to the maintainer's post-plan type-correction work per MAINTAINER-RULING-BUGS-AND-TYPES rule 3"]
fn prop_type_assertion_publishes_its_object_shape_on_both_prop_routes() {
    let project = make_project();
    let macro_id = "/rcm/CapturedPropTypeMacro.vue";
    let options_id = "/rcm/CapturedPropTypeOptions.vue";
    let control_id = "/rcm/CapturedPropTypeShapeControl.vue";

    project
        .upsert_base(
            macro_id,
            r#"<script setup lang="ts">
import type { PropType } from 'vue'
defineProps({
  item: { type: Object as PropType<{ id: number }> },
  label: String,
})
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            options_id,
            r#"<script lang="ts">
import type { PropType } from 'vue'
export default {
  props: {
    item: { type: Object as PropType<{ id: number }> },
    label: String,
  },
}
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            control_id,
            r#"<script setup lang="ts">
defineProps<{ anchor: { id: number } }>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let macro_props = publish(project.host(), macro_id);
    let options_props = publish(project.host(), options_id);
    let control_props = publish(project.host(), control_id);

    assert_eq!(
        published_shape(&macro_props, "label"),
        ShapeTag::Fold(vec![PrimitiveName::String]),
        "{macro_id}: soundness anchor — the sibling `label: String` prop must publish \
         Fold([String]) before testing the PropType assertion defect on the macro route",
    );
    assert_eq!(
        published_shape(&options_props, "label"),
        ShapeTag::Fold(vec![PrimitiveName::String]),
        "{options_id}: soundness anchor — the sibling `label: String` prop must publish \
         Fold([String]) before testing the PropType assertion defect on the Options route",
    );
    assert_eq!(
        published_shape(&control_props, "anchor"),
        ShapeTag::ObjectKeys(vec!["id".to_string()]),
        "{control_id}: soundness anchor — the type-declared control must publish \
         ObjectKeys([\"id\"]) and witness the expected shape before the PropType defect is tested",
    );

    assert_eq!(
        (
            published_shape(&macro_props, "item"),
            published_shape(&options_props, "item"),
        ),
        (
            ShapeTag::ObjectKeys(vec!["id".to_string()]),
            ShapeTag::ObjectKeys(vec!["id".to_string()]),
        ),
        "PropType assertion defect — `{macro_id}` (macro) and `{options_id}` (Options) \
         must both publish the asserted object shape ObjectKeys([\"id\"]); the left tuple \
         shows the actual lost-type shapes in that route order",
    );
}

/// Correction owner: the maintainer's post-plan type-correction work, per
/// `MAINTAINER-RULING-BUGS-AND-TYPES` rule 3.
#[test]
#[ignore = "captured `<script setup>`-local class declaration-site defect: its bare constructor reference never enters the authored-payload route; deferred to the maintainer's post-plan type-correction work per MAINTAINER-RULING-BUGS-AND-TYPES rule 3"]
fn script_setup_local_class_publishes_the_class_constructor_shape() {
    let project = make_project();
    let module_id = "/rcm/CapturedModuleClass.vue";
    let setup_id = "/rcm/CapturedSetupLocalClass.vue";

    project
        .upsert_base(
            module_id,
            r#"<script lang="ts">
class Thing {
  id = 1
}
export { Thing }
</script>
<script setup lang="ts">
defineProps({ thing: Thing })
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            setup_id,
            r#"<script setup lang="ts">
class Thing {
  id = 1
}
defineProps({ thing: Thing })
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let module_props = publish(project.host(), module_id);
    assert_eq!(
        published_shape(&module_props, "thing"),
        ShapeTag::ConstructorOf("Thing".to_string()),
        "{module_id}: soundness anchor — the module-script form of the same class must \
         publish ConstructorOf(\"Thing\") before testing the script-setup-local defect",
    );

    let setup_props = publish(project.host(), setup_id);
    assert_eq!(
        published_shape(&setup_props, "thing"),
        ShapeTag::ConstructorOf("Thing".to_string()),
        "{setup_id}: script-setup-local class defect — `thing` must publish the same \
         ConstructorOf(\"Thing\") shape as the module-script control, not the actual \
         lost-type shape shown on the left",
    );
}

/// Correction owner: the maintainer's post-plan type-correction work, per
/// `MAINTAINER-RULING-BUGS-AND-TYPES` rule 3.
///
/// This is the same macro-route defect class as
/// `prop_type_assertion_publishes_its_object_shape_on_both_prop_routes`:
/// `as () => T` is a second authored spelling of the same single loss point.
/// It is not the declaration-site defect captured by
/// `script_setup_local_class_publishes_the_class_constructor_shape`.
#[test]
#[ignore = "captured authored runtime-prop `as` assertion defect: its payload is discarded when the props normalizer selects the runtime object member's closed `Unknown` leaf; deferred to the maintainer's post-plan type-correction work per MAINTAINER-RULING-BUGS-AND-TYPES rule 3"]
fn runtime_prop_as_function_assertion_publishes_its_object_shape() {
    let project = make_project();
    let id = "/rcm/CapturedRuntimePropAsFunction.vue";
    let control_id = "/rcm/CapturedRuntimePropAsFunctionShapeControl.vue";

    project
        .upsert_base(
            id,
            r#"<script setup lang="ts">
defineProps({
  label: String,
  count: Number,
  flag: Boolean,
  item: { type: Object as () => { id: number } },
})
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            control_id,
            r#"<script setup lang="ts">
defineProps<{ anchor: { id: number } }>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let props = publish(project.host(), id);
    let control_props = publish(project.host(), control_id);
    assert_eq!(
        published_shape(&props, "label"),
        ShapeTag::Fold(vec![PrimitiveName::String]),
        "{id}: soundness anchor — the sibling `label: String` prop must publish \
         Fold([String]) before testing the authored runtime-prop `as` assertion defect",
    );
    assert_eq!(
        published_shape(&props, "count"),
        ShapeTag::Fold(vec![PrimitiveName::Number]),
        "{id}: soundness anchor — the sibling `count: Number` prop must publish \
         Fold([Number]) before testing the authored runtime-prop `as` assertion defect",
    );
    assert_eq!(
        published_shape(&props, "flag"),
        ShapeTag::Fold(vec![PrimitiveName::Boolean]),
        "{id}: soundness anchor — the sibling `flag: Boolean` prop must publish \
         Fold([Boolean]) before testing the authored runtime-prop `as` assertion defect",
    );
    assert_eq!(
        published_shape(&control_props, "anchor"),
        ShapeTag::ObjectKeys(vec!["id".to_string()]),
        "{control_id}: soundness anchor — the type-declared control must publish \
         ObjectKeys([\"id\"]) and witness the expected shape before the authored \
         runtime-prop `as` assertion defect is tested",
    );

    assert_eq!(
        published_shape(&props, "item"),
        ShapeTag::ObjectKeys(vec!["id".to_string()]),
        "{id}: authored runtime-prop `as` assertion defect — `item` must publish \
         ObjectKeys([\"id\"]), not the actual lost-type shape shown on the left",
    );
}

// ─────────────────────────────────────────────────────────────────────────
// The single assertion authority
// ─────────────────────────────────────────────────────────────────────────

/// Assert one cell's published props satisfy its expectations EXACTLY.
///
/// This is the ONLY place a matrix expectation is checked. Every mode — base
/// and overlay, cold, warm, sequential, concurrent and batch — routes through
/// it, so no mode can be held to a weaker contract than another by accident.
fn assert_published(cell: &Cell, published: &[Published], mode: &str) {
    assert_eq!(
        published.len(),
        cell.props.len(),
        "{} [{mode}]: the published prop count must match the authored one exactly \
         — an extra or dropped prop is a surface defect; published: {:?}",
        cell.id,
        published.iter().map(|p| &p.name).collect::<Vec<_>>(),
    );

    for expectation in cell.props {
        let row = published
            .iter()
            .find(|p| p.name == expectation.name)
            .unwrap_or_else(|| {
                panic!(
                    "{} [{mode}]: prop `{}` is missing from the published surface — \
                     a runtime-constructor prop is never silently omitted; published: {:?}",
                    cell.id,
                    expectation.name,
                    published.iter().map(|p| &p.name).collect::<Vec<_>>()
                )
            });

        match expectation.ty {
            Expect::Primitive(want) => assert_eq!(
                row.shape,
                ShapeTag::Fold(vec![want]),
                "{} [{mode}]: prop `{}` must publish EXACTLY the closed primitive \
                 {want:?} the runtime constructor folds to — not `unknown`, not the \
                 unresolved display placeholder, not a wider union",
                cell.id,
                expectation.name,
            ),
            Expect::Union(want) => assert_eq!(
                row.shape,
                ShapeTag::Fold(want.to_vec()),
                "{} [{mode}]: prop `{}` is a constructor ARRAY and must publish \
                 EXACTLY the union {want:?} in authored element order — a dropped or \
                 reordered element silently narrows the author's declared surface",
                cell.id,
                expectation.name,
            ),
            Expect::NotFolded(want) => {
                // (1) The position must still publish SOMETHING. Without
                // this, "not a fold" is also satisfied by publishing
                // nothing, so a control that lost its type entirely passes.
                assert_ne!(
                    row.shape,
                    ShapeTag::Absent,
                    "{} [{mode}]: prop `{}` is a NEGATIVE CONTROL and must still \
                     publish a materialized type through its own route — publishing \
                     nothing is a loss, not a preserved route",
                    cell.id,
                    expectation.name,
                );
                // (2) ...and it must not be the shape the fold produces.
                assert!(
                    !matches!(row.shape, ShapeTag::Fold(_)),
                    "{} [{mode}]: prop `{}` is a NEGATIVE CONTROL — it carries no \
                     closed primitive fact and must stay on its own route. The \
                     runtime-constructor fold captured it and published a primitive \
                     shape: {:?}",
                    cell.id,
                    expectation.name,
                    row.shape,
                );
                // (3) ...and it must be EXACTLY the shape this control
                // names. A control that silently degraded from a resolved
                // object into `unknown` (or the reverse) satisfies (1) and
                // (2) and would otherwise stay green.
                assert_eq!(
                    row.shape,
                    want.tag(),
                    "{} [{mode}]: prop `{}` is a NEGATIVE CONTROL whose published \
                     shape is pinned. It is neither folded nor absent, but it is no \
                     longer the shape its route produces. If this change is \
                     intended, update the pin and say why",
                    cell.id,
                    expectation.name,
                );
            }
        }

        assert_eq!(
            row.required, expectation.required,
            "{} [{mode}]: prop `{}` optionality must be exactly as authored — \
             `required` is computed independently of the type fold and must not \
             drift with it",
            cell.id, expectation.name,
        );
        assert_eq!(
            row.has_default, expectation.has_default,
            "{} [{mode}]: prop `{}` default presence must be exactly as authored",
            cell.id, expectation.name,
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Exactness: every cell, every prop, cold through the base session
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn every_matrix_cell_publishes_its_exact_runtime_constructor_type() {
    for cell in CELLS {
        // A FRESH project per cell — this is the COLD invocation arm: the
        // first resolve of a canonical in a host that has never seen it.
        let project = make_project();
        load(&project, cell);
        assert_published(cell, &publish(project.host(), cell.id), "cold");
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Invocation invariance: cold ≡ warm ≡ sequential ≡ concurrent ≡ batch
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn matrix_results_are_identical_cold_and_warm() {
    for cell in CELLS {
        let project = make_project();
        load(&project, cell);
        let cold = publish(project.host(), cell.id);
        let warm = publish(project.host(), cell.id);
        assert_eq!(
            cold, warm,
            "{}: a warm resolve must publish the IDENTICAL runtime-constructor \
             surface as the cold one — a cache that answers differently on the \
             second read is a defect even when both answers are individually \
             plausible",
            cell.id
        );
        // Not just self-consistent: still EXACTLY right on the warm read.
        assert_published(cell, &warm, "warm");
    }
}

#[test]
fn matrix_results_are_identical_sequential_and_concurrent() {
    // Sequential: every cell loaded into ONE project, resolved in order.
    let sequential_project = make_project();
    for cell in CELLS {
        load(&sequential_project, cell);
    }
    let sequential: Vec<Vec<Published>> = CELLS
        .iter()
        .map(|cell| publish(sequential_project.host(), cell.id))
        .collect();
    for (cell, published) in CELLS.iter().zip(&sequential) {
        assert_published(cell, published, "sequential");
    }

    // Concurrent: the `Promise.all`-equivalent — a fresh project, every cell
    // resolved from its own thread with no ordering between them.
    let concurrent_project = make_project();
    for cell in CELLS {
        load(&concurrent_project, cell);
    }
    let concurrent: Vec<Vec<Published>> = std::thread::scope(|scope| {
        let handles: Vec<_> = CELLS
            .iter()
            .map(|cell| {
                let project = Arc::clone(&concurrent_project);
                scope.spawn(move || publish(project.host(), cell.id))
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().expect("concurrent resolve must not panic"))
            .collect()
    });

    for ((cell, seq), conc) in CELLS.iter().zip(&sequential).zip(&concurrent) {
        // Both halves: the concurrent surface's own exactness, AND agreement
        // with sequential. Agreement alone passes on a uniformly wrong answer.
        assert_published(cell, conc, "concurrent");
        assert_eq!(
            seq, conc,
            "{}: concurrent resolution must publish the IDENTICAL surface as \
             sequential resolution — a divergence here is a view/cache race, not a \
             different-but-valid answer",
            cell.id
        );
    }
}

#[test]
fn matrix_results_are_identical_scalar_and_batch() {
    let project = make_project();
    for cell in CELLS {
        load(&project, cell);
    }
    let scalar: Vec<Vec<Published>> = CELLS
        .iter()
        .map(|cell| publish(project.host(), cell.id))
        .collect();

    let ids: Vec<String> = CELLS.iter().map(|cell| cell.id.to_string()).collect();
    // The batch surface is session-owned; a plain session over the base
    // project sees exactly the base files upserted above, so scalar and
    // batch read the SAME sources.
    let session = project.open_session().expect("batch session opens");
    let batched = session
        .get_component_meta_output_batch(&ids)
        .expect("the batch surface must not fail for a runtime-constructor matrix");
    assert_eq!(
        batched.len(),
        CELLS.len(),
        "the batch surface returns one slot per input, in input order"
    );

    for ((cell, scalar_props), slot) in CELLS.iter().zip(&scalar).zip(batched) {
        let output = slot
            .unwrap_or_else(|error| {
                panic!(
                    "{}: the batch slot carries a typed failure the scalar surface \
                     did not produce — scalar and batch must agree: {error:?}",
                    cell.id
                )
            })
            .unwrap_or_else(|| {
                panic!(
                    "{}: the batch slot is the missing-canonical sentinel, but the \
                     canonical was upserted — a batch failure must never collapse onto \
                     absence",
                    cell.id
                )
            });
        let batch_props = rows(output);
        assert_eq!(
            *scalar_props, batch_props,
            "{}: the batch surface must publish the IDENTICAL runtime-constructor \
             surface as the scalar surface (scalar ≡ batch)",
            cell.id
        );
        // Agreement alone is not enough: two modes can agree on a WRONG
        // answer. Pin the batch surface's own exactness as well.
        assert_published(cell, &batch_props, "batch");
    }
    session.close();
}

// ─────────────────────────────────────────────────────────────────────────
// Request-view scope × invocation: the overlay lane runs the SAME cross
// ─────────────────────────────────────────────────────────────────────────

/// The overlay canonical for a cell — a path the base store has never seen,
/// so the answer can only come from the overlay view. It stays in the cell's
/// OWN directory so a cell's relative imports resolve to the same sibling
/// modules the base lane reads.
fn overlay_id(cell: &Cell) -> String {
    format!("/rcm/overlay-{}", cell.id.trim_start_matches("/rcm/"))
}

/// The full cross: for EVERY cell, the overlay lane is exercised cold, warm,
/// sequential, `Promise.all`-equivalent concurrent AND batch, and every one
/// of those must agree with the base lane's answer.
///
/// This is the axis the base-lane tests above cannot cover: a result that is
/// correct in the base store but wrong (or mode-dependent) under an overlay
/// request view is exactly the request-view defect class the acceptance
/// criteria call out, and it is invisible to a base-only cross.
#[test]
fn matrix_overlay_lane_runs_the_full_invocation_cross() {
    let project = make_project();
    for cell in CELLS {
        load(&project, cell);
    }
    // The base answer every overlay mode must reproduce.
    let base: Vec<Vec<Published>> = CELLS
        .iter()
        .map(|cell| publish(project.host(), cell.id))
        .collect();

    let session = project.open_session().expect("overlay session opens");
    let ids: Vec<String> = CELLS.iter().map(overlay_id).collect();
    for (cell, id) in CELLS.iter().zip(&ids) {
        session
            .upsert(id, cell.source.to_string())
            .expect("overlay upsert");
    }

    // ── overlay × cold, then overlay × warm (SAME session, resolved twice) ──
    let overlay_cold: Vec<Vec<Published>> =
        ids.iter().map(|id| publish_overlay(&session, id)).collect();
    let overlay_warm: Vec<Vec<Published>> =
        ids.iter().map(|id| publish_overlay(&session, id)).collect();

    // ── overlay × concurrent ──
    let overlay_concurrent: Vec<Vec<Published>> = std::thread::scope(|scope| {
        let handles: Vec<_> = ids
            .iter()
            .map(|id| {
                let session = &session;
                scope.spawn(move || publish_overlay(session, id))
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().expect("concurrent overlay resolve must not panic"))
            .collect()
    });

    // ── overlay × batch ──
    let batched = session
        .get_component_meta_output_batch(&ids)
        .expect("the overlay batch surface must not fail");
    assert_eq!(batched.len(), CELLS.len(), "one slot per input, in order");
    let overlay_batch: Vec<Vec<Published>> = batched
        .into_iter()
        .zip(CELLS)
        .map(|(slot, cell)| {
            let output = slot
                .unwrap_or_else(|error| {
                    panic!("{} [overlay/batch]: typed failure: {error:?}", cell.id)
                })
                .unwrap_or_else(|| {
                    panic!(
                        "{} [overlay/batch]: missing-canonical sentinel for an \
                         upserted overlay",
                        cell.id
                    )
                });
            rows(output)
        })
        .collect();

    for (index, cell) in CELLS.iter().enumerate() {
        for (mode, observed) in [
            ("overlay/cold", &overlay_cold[index]),
            ("overlay/warm", &overlay_warm[index]),
            ("overlay/concurrent", &overlay_concurrent[index]),
            ("overlay/batch", &overlay_batch[index]),
        ] {
            // Exact per-cell outcome in the overlay lane...
            assert_published(cell, observed, mode);
            // ...AND agreement with the base lane. Both, because either
            // alone can hold while the other fails.
            assert_eq!(
                &base[index], observed,
                "{}: the {mode} surface must be IDENTICAL to the base-scoped \
                 surface — a request-view-dependent runtime-constructor answer is \
                 a defect even when each view is individually plausible",
                cell.id
            );
        }
    }
    session.close();
}

/// What the barrel cell must publish once the session REDIRECTS its barrel.
/// The member keys differ from the base module's on purpose: a route walk
/// that read base state cannot produce these.
const OVERLAY_REDIRECTED_BARREL_PROPS: &[PropExpectation] = &[
    prop(
        "label",
        Expect::NotFolded(NegShape::ObjectKeys(&["overlaid"])),
        false,
        false,
    ),
    prop(
        "thing",
        Expect::NotFolded(NegShape::ObjectKeys(&["rerouted"])),
        false,
        false,
    ),
    number("count", false, false),
];

/// The overlay cross above cannot see a request-view hole in the EXPORT
/// ROUTE: it overlays only session-only SFC copies while every dependency
/// (the barrel included) is byte-identical in both views, so a route walk
/// that ignored the session's view would still agree with the base lane.
///
/// This test makes the views genuinely DISAGREE at the route itself. The
/// base store keeps the barrel cell exactly as authored; the session then
/// upserts, at the SAME barrel canonical, a redirect to a session-only
/// module whose members carry DIFFERENT keys. The SFC canonical is never
/// overlaid — the only edit is on the export route, and only in the
/// session's view.
///
/// Both halves are asserted, because either alone is weak:
///
/// 1. the session read publishes EXACTLY the redirected shapes — a walk
///    that resolved the barrel through the base host returns the base
///    module's keys here and fails;
/// 2. the base lane still publishes EXACTLY the base shapes, before AND
///    after the session read — so the two views demonstrably diverge and
///    the overlay never leaks into the base store. Without this half, a
///    uniformly-redirected answer in both views would pass (1) while
///    proving nothing about view separation.
#[test]
fn overlay_redirected_barrel_resolves_the_export_route_through_the_session_view() {
    let cell = CELLS
        .iter()
        .find(|cell| cell.id == "/rcm/ImportedBarrel.vue")
        .expect("the barrel cell is part of the matrix");

    let project = make_project();
    load(&project, cell);

    // The base answer: the barrel routes to `imported-ctors`.
    let base = publish(project.host(), cell.id);
    assert_published(cell, &base, "base/before-overlay");

    let session = project.open_session().expect("overlay session opens");
    session
        .upsert(
            "/rcm/overlaid-ctors.ts",
            "export const String = { overlaid: 1 }\nexport const Shadow = { rerouted: 2 }\n"
                .to_string(),
        )
        .expect("overlay module upsert");
    session
        .upsert(
            "/rcm/imported-barrel.ts",
            "export { String, Shadow } from './overlaid-ctors'\n".to_string(),
        )
        .expect("overlay barrel upsert");

    let overlay = publish_overlay(&session, cell.id);

    // (1) The session read follows the redirected route exactly.
    let redirected_cell = Cell {
        id: cell.id,
        source: cell.source,
        deps: cell.deps,
        props: OVERLAY_REDIRECTED_BARREL_PROPS,
    };
    assert_published(&redirected_cell, &overlay, "overlay/redirected-barrel");

    // (2) The redirected props genuinely DIVERGE between the views...
    for name in ["label", "thing"] {
        let base_row = base
            .iter()
            .find(|row| row.name == name)
            .expect("base publishes the prop");
        let overlay_row = overlay
            .iter()
            .find(|row| row.name == name)
            .expect("overlay publishes the prop");
        assert_ne!(
            base_row.shape, overlay_row.shape,
            "{}: prop `{name}` must resolve to DIFFERENT shapes in the base and \
             session views — agreement here means the redirect was never followed \
             and this test lost its discrimination",
            cell.id,
        );
    }

    // ...and the base lane still publishes the base route AFTER the session
    // read: the redirect stays session-scoped, never leaking into the base
    // store or its caches.
    let base_after = publish(project.host(), cell.id);
    assert_published(cell, &base_after, "base/after-overlay");
    assert_eq!(
        base, base_after,
        "{}: the base surface must be byte-for-byte unchanged by the session's \
         redirected resolve — an overlay that warms base state is a leak",
        cell.id,
    );

    session.close();
}

// ─────────────────────────────────────────────────────────────────────────
// Fail-closed positions: the exact typed failure, where failure is CORRECT
// ─────────────────────────────────────────────────────────────────────────

/// A verdict-bearing exact-variant assertion on the typed output failure.
///
/// This is the discrimination that a regression test for a REPAIRED defect
/// cannot provide. On an input whose correct answer is success, every `Err`
/// is red whatever it carries, so no check on the lane or variant can change
/// the verdict — `exposed_binding_regression.rs` says so about itself.
///
/// These two inputs are different: a typed failure is the CORRECT answer, so
/// pinning the exact `(lane, failure)` pair genuinely discriminates. A
/// different lane, a different `ComponentMetaOutputFailure`, a success, or a
/// collapse into absence each fail this test.
///
/// - A NAMESPACE import binds the module object, not one exported value, so
///   it has no single declaration body to anchor a locator to. The
///   import-origin resolver refuses it explicitly rather than anchoring on a
///   body that does not exist.
/// - An import whose module does not resolve has no declaring module to
///   re-anchor onto at all.
///
/// In both cases the honest outcome is the typed failure, never a fabricated
/// `unknown` and never `Ok(None)`.
#[test]
fn fail_closed_constructor_positions_surface_the_exact_typed_failure() {
    use verter_session::meta_resolve::{ComponentMetaOutputFailure, ComponentMetaOutputLane};

    struct FailCase {
        id: &'static str,
        source: &'static str,
        deps: &'static [(&'static str, &'static str)],
        why: &'static str,
    }

    const CASES: &[FailCase] = &[
        FailCase {
            id: "/rcm/NamespaceCtor.vue",
            source: r#"<script setup lang="ts">
import * as NS from './ns-src'
defineProps({ label: NS })
</script>
<template><div /></template>"#,
            deps: &[("/rcm/ns-src.ts", "export const a = 1\n")],
            why: "a namespace import binds the module object, not one exported value",
        },
        FailCase {
            id: "/rcm/UnresolvableCtor.vue",
            source: r#"<script setup lang="ts">
import { Nope } from './definitely-not-here'
defineProps({ label: Nope })
</script>
<template><div /></template>"#,
            deps: &[],
            why: "the imported module does not resolve, so there is no declaring module",
        },
    ];

    for case in CASES {
        let project = make_project();
        for (id, source) in case.deps {
            project.upsert_base(id, source).unwrap();
        }
        project.upsert_base(case.id, case.source).unwrap();

        match project.host().get_component_meta_output(case.id) {
            Err(error) => {
                assert_eq!(
                    error.lane,
                    ComponentMetaOutputLane::Prop,
                    "{}: {} — the failure belongs to the PROP lane; another lane is a \
                     different defect, got {error:?}",
                    case.id,
                    case.why,
                );
                assert_eq!(
                    error.failure,
                    ComponentMetaOutputFailure::UnraisableSource,
                    "{}: {} — the source has no live graph representation, which is \
                     exactly `UnraisableSource`. A different typed failure means the \
                     position failed for a different reason and must not be reported \
                     as this one, got {error:?}",
                    case.id,
                    case.why,
                );
            }
            Ok(Some(_)) => panic!(
                "{}: {} — this position must FAIL CLOSED. Publishing a type here means \
                 something was invented for a binding with no declaration body",
                case.id, case.why,
            ),
            Ok(None) => panic!(
                "{}: the component collapsed to ABSENCE. A fail-closed position must \
                 surface its typed failure as `Err`, never as the same result a missing \
                 canonical produces",
                case.id,
            ),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Owner propagation: the re-anchored slot carries the TARGET's real owner
// ─────────────────────────────────────────────────────────────────────────

/// The re-anchored locator must carry the owner the export route RESOLVED,
/// never a fabricated default.
///
/// Every other imported-binding cell in this file happens to resolve to a
/// plain `.ts` module, whose top-level owner IS `ordinary_file()`
/// (`Module(0)`). That makes those cells blind to a producer that throws the
/// resolved owner away and stamps `ordinary_file()` unconditionally: the
/// fabricated value and the real one coincide, so the suite stays green
/// under exactly the defect the owner column exists to prevent.
///
/// A `.vue` component's default export does not coincide. Its value symbol
/// is synthesised on the INSTANCE owner, so the resolved owner is
/// `Instance(0)` and a fabricated `ordinary_file()` is observably wrong.
///
/// The position itself fails closed here — a component default has no
/// raisable value body to anchor a constructor position to, which is the
/// honest outcome for `defineProps({ label: SomeComponent })` — but the
/// typed failure still carries the locator it built, so the ANCHOR is
/// observable independently of whether the body raised. That is what this
/// test reads.
#[test]
fn reanchored_slot_carries_the_resolved_owner_not_a_fabricated_default() {
    let project = make_project();
    project
        .upsert_base(
            "/rcm/Widget.vue",
            r#"<script setup lang="ts">
defineProps({ a: String })
</script>
<template><i /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/rcm/VueDefaultCtor.vue",
            r#"<script setup lang="ts">
import Widget from './Widget.vue'
defineProps({ label: Widget })
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let error = project
        .host()
        .get_component_meta_output("/rcm/VueDefaultCtor.vue")
        .expect_err(
            "a component default has no raisable value body, so this constructor \
             position must fail closed rather than publish something invented",
        );

    let verter_type_expr::facts::SourcePosition::Present(
        verter_type_expr::facts::SemanticTypeSource::Authored(
            verter_type_expr::locators::AuthoredBodyLocator::DeclBody(slot),
        ),
    ) = error.position.as_ref()
    else {
        panic!(
            "the failure must still carry the authored decl-body locator it built — \
             without it the anchor is unobservable, got {:?}",
            error.position
        );
    };

    assert_eq!(
        slot.anchor.canonical_id.as_ref(),
        "/rcm/Widget.vue",
        "the anchor names the exporting component, got {slot:?}"
    );
    assert_eq!(
        slot.anchor.symbol.as_ref(),
        "default",
        "the anchor names the exported symbol, got {slot:?}"
    );
    assert_eq!(
        slot.anchor.owner,
        verter_type_expr::TopLevelOwnerId::instance(0),
        "THE POINT OF THIS TEST: a `.vue` component default is synthesised on the \
         INSTANCE owner, so the re-anchored slot must carry `Instance(0)` — the \
         owner the export route resolved. A producer that discards the resolved \
         owner and stamps `ordinary_file()` lands on `Module(0)` here, and every \
         other imported cell in this file would stay green while it did, got {slot:?}"
    );
}
