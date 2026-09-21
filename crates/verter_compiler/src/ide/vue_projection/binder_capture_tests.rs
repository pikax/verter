use super::binder_capture::*;
use super::script_setup::{ScriptBlockInput, SetupProjectionRefusal, SourceRange};
use crate::cursor::ScriptLanguage;

fn ts(content: &str, start: u32) -> ScriptBlockInput<'_> {
    ScriptBlockInput {
        content,
        content_start: start,
        lang: Some(ScriptLanguage::TypeScript),
    }
}

fn plan(setup: &str, generic: Option<&str>) -> BinderCapturePlan {
    capture_binder_plan(None, Some(ts(setup, 0)), generic).expect("captures")
}

fn slice_of(plan: &BinderCapturePlan, root: PublicTypeRoot) -> &PublicTypeDependencySlice {
    plan.slices
        .iter()
        .find(|slice| slice.root == root)
        .expect("slice")
}

fn names(dependencies: &[CapturedDependency]) -> Vec<&str> {
    dependencies.iter().map(|d| d.name.as_str()).collect()
}

fn text(source: &str, span: SourceRange) -> &str {
    &source[span.start as usize..span.end as usize]
}

fn binder_param<'p>(
    declaration: &'p LiftedSourceDeclaration,
    source_name: &str,
) -> &'p LiftedBinderParam {
    declaration
        .binder_params
        .iter()
        .find(|param| param.source_name == source_name)
        .expect("binder param")
}

#[test]
fn stp14_local_capture_keeps_binder_bound_local_selection_in_the_public_surface() {
    let setup = "type Selection = { item: T; key: T[\"id\"] }\n\
                 defineProps<{ selected: Selection }>()";
    let captured = plan(setup, Some("T extends { id: number }"));

    let props = slice_of(&captured, PublicTypeRoot::Props);
    assert_eq!(names(&props.dependencies), ["Selection"]);
    assert_eq!(
        props.dependencies[0].origin.clone(),
        DependencyOrigin::LocalDeclaration { index: 0 }
    );
    assert_eq!(props.dependencies[0].space, DependencySpace::Type);
    assert_eq!(text(setup, props.expression), "{ selected: Selection }");

    let selection = captured.declaration("Selection").expect("lifted Selection");
    assert_eq!(selection.kind, LiftedDeclarationKind::TypeAlias);
    assert!(!selection.exported, "authored visibility is preserved");
    assert!(selection.from_setup, "origin block is preserved");
    // The declaration is free in the binder, so lifting it to module scope
    // re-parameterizes it; the body is never expanded to reach that answer.
    assert_eq!(
        selection
            .binder_params
            .iter()
            .map(|param| (param.ordinal, param.emitted_name.as_str()))
            .collect::<Vec<_>>(),
        [(0, "T")]
    );
    // Both `T` occurrences are reported so an emitter can rewrite each site.
    let sites: Vec<&str> = selection
        .binder_references
        .iter()
        .map(|reference| text(setup, reference.span))
        .collect();
    assert_eq!(sites, ["T", "T"]);
    assert_eq!(selection.binder_references[0].ordinal, 0);
    assert!(captured.cycles.is_empty());
    assert!(captured.duplicates.is_empty());
}

#[test]
fn stp14_local_capture_leaves_a_shadowed_binder_name_bound_by_the_inner_binder() {
    // The inner `<T>` of the function type shadows the component binder, so
    // `Wrapped` is not free in the binder and needs no re-parameterization.
    let setup = "type Wrapped = { make: <T>(value: T) => T }\n\
                 defineProps<{ wrapped: Wrapped }>()";
    let captured = plan(setup, Some("T extends { id: number }"));
    let wrapped = captured.declaration("Wrapped").expect("lifted Wrapped");
    assert!(wrapped.binder_params.is_empty());
    assert!(wrapped.binder_references.is_empty());
}

#[test]
fn stp14_local_capture_renames_a_binder_parameter_that_module_scope_already_binds() {
    // The normal script owns a module-scope `T`. Lifting the setup-local
    // `Selection` to module scope must not let its `T` capture that
    // declaration, so the introduced parameter is alpha-renamed.
    let normal = "export type T = { id: string }";
    let setup = "type Selection = { item: T }\ndefineProps<{ selected: Selection }>()";
    let captured = capture_binder_plan(
        Some(ts(normal, 0)),
        Some(ts(setup, 400)),
        Some("T extends { id: number }"),
    )
    .expect("captures");

    let selection = captured.declaration("Selection").expect("lifted Selection");
    let param = binder_param(selection, "T");
    assert_eq!(param.emitted_name, "T_1");
    assert!(param.is_renamed());
    assert_eq!(selection.binder_references.len(), 1);

    // The normal-script declaration is outside the binder scope, so its own
    // `T` still means the module-scope declaration, not the binder.
    assert!(captured.duplicates.is_empty());
}

