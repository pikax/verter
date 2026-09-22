use super::binding_views::*;
use super::script_setup::ScriptBlockInput;
use crate::cursor::ScriptLanguage;

fn block(content: &str) -> ScriptBlockInput<'_> {
    ScriptBlockInput {
        content,
        content_start: 0,
        lang: Some(ScriptLanguage::TypeScript),
    }
}

fn project_setup(content: &str) -> BindingViewsProjection {
    project_binding_views(None, Some(block(content)), None).expect("projects")
}

fn read_names(projection: &BindingViewsProjection) -> Vec<&str> {
    projection
        .read
        .bindings
        .iter()
        .map(|binding| binding.name.as_str())
        .collect()
}

const COUNT_SETUP: &str = r#"import { ref, computed } from 'vue';
const count = ref(0);
const nested = { c: ref(0) };
const doubled = computed(() => count.value * 2);
const label = computed({
  get: (): number => count.value,
  set: (v: string) => { count.value = Number(v); },
});
"#;

#[test]
fn stp15_read_ref_unwraps_top_level_ref_only() {
    let projection = project_setup(COUNT_SETUP);
    let count = projection.read.lookup("count").expect("count");
    assert_eq!(count.kind, BindingKind::Ref);
    assert!(
        count.unwrapped,
        "template reads the top-level ref unwrapped"
    );
    assert!(
        count.script_wraps_ref,
        "script still sees the Ref (.value) wrapper"
    );
    let nested = projection.read.lookup("nested").expect("nested");
    assert_eq!(nested.kind, BindingKind::Plain);
    assert!(
        !nested.unwrapped,
        "a ref nested in an object literal stays wrapped"
    );
    // Ranges point back at the authored declarator names.
    assert!(COUNT_SETUP[count.range.start as usize..count.range.end as usize] == *"count");
}

#[test]
fn stp15_readonly_write_rejects_getter_computed_and_readonly_prop() {
    let content = r#"import { computed, readonly } from 'vue';
const count = ref(0);
const doubled = computed(() => count.value * 2);
const { title } = defineProps<{ title: string }>();
const state = readonly({ mode: 'x' });
"#;
    let projection = project_setup(content);
    assert_eq!(
        projection.write.write_target("doubled"),
        Err(WriteRejection::GetterOnlyComputed)
    );
    assert_eq!(
        projection.write.write_target("title"),
        Err(WriteRejection::ReadonlyProp)
    );
    assert_eq!(
        projection.write.write_target("state"),
        Err(WriteRejection::ReadonlyProp)
    );
    assert_eq!(
        projection.write.write_target("missing"),
        Err(WriteRejection::UnknownBinding)
    );
    // No universal mutable alias: exactly the mutable population is writable.
    assert!(projection.write.write_target("count").is_ok());
    assert!(
        !projection
            .write
            .writable
            .iter()
            .any(|w| w.name == "doubled"),
        "getter-only computed must not appear in the writable rows"
    );
}

#[test]
fn stp15_setter_domain_uses_declared_setter_type() {
    let projection = project_setup(COUNT_SETUP);
    let label = projection.write.write_target("label").expect("writable");
    match &label.domain {
        WriteDomain::SetterParam(domain) => assert_eq!(
            domain, "string",
            "the write domain is the declared setter domain, not the number read type"
        ),
        other => panic!("writable computed must carry its setter domain, got {other:?}"),
    }
    let read = projection.read.lookup("label").expect("read");
    assert_eq!(read.kind, BindingKind::Computed { setter: true });
    assert!(!read.unwrapped);
}

