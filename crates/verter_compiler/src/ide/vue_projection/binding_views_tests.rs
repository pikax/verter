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

#[test]
fn stp15_setup_row_wins_over_options_prop() {
    let normal = block("export default { props: ['count'] };");
    let setup = block("import { ref } from 'vue';\nconst count = ref(0);\n");
    let projection = project_binding_views(Some(normal), Some(setup), None).expect("projects");
    let count = projection.read.lookup("count").expect("count");
    assert_eq!(
        count.kind,
        BindingKind::Ref,
        "the setup `ref` wins over the Options prop of the same name"
    );
    assert!(count.unwrapped);
    assert_eq!(
        projection
            .write
            .write_target("count")
            .expect("count")
            .domain,
        WriteDomain::RefValue
    );
    assert_eq!(
        projection
            .read
            .bindings
            .iter()
            .filter(|binding| binding.name == "count")
            .count(),
        1,
        "one earliest row is kept, not an Options shadow plus a setup row"
    );
}

#[test]
fn stp15_functions_and_imports_are_not_writable_targets() {
    let content = r#"import { ref } from 'vue';
import { store } from './store';
function increment(step: number) { return step + 1; }
const count = ref(0);
"#;
    let projection = project_setup(content);
    for name in ["increment", "ref", "store"] {
        assert!(
            projection.read.lookup(name).is_some(),
            "{name} stays a template-visible read"
        );
        assert_eq!(
            projection.write.write_target(name),
            Err(WriteRejection::UnknownBinding),
            "{name} is immutable in scope, never a writable assignment target"
        );
    }
    assert!(projection.write.write_target("count").is_ok());
}

#[test]
fn stp15_partially_bound_pattern_keeps_earliest_row() {
    let content = r#"import { ref, toRefs } from 'vue';
const { title } = defineProps<{ title: string }>();
const { title: _again, extra } = toRefs(state);
const count = ref(0);
"#;
    let projection = project_setup(content);
    let title_rows: Vec<_> = projection
        .read
        .bindings
        .iter()
        .filter(|binding| binding.name == "title")
        .collect();
    assert_eq!(title_rows.len(), 1, "no duplicate read row for title");
    assert_eq!(title_rows[0].kind, BindingKind::Readonly);
    assert_eq!(
        projection.write.write_target("title"),
        Err(WriteRejection::ReadonlyProp),
        "the earlier readonly classification is not overridden by the later pattern"
    );
    assert!(projection.read.lookup("extra").is_some());
    assert!(projection.read.lookup("count").is_some());
}

#[test]
fn stp15_renamed_destructured_props_register_locals() {
    let content = r#"const { title: heading, count } = defineProps<{ title: string; count: number }>();
"#;
    let projection = project_setup(content);
    let heading = projection.read.lookup("heading").expect("heading");
    assert_eq!(heading.kind, BindingKind::Readonly);
    assert_eq!(
        content[heading.range.start as usize..heading.range.end as usize],
        *"heading",
        "the range points at the bound local, not the prop key"
    );
    assert_eq!(
        projection.write.write_target("heading"),
        Err(WriteRejection::ReadonlyProp)
    );
    assert!(projection.read.lookup("count").is_some());
}

#[test]
fn stp15_standalone_define_props_binds_template_rows() {
    let content = "defineProps<{ title: string; count: number }>();\n";
    let projection = project_setup(content);
    for name in ["title", "count"] {
        let read = projection.read.lookup(name).expect(name);
        assert_eq!(read.kind, BindingKind::Readonly);
        assert_eq!(
            projection.write.write_target(name),
            Err(WriteRejection::ReadonlyProp)
        );
    }
}

#[test]
fn stp15_with_defaults_keeps_inner_type_arguments() {
    let content = r#"const { title, extra } = withDefaults(defineProps<{ title: string }>(), {});
"#;
    let projection = project_setup(content);
    let title = projection.read.lookup("title").expect("title");
    assert_eq!(title.kind, BindingKind::Readonly);
    assert_eq!(
        content[title.range.start as usize..title.range.end as usize],
        *"title"
    );
    assert_eq!(
        projection.write.write_target("title"),
        Err(WriteRejection::ReadonlyProp)
    );
    assert!(
        projection.read.lookup("extra").is_none(),
        "only declared props become rows, not sibling pattern names"
    );
}

/// The `Counter.vue.d.ts` tsc fixture pins the contract shape exercised
/// by the STP15 probes; this test ties that static file to the live
/// compiler projection so fixture drift fails here instead of passing
/// silently against a stale copy.
#[test]
fn stp15_counter_fixture_matches_projection() {
    const FIXTURE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/sfc-projection/STP15/probes/components/Counter.vue.d.ts"
    ));
    for pinned in [
        "countRef",
        "CounterProps",
        "set label",
        "get label",
        "doubled",
        "modelValue",
        "ComponentPublicInstance",
    ] {
        assert!(
            FIXTURE.contains(pinned),
            "fixture still pins the probed shape {pinned:?}"
        );
    }
    let content = r#"import { ref, computed } from 'vue';