#[test]
fn stp14_dependent_default_keeps_the_earlier_parameter_bound() {
    // `Row` is free only in `B`, but `B`'s default refers to `A`: lifting
    // `Row` must carry `A` too, in authored order.
    let setup = "type Row = { value: B }\ndefineProps<{ row: Row }>()";
    let captured = plan(setup, Some("A extends { id: number }, B = A[]"));

    assert_eq!(captured.binder_param_dependencies, [vec![], vec![0]]);
    assert_eq!(captured.binder_closure(&[1]), [0, 1]);

    let row = captured.declaration("Row").expect("lifted Row");
    assert_eq!(
        row.binder_params
            .iter()
            .map(|param| (param.ordinal, param.source_name.as_str()))
            .collect::<Vec<_>>(),
        [(0, "A"), (1, "B")]
    );
    // Only `B` is referenced directly; `A` arrives through the default.
    assert_eq!(
        row.binder_references
            .iter()
            .map(|reference| reference.ordinal)
            .collect::<Vec<_>>(),
        [1]
    );
}

#[test]
fn stp14_dependent_default_only_binds_parameters_declared_before_it() {
    // A parameter cannot refer forward, so a same-named later parameter is
    // not a dependency edge.
    let captured = plan("const a = 1", Some("A extends B, B = string"));
    assert_eq!(
        captured.binder_param_dependencies,
        [Vec::<usize>::new(), Vec::new()]
    );
}

#[test]
fn stp14_dependent_default_closure_carries_a_transitive_constraint() {
    let setup = "type Row = { value: C }\ndefineProps<{ row: Row }>()";
    let captured = plan(setup, Some("A, B extends A, C = B"));
    assert_eq!(captured.binder_closure(&[2]), [0, 1, 2]);
    let row = captured.declaration("Row").expect("lifted Row");
    assert_eq!(
        row.binder_params
            .iter()
            .map(|param| param.ordinal)
            .collect::<Vec<_>>(),
        [0, 1, 2]
    );
}

#[test]
fn stp14_typeof_capture_names_value_space_dependencies_in_the_emitted_declaration() {
    let setup = "declare const marker: unique symbol\n\
                 class Row { id = 1 }\n\
                 function make(): number { return 1 }\n\
                 defineProps<{ key: typeof marker; row: typeof Row; made: typeof make }>()";
    let captured = plan(setup, None);

    let props = slice_of(&captured, PublicTypeRoot::Props);
    assert_eq!(names(&props.dependencies), ["marker", "Row", "make"]);
    assert!(props
        .dependencies
        .iter()
        .all(|dependency| dependency.space == DependencySpace::Value));

    let marker = captured.declaration("marker").expect("lifted marker");
    assert_eq!(
        marker.kind,
        LiftedDeclarationKind::Variable {
            unique_symbol: true
        }
    );
    assert!(marker.kind.occupies_value_space());
    assert!(!marker.kind.occupies_type_space());

    let row = captured.declaration("Row").expect("lifted Row");
    assert_eq!(row.kind, LiftedDeclarationKind::Class);
    assert!(row.kind.occupies_type_space() && row.kind.occupies_value_space());

    assert_eq!(
        captured.declaration("make").expect("lifted make").kind,
        LiftedDeclarationKind::Function
    );
}

#[test]
fn stp14_typeof_capture_separates_type_space_from_value_space_for_one_name() {
    let setup = "class Row { id = 1 }\ndefineProps<{ a: Row; b: typeof Row }>()";
    let captured = plan(setup, None);
    let props = slice_of(&captured, PublicTypeRoot::Props);
    assert_eq!(
        props
            .dependencies
            .iter()
            .map(|dependency| (dependency.name.as_str(), dependency.space))
            .collect::<Vec<_>>(),
        [
            ("Row", DependencySpace::Type),
            ("Row", DependencySpace::Value)
        ]
    );
    assert_eq!(captured.declarations.len(), 1);
}