#[test]
fn stp15_unused_reports_only_authored_references() {
    let projection = project_setup(COUNT_SETUP);
    let declared: Vec<String> = read_names(&projection)
        .into_iter()
        .map(str::to_string)
        .collect();
    // No references anywhere: nothing is used, nothing is hidden by
    // synthetic reads, and every declared binding surfaces as unused.
    let usage = BindingUsageSet::from_authored_references(&declared, &[], &[], &[]);
    assert!(usage.used.is_empty());
    assert_eq!(
        usage.unused(),
        declared.iter().map(String::as_str).collect::<Vec<_>>()
    );
    // Real references account per region; unknown names never join.
    let usage = BindingUsageSet::from_authored_references(
        &declared,
        &["count", "ghost"],
        &["label"],
        &["doubled"],
    );
    assert!(usage.is_used("count"));
    assert!(!usage.is_used("ghost"));
    assert!(!usage.is_used("nested"));
    // Import rows (`ref`, `computed`) are template-visible bindings too:
    // they stay in the declared population with no synthetic references.
    assert_eq!(usage.unused(), vec!["ref", "computed", "nested"]);
    let count = usage
        .used
        .iter()
        .find(|u| u.name == "count")
        .expect("count");
    assert!(count.regions.template && !count.regions.script && !count.regions.style);
}

#[test]
fn stp15_mutation_uses_live_views_without_snapshots() {
    let projection = project_setup(COUNT_SETUP);
    assert_eq!(projection.read.snapshot_kind(), ViewSnapshotKind::Live);
    // An authored script-side mutation is an ordinary script reference:
    // the live view keeps pointing at the source binding.
    let declared: Vec<String> = read_names(&projection)
        .into_iter()
        .map(str::to_string)
        .collect();
    let usage = BindingUsageSet::from_authored_references(&declared, &["count"], &["count"], &[]);
    let count = usage
        .used
        .iter()
        .find(|u| u.name == "count")
        .expect("count");
    assert!(count.regions.template && count.regions.script);
    assert!(usage.unused().contains(&"nested"));
}

#[test]
fn stp15_model_refs_and_reactive_members_stay_writable() {
    let content = r#"import { reactive } from 'vue';
const model = defineModel<string>();
const state = reactive({ items: [1], table: new Map() });
const { a, b } = toRefs(state);
const { x } = reactive({ x: 1 });
"#;
    let projection = project_setup(content);
    assert_eq!(
        projection
            .write
            .write_target("model")
            .expect("model")
            .domain,
        WriteDomain::ModelValue
    );
    assert!(projection.read.lookup("model").expect("model").unwrapped);
    assert_eq!(
        projection
            .write
            .write_target("state")
            .expect("state")
            .domain,
        WriteDomain::ReactiveMember
    );
    // `toRefs` members stay refs: unwrapped in the template, writable.
    for name in ["a", "b"] {
        let read = projection.read.lookup(name).expect(name);
        assert_eq!(read.kind, BindingKind::Ref);
        assert!(read.unwrapped);
        assert!(projection.write.write_target(name).is_ok());
    }
    // Plain destructuring out of `reactive(...)` copies: direct, not a ref.
    let x = projection.read.lookup("x").expect("x");
    assert_eq!(x.kind, BindingKind::Plain);
    assert!(!x.unwrapped);
}

#[test]
fn stp15_shadowed_factory_is_an_ordinary_binding() {
    let content = r#"function ref(value: number) { return value; }
const count = ref(0);
"#;
    let projection = project_setup(content);
    let count = projection.read.lookup("count").expect("count");
    assert_eq!(
        count.kind,
        BindingKind::Plain,
        "a setup-local `ref` shadows the factory: no unwrapping"
    );
    assert!(!count.unwrapped);
}

#[test]
fn stp15_destructured_reactive_props_read_directly() {
    let content = r#"const { title, initial = 0 } = defineProps<{ title: string; initial?: number }>();
"#;
    let projection = project_setup(content);
    for name in ["title", "initial"] {
        let read = projection.read.lookup(name).expect(name);
        assert_eq!(read.kind, BindingKind::Readonly);
        assert!(!read.unwrapped);
        assert_eq!(
            projection.write.write_target(name),
            Err(WriteRejection::ReadonlyProp)
        );
    }
}