const count = ref(0);
const doubled = computed(() => count.value * 2);
const label = computed({
  get: (): number => count.value,
  set: (v: string) => { count.value = Number(v); },
});
const modelValue = defineModel<string>();
const { title } = defineProps<{ title: string }>();
function increment(step?: number) { count.value += step ?? 1; }
"#;
    let projection = project_setup(content);
    let count = projection.read.lookup("count").expect("count");
    assert_eq!(count.kind, BindingKind::Ref);
    assert!(count.unwrapped && count.script_wraps_ref);
    assert_eq!(
        projection.write.write_target("doubled"),
        Err(WriteRejection::GetterOnlyComputed)
    );
    match &projection
        .write
        .write_target("label")
        .expect("label")
        .domain
    {
        WriteDomain::SetterParam(domain) => assert_eq!(domain, "string"),
        other => panic!("setter domain must be the declared type, got {other:?}"),
    }
    assert_eq!(
        projection
            .write
            .write_target("modelValue")
            .expect("model")
            .domain,
        WriteDomain::ModelValue
    );
    assert_eq!(
        projection.write.write_target("title"),
        Err(WriteRejection::ReadonlyProp)
    );
    assert!(projection.read.lookup("increment").is_some());
    assert_eq!(
        projection.write.write_target("increment"),
        Err(WriteRejection::UnknownBinding)
    );
}

#[test]
fn stp15_non_literal_computed_getter_stays_getter_only() {
    let content = r#"import { computed } from 'vue';
declare function getDoubled(): number;
declare function createGetter(): () => number;
const fromRef = computed(getDoubled);
const fromCall = computed(createGetter());
"#;
    let projection = project_setup(content);
    for name in ["fromRef", "fromCall"] {
        let read = projection.read.lookup(name).expect(name);
        assert_eq!(
            read.kind,
            BindingKind::Computed { setter: false },
            "{name} is a getter-only computed, never a plain writable row"
        );
        assert_eq!(
            projection.write.write_target(name),
            Err(WriteRejection::GetterOnlyComputed)
        );
    }
    assert!(
        !projection
            .write
            .writable
            .iter()
            .any(|w| w.name == "fromRef" || w.name == "fromCall"),
        "non-literal computed getters must not appear in the writable rows"
    );
}

#[test]
fn stp15_renamed_prop_with_default_binds_local() {
    let content = "const { title: heading = \"Default\" } = defineProps<{ title?: string }>();\n";
    let projection = project_setup(content);
    let heading = projection.read.lookup("heading").expect("heading");
    assert_eq!(heading.kind, BindingKind::Readonly);
    assert_eq!(
        content[heading.range.start as usize..heading.range.end as usize],
        *"heading",
        "the range points at the bound local, not the prop key"
    );
    assert_eq!(
        projection.write.write_target("heading"),
        Err(WriteRejection::ReadonlyProp)
    );
}

#[test]
fn stp15_options_methods_are_reads_not_write_targets() {
    let normal = block("export default { methods: { increment() {} } };");
    let projection = project_binding_views(Some(normal), None, None).expect("projects");
    assert!(
        projection.read.lookup("increment").is_some(),
        "an Options method stays a template-visible read"
    );
    assert_eq!(
        projection.write.write_target("increment"),
        Err(WriteRejection::UnknownBinding),
        "an Options method is callable, never a writable assignment target"
    );
}

#[test]
fn stp15_standalone_runtime_object_props_bind_template_rows() {
    let content = "defineProps({ title: String, count: { type: Number, required: true } });\n";
    let projection = project_setup(content);
    for name in ["title", "count"] {
        let read = projection.read.lookup(name).expect(name);
        assert_eq!(read.kind, BindingKind::Readonly);
        assert_eq!(
            content[read.range.start as usize..read.range.end as usize],
            *name,
            "the range points at the declared runtime prop key"
        );
        assert_eq!(
            projection.write.write_target(name),
            Err(WriteRejection::ReadonlyProp)
        );
    }
}

#[test]
fn stp15_const_plain_bindings_refuse_template_writes() {
    let content = r#"import { ref } from 'vue';
const count = ref(0);
const nested = { c: ref(0) };
const frozen = 1;
let loose = 2;
"#;
    let projection = project_setup(content);
    // Immutable lexical plains keep their direct read row ...
    for name in ["nested", "frozen"] {
        let read = projection.read.lookup(name).expect(name);
        assert_eq!(read.kind, BindingKind::Plain);
        assert!(!read.unwrapped);
        // ... but template reassignment of a `const` name must not typecheck.
        assert_eq!(
            projection.write.write_target(name),
            Err(WriteRejection::ConstBinding),
            "{name} is immutable in scope"
        );
    }
    // The mutable twin stays writable: mutability follows the declaration.
    assert!(projection.write.write_target("loose").is_ok());
    // Refs are unaffected by declaration mutability: the wrapper is mutable.
    assert!(projection.write.write_target("count").is_ok());
}