#[test]
fn stp14_typeof_capture_reaches_exposed_runtime_bindings() {
    let setup = "class Row { id = 1 }\nconst rows: Row[] = []\ndefineExpose({ rows, Row })";
    let captured = plan(setup, None);
    let exposed = slice_of(&captured, PublicTypeRoot::Expose);
    assert_eq!(names(&exposed.dependencies), ["rows", "Row"]);
    assert!(captured.declaration("rows").is_some());
    assert!(captured.declaration("Row").is_some());
}

#[test]
fn stp14_typeof_capture_keeps_imported_dependencies_as_imports() {
    let setup = "import type { Far } from './far'\n\
                 type Near = { far: Far }\n\
                 defineProps<{ near: Near }>()";
    let captured = plan(setup, None);
    let near = captured.declaration("Near").expect("lifted Near");
    assert_eq!(
        near.dependencies[0].origin.clone(),
        DependencyOrigin::Import { index: 0 }
    );
    assert_eq!(captured.imports[0].specifier, "./far");
    assert!(captured.imports[0].type_only);
    // An imported name is never followed into another file, so it is never
    // lifted as a local declaration.
    assert!(captured.declaration("Far").is_none());
}

#[test]
fn stp14_alias_cycle_terminates_and_is_recorded_by_declaration_identity() {
    let setup = "type A = { b: B }\ntype B = { a: A }\ndefineProps<{ a: A }>()";
    let captured = plan(setup, None);

    // Each member is visited exactly once: the closure is a worklist over
    // declaration identity, not an eager expansion.
    assert_eq!(
        captured
            .declarations
            .iter()
            .map(|declaration| declaration.name.as_str())
            .collect::<Vec<_>>(),
        ["A", "B"]
    );
    assert_eq!(captured.cycles.len(), 1);
    assert_eq!(captured.cycles[0].members, ["A", "B"]);
}

#[test]
fn stp14_alias_cycle_records_a_self_referential_alias_without_expanding_it() {
    let setup = "type Tree = { child: Tree | null }\ndefineProps<{ tree: Tree }>()";
    let captured = plan(setup, None);
    assert_eq!(captured.declarations.len(), 1);
    assert_eq!(captured.cycles[0].members, ["Tree"]);
}

#[test]
fn stp14_alias_cycle_through_an_import_is_not_followed_into_another_file() {
    let setup = "import type { Outer } from './outer'\n\
                 type Inner = { outer: Outer }\n\
                 defineProps<{ inner: Inner }>()";
    let captured = plan(setup, None);
    assert!(captured.cycles.is_empty());
    assert_eq!(captured.declarations.len(), 1);
}

#[test]
fn stp14_duplicate_error_lifts_nothing_and_preserves_every_origin() {
    // Both blocks share one module scope, so declaring `Row` twice is a real
    // authored error. Lifting either one would hide it; lifting both would
    // invent a second conflicting declaration.
    let normal = "type Row = { id: number }";
    let setup = "type Row = { id: string }\ndefineProps<{ row: Row }>()";
    let captured =
        capture_binder_plan(Some(ts(normal, 0)), Some(ts(setup, 400)), None).expect("captures");

    let props = slice_of(&captured, PublicTypeRoot::Props);
    assert_eq!(props.dependencies[0].origin, DependencyOrigin::Duplicate);
    assert!(captured.declaration("Row").is_none());
    assert!(captured.declarations.is_empty());

    assert_eq!(captured.duplicates.len(), 1);
    assert_eq!(captured.duplicates[0].name, "Row");
    let origins: Vec<u32> = captured.duplicates[0]
        .origins
        .iter()
        .map(|span| span.start)
        .collect();
    assert_eq!(origins, [0, 400]);
}

#[test]
fn stp14_duplicate_error_covers_an_import_colliding_with_a_declaration() {
    let setup = "import type { Row } from './row'\n\
                 type Row = { id: number }\n\
                 defineProps<{ row: Row }>()";
    let captured = plan(setup, None);
    let props = slice_of(&captured, PublicTypeRoot::Props);
    assert_eq!(props.dependencies[0].origin, DependencyOrigin::Duplicate);
    assert!(captured.declarations.is_empty());
    assert_eq!(captured.duplicates[0].name, "Row");
    assert_eq!(captured.duplicates[0].origins.len(), 2);
}

