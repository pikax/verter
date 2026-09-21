use super::options_api::*;
use super::script_setup::ScriptBlockInput;
use super::script_setup::SetupProjectionRefusal;
use crate::cursor::ScriptLanguage;

fn block(content: &str, start: u32, lang: Option<ScriptLanguage>) -> ScriptBlockInput<'_> {
    ScriptBlockInput {
        content,
        content_start: start,
        lang,
    }
}

fn ts(content: &str) -> ScriptBlockInput<'_> {
    block(content, 0, Some(ScriptLanguage::TypeScript))
}

fn member_names(projection: &OptionsComponentProjection) -> Vec<(&str, OptionsMemberKind)> {
    projection
        .members
        .iter()
        .map(|member| (member.name.as_str(), member.kind))
        .collect()
}

const OPTIONS_THIS: &str = r#"import { defineComponent } from 'vue';
export default defineComponent({
  name: 'Counter',
  props: { title: String, initial: { type: Number, default: 0 } },
  data() { return { count: this.initial ?? 0 }; },
  computed: {
    doubled(): number { return this.count * 2; },
    label: {
      get(): string { return `${this.title}: ${this.count}`; },
      set(value: string) { this.title = value; },
    },
  },
  methods: {
    increment(step?: number) { this.count += step ?? 1; },
  },
  emits: ['change', 'reset'],
});"#;

#[test]
fn stp13_options_this_members_keep_contextual_kinds() {
    let projection = project_options_block(ts(OPTIONS_THIS)).expect("projects");
    assert_eq!(projection.dialect, OptionsDialect::TypeScript);
    assert!(projection.has_default_export);
    assert!(projection.define_component_wrapped);
    assert!(projection.constructor_shaped);
    assert!(projection.has_data_fn);
    assert_eq!(projection.component_name.as_deref(), Some("Counter"));
    let names = member_names(&projection);
    assert!(names.contains(&("title", OptionsMemberKind::Prop)));
    assert!(names.contains(&("initial", OptionsMemberKind::Prop)));
    assert!(names.contains(&("doubled", OptionsMemberKind::Computed { setter: false })));
    assert!(names.contains(&("label", OptionsMemberKind::Computed { setter: true })));
    assert!(names.contains(&("increment", OptionsMemberKind::Method)));
    assert!(names.contains(&("change", OptionsMemberKind::Emit)));
    // Ranges slice the authored member names.
    for member in &projection.members {
        assert_eq!(
            &OPTIONS_THIS[member.range.start as usize..member.range.end as usize],
            member.name
        );
    }
}

#[test]
fn stp13_plain_object_export_is_unwrapped_without_wrapper() {
    let projection =
        project_options_block(ts("export default { methods: { go() {} } };")).expect("projects");
    assert!(projection.has_default_export);
    assert!(!projection.define_component_wrapped);
    assert!(projection.constructor_shaped);
    assert!(member_names(&projection).contains(&("go", OptionsMemberKind::Method)));
}

#[test]
fn stp13_local_wrapper_binding_is_an_ordinary_call() {
    let content =
        "function defineComponent(x: unknown) { return x; }\nexport default defineComponent({});";
    let projection = project_options_block(ts(content)).expect("projects");
    assert!(projection.has_default_export);
    assert!(!projection.define_component_wrapped);
    assert!(projection.members.is_empty());
}

#[test]
fn stp13_vue_import_alias_still_unwraps_by_binding() {
    // An imported alias resolves by binding, not callee spelling: `dc` is
    // the `defineComponent` symbol imported from `'vue'`.
    let content =
        "import { defineComponent as dc } from 'vue';\nexport default dc({ props: ['a'] });";
    let projection = project_options_block(ts(content)).expect("projects");
    assert!(projection.define_component_wrapped);
    assert!(member_names(&projection).contains(&("a", OptionsMemberKind::Prop)));
    let content =
        "import { defineOptions as configure } from 'vue';\nexport default configure({ name: 'Aliased' });";
    let projection = project_options_block(ts(content)).expect("projects");
    assert!(projection.define_component_wrapped);
    assert_eq!(projection.component_name.as_deref(), Some("Aliased"));
    let content =
        "import { defineComponent } from 'vue';\nexport default defineComponent({ props: ['a'] });";
    let projection = project_options_block(ts(content)).expect("projects");
    assert!(projection.define_component_wrapped);
    assert!(member_names(&projection).contains(&("a", OptionsMemberKind::Prop)));
}