#[test]
fn stp15_whole_torefs_result_reads_directly() {
    let content = r#"import { reactive, toRefs } from 'vue';
const state = reactive({ count: 0 });
const refs = toRefs(state);
const { count } = toRefs(state);
"#;
    let projection = project_setup(content);
    // The whole `toRefs(...)` object is not itself a ref: direct read, and
    // (declared `const`) refused as a write target.
    let refs = projection.read.lookup("refs").expect("refs");
    assert_eq!(
        refs.kind,
        BindingKind::Plain,
        "the whole toRefs result is an object of refs, not an unwrapped ref"
    );
    assert!(!refs.unwrapped);
    assert!(!refs.script_wraps_ref);
    assert_eq!(
        projection.write.write_target("refs"),
        Err(WriteRejection::ConstBinding)
    );
    // Control twin: destructured `toRefs` members stay unwrapped refs.
    let count = projection.read.lookup("count").expect("count");
    assert_eq!(count.kind, BindingKind::Ref);
    assert!(count.unwrapped && count.script_wraps_ref);
    assert!(projection.write.write_target("count").is_ok());
}

#[test]
fn stp15_usage_collects_authored_region_references() {
    let script = r#"import { ref, computed } from 'vue';
const count = ref(0);
const doubled = computed(() => count.value * 2);
const nested = { c: ref(0) };
const theme = ref('red');
function bump() { count.value++; }
"#;
    let template = r#"<button @click="bump(); count++">{{ doubled }}</button>"#;
    let style = r#".accent { color: v-bind(theme); }"#;
    let projection = project_setup(script);
    let declared: Vec<String> = read_names(&projection)
        .into_iter()
        .map(str::to_string)
        .collect();
    // No caller-supplied slices: regions come from authored bytes only.
    let usage = BindingUsageSet::from_region_text(&declared, template, script, style);
    let count = usage
        .used
        .iter()
        .find(|u| u.name == "count")
        .expect("count");
    assert!(
        count.regions.template && count.regions.script,
        "count is referenced in authored template and script"
    );
    let doubled = usage
        .used
        .iter()
        .find(|u| u.name == "doubled")
        .expect("doubled");
    assert!(
        doubled.regions.template && !doubled.regions.script && !doubled.regions.style,
        "doubled is a template-only reference"
    );
    let theme = usage
        .used
        .iter()
        .find(|u| u.name == "theme")
        .expect("theme");
    assert!(
        theme.regions.style && !theme.regions.template,
        "theme is a style v-bind reference"
    );
    // Declarations never count as uses and nothing is seeded: a binding
    // unreferenced in every region stays visible as unused.
    assert!(
        usage.unused().contains(&"nested"),
        "nested has no authored reference in any region"
    );
    assert!(
        !usage.used.iter().any(|u| u.name == "ghost"),
        "unmentioned names never join the population"
    );
}

#[test]
fn stp15_defineprops_rest_spread_binds_a_readonly_row() {
    let content = "const { title, ...rest } = defineProps<{ title: string }>();\n";
    let projection = project_setup(content);
    let rest = projection.read.lookup("rest").expect("rest");
    assert_eq!(
        rest.kind,
        BindingKind::Readonly,
        "a rest-spread local holds readonly prop copies"
    );
    assert_eq!(
        projection.write.write_target("rest"),
        Err(WriteRejection::ReadonlyProp),
        "a rest-spread local is never a writable assignment target"
    );
}

#[test]
fn stp15_type_only_imports_bind_no_read_row() {
    let content = "import type { Foo } from './types';\nimport { type Bar, ref } from 'vue';\nconst count = ref(0);\n";
    let projection = project_setup(content);
    assert!(
        projection.read.lookup("Foo").is_none(),
        "`import type` names are not template-visible reads"
    );
    assert!(
        projection.read.lookup("Bar").is_none(),
        "inline `type` specifiers are not template-visible reads"
    );
    assert!(
        projection.read.lookup("count").is_some(),
        "runtime bindings still bind"
    );
    assert!(
        projection.read.lookup("ref").is_some(),
        "runtime imports still bind"
    );
}

#[test]
fn stp15_non_factory_import_is_an_ordinary_binding() {
    let content = "import { watch } from 'vue';\nconst stop = watch(count);\n";
    let projection = project_setup(content);
    let stop = projection.read.lookup("stop").expect("stop");
    assert_eq!(
        stop.kind,
        BindingKind::Plain,
        "`watch` is outside the cited factory table, so the call is ordinary"
    );
    assert!(
        !stop.unwrapped,
        "a non-factory result never reads unwrapped"
    );
}

#[test]
fn stp15_options_writable_computed_uses_declared_setter_domain() {
    let normal = block(
        "export default { computed: { label: { get(): number { return 1; }, set(v: string) {} } } };\n",
    );
    let projection = project_binding_views(Some(normal), None, None).expect("projects");
    let label = projection.read.lookup("label").expect("label");
    assert_eq!(
        label.kind,
        BindingKind::Computed { setter: true },
        "an Options computed with `set` is writable"
    );
    match &projection
        .write
        .write_target("label")
        .expect("label")
        .domain
    {
        WriteDomain::SetterParam(domain) => assert_eq!(
            domain, "string",
            "an Options setter records its declared annotation, not the read type"
        ),
        other => panic!("Options setter domain must be the declared type, got {other:?}"),
    }
}