#[test]
fn stp14_duplicate_error_does_not_fire_for_a_name_bound_once() {
    let normal = "type Other = { id: number }";
    let setup = "type Row = { other: Other }\ndefineProps<{ row: Row }>()";
    let captured =
        capture_binder_plan(Some(ts(normal, 0)), Some(ts(setup, 400)), None).expect("captures");
    assert!(captured.duplicates.is_empty());
    assert_eq!(
        captured
            .declarations
            .iter()
            .map(|declaration| (declaration.name.as_str(), declaration.from_setup))
            .collect::<Vec<_>>(),
        [("Row", true), ("Other", false)]
    );
    assert!(!captured.declaration("Other").expect("Other").exported);
}

#[test]
fn stp14_capture_reaches_every_type_argument_macro_and_ignores_ordinary_calls() {
    let setup = "type P = { a: number }\n\
                 type E = { (e: 'x'): void }\n\
                 type S = { default(): unknown }\n\
                 type M = { v: number }\n\
                 function defineOptions() { return 0 }\n\
                 type Hidden = { h: number }\n\
                 defineProps<P>()\n\
                 defineEmits<E>()\n\
                 defineSlots<S>()\n\
                 defineModel<M>()\n\
                 defineOptions<Hidden>()";
    let captured = plan(setup, None);
    assert_eq!(
        captured
            .slices
            .iter()
            .map(|slice| slice.root)
            .collect::<Vec<_>>(),
        [
            PublicTypeRoot::Props,
            PublicTypeRoot::Emits,
            PublicTypeRoot::Slots,
            PublicTypeRoot::Model
        ]
    );
    // A setup-local function of the same name is an ordinary call, so its
    // type argument is not a public surface.
    assert!(captured.declaration("Hidden").is_none());
}

#[test]
fn stp14_capture_unwraps_with_defaults_to_the_props_type_argument() {
    let setup = "type P = { a?: number }\nwithDefaults(defineProps<P>(), { a: 1 })";
    let captured = plan(setup, None);
    let props = slice_of(&captured, PublicTypeRoot::Props);
    assert_eq!(names(&props.dependencies), ["P"]);
}

#[test]
fn stp14_capture_lifts_only_the_declarations_the_public_surface_reaches() {
    let setup = "type Used = { a: number }\n\
                 type Unused = { b: number }\n\
                 defineProps<{ used: Used }>()";
    let captured = plan(setup, None);
    assert_eq!(
        captured
            .declarations
            .iter()
            .map(|declaration| declaration.name.as_str())
            .collect::<Vec<_>>(),
        ["Used"]
    );
}

#[test]
fn stp14_capture_reads_a_class_shape_without_reading_method_bodies() {
    let setup = "type Internal = { hidden: number }\n\
                 type Public = { shown: number }\n\
                 class Row {\n  field: Public | null = null\n  method(): void { const x: Internal = { hidden: 1 }; void x }\n}\n\
                 defineProps<{ row: typeof Row }>()";
    let captured = plan(setup, None);
    assert!(captured.declaration("Public").is_some());
    assert!(
        captured.declaration("Internal").is_none(),
        "a method body is not public surface"
    );
}

#[test]
fn stp14_capture_refuses_the_same_inputs_the_statement_projection_refuses() {
    assert_eq!(
        capture_binder_plan(None, Some(ts("const a = 1", 0)), Some("T extends")).unwrap_err(),
        SetupProjectionRefusal::InvalidGeneric
    );
    assert_eq!(
        capture_binder_plan(None, Some(ts("const a = (", 0)), None).unwrap_err(),
        SetupProjectionRefusal::SyntaxErrors { setup: true }
    );
    assert_eq!(
        capture_binder_plan(
            None,
            Some(ScriptBlockInput {
                content: "const a = 1",
                content_start: 0,
                lang: None,
            }),
            None,
        )
        .unwrap_err(),
        SetupProjectionRefusal::NotTypeScript
    );
    assert_eq!(
        capture_binder_plan(
            Some(ts("const a = 1", 0)),
            Some(ScriptBlockInput {
                content: "const b = 2",
                content_start: 40,
                lang: Some(ScriptLanguage::TSX),
            }),
            None,
        )
        .unwrap_err(),
        SetupProjectionRefusal::ScriptLangConflict
    );
}