#[test]
fn stp13_vue_import_masquerade_stays_an_ordinary_call() {
    // A different `'vue'` symbol aliased to the wrapper name is not the
    // wrapper: the binding must originate from `defineComponent` itself.
    let content =
        "import { ref as defineComponent } from 'vue';\nexport default defineComponent({ props: ['a'] });";
    let projection = project_options_block(ts(content)).expect("projects");
    assert!(!projection.define_component_wrapped);
    assert!(projection.members.is_empty());
}

#[test]
fn stp13_identifier_mixins_are_opaque_not_dropped() {
    // A dynamically composed `mixins` value keeps no static member list but
    // must surface in the template view as an opaque source.
    let content = "import M from './m';\nexport default { mixins: componentMixins };";
    let projection = project_options_block(ts(content)).expect("projects");
    assert!(projection.mixins.is_empty());
    assert!(projection.has_nonstatic_mixins);
    let combined = project_options_pair(Some(ts(content)), None, None).expect("projects");
    let view = OptionsTemplateBindingView::build(&combined);
    assert!(view.opaque_sources.contains(&"mixins".to_string()));
}

#[test]
fn stp13_mixins_extends_components_and_directives_recorded() {
    let content = r#"import M from './m';
import C from './c';
export default {
  components: { C, Alias: C },
  directives: { focus: {} },
  mixins: [M, other],
  extends: Base,
};"#;
    let projection = project_options_block(ts(content)).expect("projects");
    let names = member_names(&projection);
    assert!(names.contains(&("C", OptionsMemberKind::Component)));
    assert!(names.contains(&("Alias", OptionsMemberKind::Component)));
    assert!(names.contains(&("focus", OptionsMemberKind::Directive)));
    assert_eq!(
        projection.mixins,
        vec!["M".to_string(), "other".to_string()]
    );
    assert!(!projection.has_nonstatic_mixins);
    assert_eq!(projection.extends_source.as_deref(), Some("Base"));
    let content = "export default { mixins: [cond ? A : B] };";
    let projection = project_options_block(ts(content)).expect("projects");
    assert!(projection.has_nonstatic_mixins);
    assert!(projection.mixins.is_empty());
}

#[test]
fn stp13_js_dialect_still_extracts_members() {
    let content = "export default { props: ['a'], methods: { go() {} } };";
    for lang in [None, Some(ScriptLanguage::JavaScript)] {
        let projection = project_options_block(block(content, 0, lang)).expect("projects");
        assert_eq!(projection.dialect, OptionsDialect::JavaScript);
        assert!(member_names(&projection).contains(&("a", OptionsMemberKind::Prop)));
        assert!(member_names(&projection).contains(&("go", OptionsMemberKind::Method)));
    }
}

#[test]
fn stp13_combined_coexists_without_template_leakage() {
    let normal = ts("export const storeKey = 'k';\nexport default { props: ['title'] };");
    let setup = ts("const local = 1;\nconst stp13setup = local;");
    let combined = project_options_pair(Some(normal), Some(setup), None).expect("projects");
    assert!(combined.named_exports.contains(&"storeKey".to_string()));
    // The default export is constructor-shaped, never a named export.
    assert!(!combined.named_exports.contains(&"default".to_string()));
    let view = OptionsTemplateBindingView::build(&combined);
    assert_eq!(view.lookup("title"), Some(&OptionsBindingKind::Prop));
    assert_eq!(view.lookup("local"), Some(&OptionsBindingKind::SetupLocal));
    // Named module exports coexist without leaking into template scope.
    assert!(!view.is_template_visible("storeKey"));
    assert!(view.is_template_visible("title"));
}

#[test]
fn stp13_shadowed_macro_is_an_ordinary_call() {
    let normal = ts("function defineProps() { return {}; }\nexport default {};");
    let setup = ts("const props = defineProps();");
    let combined = project_options_pair(Some(normal), Some(setup), None).expect("projects");
    assert_eq!(combined.shadowed_macros, vec!["defineProps"]);
    // The single setup authority agrees: no macro recorded.
    assert!(combined.facts.setup.expect("setup").macros.is_empty());
}

#[test]
fn stp13_vue_macro_import_stays_a_macro() {
    let setup = ts("import { defineProps } from 'vue';\nconst properties = defineProps();");
    let combined = project_options_pair(None, Some(setup), None).expect("projects");
    assert!(combined.shadowed_macros.is_empty());
    let macros = &combined.facts.setup.as_ref().expect("setup").macros;
    assert_eq!(macros.len(), 1);
    assert_eq!(macros[0].name, "defineProps");
    let view = OptionsTemplateBindingView::build(&combined);
    assert_eq!(
        view.lookup("properties"),
        Some(&OptionsBindingKind::SetupLocal)
    );
    assert_eq!(view.lookup("props"), None);
}