#[test]
fn stp14_local_capture_keeps_the_binder_out_of_scope_in_the_normal_script() {
    // The `generic` binder scopes `<script setup>` only. A normal-script
    // declaration reached from the public surface therefore resolves `T` to
    // the module-scope declaration, and needs no binder parameter.
    let normal = "export type Wrapper = { value: T }\nexport type T = { id: string }";
    let setup = "defineProps<{ wrapper: Wrapper }>()";
    let captured = capture_binder_plan(
        Some(ts(normal, 0)),
        Some(ts(setup, 400)),
        Some("T extends { id: number }"),
    )
    .expect("captures");

    let wrapper = captured.declaration("Wrapper").expect("lifted Wrapper");
    assert!(!wrapper.from_setup);
    assert_eq!(
        wrapper.dependencies[0].origin.clone(),
        DependencyOrigin::LocalDeclaration { index: 1 }
    );
    assert!(
        wrapper.binder_params.is_empty(),
        "the binder does not scope the normal script"
    );
    assert!(wrapper.binder_references.is_empty());
    assert!(captured.declaration("T").is_some());
}

#[test]
fn stp14_local_capture_propagates_binder_requirements_across_lifted_declarations() {
    let setup = "type Inner = { item: T }\n\
                 type Outer = { inner: Inner }\n\
                 defineProps<{ outer: Outer }>()";
    let captured = plan(setup, Some("T"));

    for name in ["Inner", "Outer"] {
        assert_eq!(
            captured
                .declaration(name)
                .expect("lifted declaration")
                .binder_params
                .iter()
                .map(|param| param.source_name.as_str())
                .collect::<Vec<_>>(),
            ["T"],
            "{name} must be parameterized when it references a parameterized local declaration"
        );
    }
}

#[test]
fn stp14_local_capture_uses_individual_variable_declarator_spans() {
    let setup = "const helper = 0, marker: unique symbol = Symbol();\n\
                 defineProps<{ marker: typeof marker }>()";
    let captured = plan(setup, None);
    assert_eq!(captured.declarations.len(), 1);
    assert_eq!(captured.declarations[0].name, "marker");
    assert_eq!(
        text(setup, captured.declarations[0].span),
        "marker: unique symbol = Symbol()"
    );
}

#[test]
fn stp14_local_capture_lifts_interface_heritage_dependencies() {
    let setup = "interface Parent { id: number }\n\
                 interface Child extends Parent { name: string }\n\
                 defineProps<{ child: Child }>()";
    let captured = plan(setup, None);
    assert_eq!(
        captured
            .declarations
            .iter()
            .map(|declaration| declaration.name.as_str())
            .collect::<Vec<_>>(),
        ["Child", "Parent"]
    );
}

#[test]
fn stp14_local_capture_lifts_class_heritage_and_implements_dependencies() {
    let setup = "class Base {}\n\
                 interface Contract { id: number }\n\
                 class Derived extends Base implements Contract { id = 1 }\n\
                 defineProps<{ derived: typeof Derived }>()";
    let captured = plan(setup, None);
    assert_eq!(
        captured
            .declarations
            .iter()
            .map(|declaration| declaration.name.as_str())
            .collect::<Vec<_>>(),
        ["Derived", "Base", "Contract"]
    );
}

#[test]
fn stp14_local_capture_separates_type_and_value_namespaces_and_merges_interfaces() {
    let setup = "interface User { name: string }\n\
                 interface User { id: number }\n\
                 const User = { id: 1, name: 'one' };\n\
                 defineProps<{ type: User; value: typeof User }>()";
    let captured = plan(setup, None);
    let props = slice_of(&captured, PublicTypeRoot::Props);
    assert!(captured.duplicates.is_empty());
    assert_eq!(
        props
            .dependencies
            .iter()
            .map(|dependency| (
                dependency.name.as_str(),
                dependency.space,
                dependency.origin.clone()
            ))
            .collect::<Vec<_>>(),
        [
            (
                "User",
                DependencySpace::Type,
                DependencyOrigin::LocalDeclarations {
                    indices: vec![0, 1]
                },
            ),
            (
                "User",
                DependencySpace::Value,
                DependencyOrigin::LocalDeclaration { index: 2 },
            ),
        ]
    );
    assert_eq!(captured.declarations.len(), 3);
}

#[test]
fn stp14_local_capture_keeps_signature_type_parameters_bound() {
    let setup = "interface Service { run<T>(item: T): T; <T>(item: T): T; new <T>(item: T): T }\n\
                 defineProps<{ service: Service }>()";
    let captured = plan(setup, Some("T"));
    let service = captured.declaration("Service").expect("lifted Service");
    assert!(service.binder_params.is_empty());
    assert!(service.binder_references.is_empty());
}