#[test]
fn stp13_type_only_binding_does_not_shadow() {
    let normal = ts("import type { defineProps } from 'vue';\nexport default {};");
    let combined = project_options_pair(Some(normal), None, None).expect("projects");
    assert!(combined.shadowed_macros.is_empty());
}

#[test]
fn stp13_options_instance_preserves_constructor_shape() {
    let normal = ts("export default { props: { count: Number } };");
    let combined = project_options_pair(Some(normal), None, None).expect("projects");
    let options = combined.options.expect("options");
    assert!(options.has_default_export);
    assert!(options.constructor_shaped);
    assert!(combined.facts.setup.is_none());
}

#[test]
fn stp13_lang_conflict_and_syntax_errors_refuse() {
    let normal = block("export default {};", 0, Some(ScriptLanguage::TypeScript));
    let setup = block("const a = 1;", 0, Some(ScriptLanguage::TSX));
    assert_eq!(
        project_options_pair(Some(normal), Some(setup), None).unwrap_err(),
        SetupProjectionRefusal::ScriptLangConflict
    );
    assert_eq!(
        project_options_block(ts("export default {;")).unwrap_err(),
        SetupProjectionRefusal::SyntaxErrors { setup: false }
    );
}

#[test]
fn stp13_default_js_normal_conflicts_with_ts_setup() {
    // An absent lang is Vue's default JavaScript dialect, so it conflicts
    // with an explicit TypeScript setup block in either direction.
    let js_normal = block("export default {};", 0, None);
    let ts_setup = block("const a = 1;", 0, Some(ScriptLanguage::TypeScript));
    assert_eq!(
        project_options_pair(Some(js_normal), Some(ts_setup), None).unwrap_err(),
        SetupProjectionRefusal::ScriptLangConflict
    );
    let ts_normal = block("export default {};", 0, Some(ScriptLanguage::TypeScript));
    let js_setup = block("const a = 1;", 0, None);
    assert_eq!(
        project_options_pair(Some(ts_normal), Some(js_setup), None).unwrap_err(),
        SetupProjectionRefusal::ScriptLangConflict
    );
}

#[test]
fn stp13_named_exports_exclude_default_in_both_dialects() {
    // A default export (declaration or `as default` specifier) never
    // appears in named_exports; ordinary named exports still do.
    for normal in [
        ts("export const keep = 1;\nexport default keep;"),
        block(
            "export const keep = 1;\nexport { keep as default };",
            0,
            None,
        ),
    ] {
        let combined = project_options_pair(Some(normal), None, None).expect("projects");
        assert!(combined.named_exports.contains(&"keep".to_string()));
        assert!(!combined.named_exports.contains(&"default".to_string()));
    }
}

#[test]
fn stp13_string_literal_keys_slice_without_quotes() {
    let content = "export default { emits: { 'update:modelValue': null }, \
        props: { 'custom-prop': String }, computed: { 'doubled up'() { return 1; } } };";
    let source = content.to_string();
    let projection = project_options_block(ts(content)).expect("projects");
    assert!(!projection.members.is_empty());
    for member in &projection.members {
        assert_eq!(
            &source[member.range.start as usize..member.range.end as usize],
            member.name
        );
    }
}

#[test]
fn stp13_identifier_setup_and_data_are_opaque() {
    let content = "import { useFeature } from './f';\n\
        export default { setup: useFeature, data: initialData };";
    let normal = ts(content);
    let combined = project_options_pair(Some(normal), None, None).expect("projects");
    let view = OptionsTemplateBindingView::build(&combined);
    assert!(view.opaque_sources.contains(&"setup()".to_string()));
    assert!(view.opaque_sources.contains(&"data()".to_string()));
    // Explicit null/undefined composes no runtime state.
    let content = "export default { setup: null, data: undefined };";
    let combined = project_options_pair(Some(ts(content)), None, None).expect("projects");
    let view = OptionsTemplateBindingView::build(&combined);
    assert!(!view.opaque_sources.contains(&"setup()".to_string()));
    assert!(!view.opaque_sources.contains(&"data()".to_string()));
}

#[test]
fn stp13_opaque_sources_recorded_never_invented() {
    let normal = ts("import M from './m';\nexport default { data() { return {}; }, mixins: [M], extends: Base };");
    let combined = project_options_pair(Some(normal), None, None).expect("projects");
    let view = OptionsTemplateBindingView::build(&combined);
    assert!(view.opaque_sources.contains(&"data()".to_string()));
    assert!(view.opaque_sources.contains(&"mixins".to_string()));
    assert!(view.opaque_sources.contains(&"extends".to_string()));
    assert!(view.bindings.is_empty());
}
