use crate::analysis::build_script_analysis;
use crate::analysis::types::{AnalysisFlags, ScriptAnalysisSnapshot};
use oxc_allocator::Allocator;
use oxc_span::SourceType;

fn analyze(source: &str) -> ScriptAnalysisSnapshot {
    let alloc = Allocator::default();
    build_script_analysis(source, SourceType::tsx(), &alloc)
}

// ── Props ──

#[test]
fn props_array_form() {
    let snap = analyze("export default { props: ['foo', 'bar'] }");
    let opts = snap.options_api.expect("should have options_api");
    assert!(!opts.is_define_component);
    assert_eq!(opts.props.len(), 2);
    assert_eq!(opts.props[0].name, "foo");
    assert_eq!(opts.props[1].name, "bar");
    assert!(opts.props[0].type_constructor.is_none());
    assert!(!opts.props[0].is_required);
    assert!(!opts.props[0].has_default);
}

#[test]
fn props_object_shorthand() {
    let snap = analyze("export default { props: { count: Number, title: String } }");
    let opts = snap.options_api.unwrap();
    assert_eq!(opts.props.len(), 2);
    assert_eq!(opts.props[0].name, "count");
    assert_eq!(opts.props[0].type_constructor.as_deref(), Some("Number"));
    assert_eq!(opts.props[1].name, "title");
    assert_eq!(opts.props[1].type_constructor.as_deref(), Some("String"));
}

#[test]
fn props_full_object() {
    let snap = analyze(
        r#"export default {
            props: {
                size: { type: Number, required: true },
                label: { type: String, default: 'hello' },
                items: { type: Array },
            }
        }"#,
    );
    let opts = snap.options_api.unwrap();
    assert_eq!(opts.props.len(), 3);

    assert_eq!(opts.props[0].name, "size");
    assert_eq!(opts.props[0].type_constructor.as_deref(), Some("Number"));
    assert!(opts.props[0].is_required);
    assert!(!opts.props[0].has_default);

    assert_eq!(opts.props[1].name, "label");
    assert_eq!(opts.props[1].type_constructor.as_deref(), Some("String"));
    assert!(!opts.props[1].is_required);
    assert!(opts.props[1].has_default);

    assert_eq!(opts.props[2].name, "items");
    assert_eq!(opts.props[2].type_constructor.as_deref(), Some("Array"));
    assert!(!opts.props[2].is_required);
    assert!(!opts.props[2].has_default);
}

// ── Emits ──

#[test]
fn emits_array() {
    let snap = analyze("export default { emits: ['click', 'update'] }");
    let opts = snap.options_api.unwrap();
    assert_eq!(opts.emits.len(), 2);
    assert_eq!(opts.emits[0].name, "click");
    assert_eq!(opts.emits[1].name, "update");
}

#[test]
fn emits_object() {
    let snap = analyze("export default { emits: { click: null, update: (val: string) => true } }");
    let opts = snap.options_api.unwrap();
    assert_eq!(opts.emits.len(), 2);
    assert_eq!(opts.emits[0].name, "click");
    assert_eq!(opts.emits[1].name, "update");
}

// ── Data ──

#[test]
fn data_function() {
    let snap = analyze("export default { data() { return { count: 0, name: 'hello' } } }");
    let opts = snap.options_api.unwrap();
    assert_eq!(opts.data_fields.len(), 2);
    assert_eq!(opts.data_fields[0].name, "count");
    assert_eq!(opts.data_fields[1].name, "name");
}

#[test]
fn data_arrow() {
    let snap = analyze("export default { data: () => ({ count: 0, items: [] }) }");
    let opts = snap.options_api.unwrap();
    assert_eq!(opts.data_fields.len(), 2);
    assert_eq!(opts.data_fields[0].name, "count");
    assert_eq!(opts.data_fields[1].name, "items");
}

// ── Computed ──

#[test]
fn computed_fields() {
    let snap = analyze(
        "export default { computed: { doubled() { return this.count * 2 }, fullName: { get() { return '' }, set(v: string) {} } } }",
    );
    let opts = snap.options_api.unwrap();
    assert_eq!(opts.computed_fields.len(), 2);
    assert_eq!(opts.computed_fields[0].name, "doubled");
    assert_eq!(opts.computed_fields[1].name, "fullName");
}

// ── Methods ──

#[test]
fn methods_fields() {
    let snap = analyze("export default { methods: { increment() {}, reset() {} } }");
    let opts = snap.options_api.unwrap();
    assert_eq!(opts.methods.len(), 2);
    assert_eq!(opts.methods[0].name, "increment");
    assert_eq!(opts.methods[1].name, "reset");
}

// ── Inject ──

#[test]
fn inject_array() {
    let snap = analyze("export default { inject: ['theme', 'locale'] }");
    let opts = snap.options_api.unwrap();
    assert_eq!(opts.inject_keys.len(), 2);
    assert_eq!(opts.inject_keys[0].name, "theme");
    assert_eq!(opts.inject_keys[1].name, "locale");
}

#[test]
fn inject_object() {
    let snap =
        analyze("export default { inject: { theme: { from: 'appTheme', default: 'light' } } }");
    let opts = snap.options_api.unwrap();
    assert_eq!(opts.inject_keys.len(), 1);
    assert_eq!(opts.inject_keys[0].name, "theme");
}

// ── Provide ──

#[test]
fn provide_object() {
    let snap = analyze("export default { provide: { theme: 'dark' } }");
    let opts = snap.options_api.unwrap();
    assert_eq!(opts.provide_keys.len(), 1);
    assert_eq!(opts.provide_keys[0].name, "theme");
}

#[test]
fn provide_function() {
    let snap = analyze("export default { provide() { return { theme: this.theme } } }");
    let opts = snap.options_api.unwrap();
    assert_eq!(opts.provide_keys.len(), 1);
    assert_eq!(opts.provide_keys[0].name, "theme");
}

// ── Expose ──

#[test]
fn expose_array() {
    let snap = analyze("export default { expose: ['focus', 'reset'] }");
    let opts = snap.options_api.unwrap();
    assert_eq!(opts.expose.len(), 2);
    assert_eq!(opts.expose[0].name, "focus");
    assert_eq!(opts.expose[1].name, "reset");
}

// ── Components ──

#[test]
fn components_shorthand() {
    let snap = analyze(
        r#"import MyComp from './MyComp.vue';
export default { components: { MyComp } }"#,
    );
    let opts = snap.options_api.unwrap();
    assert_eq!(opts.components.len(), 1);
    assert_eq!(opts.components[0].name, "MyComp");
    assert_eq!(
        opts.components[0].import_source.as_deref(),
        Some("./MyComp.vue")
    );
}

#[test]
fn components_with_alias() {
    let snap = analyze(
        r#"import Foo from './Foo.vue';
export default { components: { Alias: Foo } }"#,
    );
    let opts = snap.options_api.unwrap();
    assert_eq!(opts.components.len(), 1);
    assert_eq!(opts.components[0].name, "Alias");
    assert_eq!(
        opts.components[0].import_source.as_deref(),
        Some("./Foo.vue")
    );
}

// ── inheritAttrs ──

#[test]
fn inherit_attrs_false() {
    let snap = analyze("export default { inheritAttrs: false }");
    let opts = snap.options_api.unwrap();
    assert!(opts.has_inherit_attrs_false);
}

#[test]
fn inherit_attrs_default() {
    let snap = analyze("export default { props: ['a'] }");
    let opts = snap.options_api.unwrap();
    assert!(!opts.has_inherit_attrs_false);
}

// ── defineComponent wrapping ──

#[test]
fn define_component_wrapping() {
    let snap = analyze(
        r#"import { defineComponent } from 'vue';
export default defineComponent({
    props: { msg: String },
    data() { return { count: 0 } },
    computed: { doubled() { return this.count * 2 } },
    methods: { inc() { this.count++ } },
})"#,
    );
    let opts = snap.options_api.unwrap();
    assert!(opts.is_define_component);
    assert_eq!(opts.props.len(), 1);
    assert_eq!(opts.props[0].name, "msg");
    assert_eq!(opts.data_fields.len(), 1);
    assert_eq!(opts.data_fields[0].name, "count");
    assert_eq!(opts.computed_fields.len(), 1);
    assert_eq!(opts.computed_fields[0].name, "doubled");
    assert_eq!(opts.methods.len(), 1);
    assert_eq!(opts.methods[0].name, "inc");
}

// ── Negative: script setup should NOT produce options_api ──

#[test]
fn script_setup_no_options_api() {
    let snap = analyze(
        r#"import { ref } from 'vue';
const count = ref(0);
"#,
    );
    assert!(
        snap.options_api.is_none(),
        "script setup should not produce options_api"
    );
}

// ── Flags ──

#[test]
fn has_options_api_flag() {
    let snap = analyze("export default { data() { return {} } }");
    assert!(snap.flags.contains(AnalysisFlags::HAS_OPTIONS_API));
}

#[test]
fn no_options_api_flag_for_setup() {
    let snap = analyze("import { ref } from 'vue'; const x = ref(0);");
    assert!(!snap.flags.contains(AnalysisFlags::HAS_OPTIONS_API));
}

// ── Mixed props forms ──

#[test]
fn props_mixed_shorthand_and_full() {
    let snap = analyze(
        r#"export default {
            props: {
                simple: Number,
                complex: { type: String, required: true, default: '' },
                bare: Boolean,
            }
        }"#,
    );
    let opts = snap.options_api.unwrap();
    assert_eq!(opts.props.len(), 3);
    assert_eq!(opts.props[0].name, "simple");
    assert_eq!(opts.props[0].type_constructor.as_deref(), Some("Number"));
    assert!(!opts.props[0].is_required);

    assert_eq!(opts.props[1].name, "complex");
    assert_eq!(opts.props[1].type_constructor.as_deref(), Some("String"));
    assert!(opts.props[1].is_required);
    assert!(opts.props[1].has_default);

    assert_eq!(opts.props[2].name, "bare");
    assert_eq!(opts.props[2].type_constructor.as_deref(), Some("Boolean"));
}

// ── Default values ──

#[test]
fn props_default_value_string() {
    let snap = analyze("export default { props: { message: { type: String, default: 'Hello' } } }");
    let opts = snap.options_api.unwrap();
    assert_eq!(opts.props.len(), 1);
    assert!(opts.props[0].has_default);
    assert_eq!(opts.props[0].default_value.as_deref(), Some("'Hello'"));
}

#[test]
fn props_default_value_string_double_quoted_stays_verbatim() {
    let snap = analyze(r#"export default { props: { message: { type: String, default: "Hi" } } }"#);
    let opts = snap.options_api.unwrap();
    assert_eq!(opts.props[0].default_value.as_deref(), Some("\"Hi\""));
    assert_ne!(
        opts.props[0].default_value.as_deref(),
        Some("Hi"),
        "inner-value extraction (quote stripping) must not run"
    );
}

#[test]
fn props_default_value_number() {
    let snap = analyze("export default { props: { count: { type: Number, default: 42 } } }");
    let opts = snap.options_api.unwrap();
    assert!(opts.props[0].has_default);
    assert_eq!(opts.props[0].default_value.as_deref(), Some("42"));
}

#[test]
fn props_default_value_boolean() {
    let snap = analyze("export default { props: { active: { type: Boolean, default: false } } }");
    let opts = snap.options_api.unwrap();
    assert!(opts.props[0].has_default);
    assert_eq!(opts.props[0].default_value.as_deref(), Some("false"));
}

#[test]
fn props_no_default_returns_none() {
    let snap = analyze("export default { props: { title: { type: String, required: true } } }");
    let opts = snap.options_api.unwrap();
    assert!(!opts.props[0].has_default);
    assert!(opts.props[0].default_value.is_none());
}

// ── TSAsExpression (PropType) ──

#[test]
fn props_ts_as_prop_type() {
    let snap = analyze(
        "export default { props: { canvas: { type: Object as PropType<HTMLCanvasElement> } } }",
    );
    let opts = snap.options_api.unwrap();
    assert_eq!(opts.props[0].type_constructor.as_deref(), Some("Object"));
    assert_eq!(
        opts.props[0].type_annotation.as_deref(),
        Some("HTMLCanvasElement")
    );
}

#[test]
fn props_ts_as_prop_type_without_annotation() {
    let snap = analyze("export default { props: { count: { type: Number } } }");
    let opts = snap.options_api.unwrap();
    assert_eq!(opts.props[0].type_constructor.as_deref(), Some("Number"));
    assert!(opts.props[0].type_annotation.is_none());
}

// ── Emit payload types ──

#[test]
fn emits_object_validator_extracts_payload_type() {
    let snap = analyze(
        r#"export default {
            emits: {
                select: (item: string) => typeof item === 'string',
                clear: () => true,
                'update:count': (value: number) => typeof value === 'number',
            }
        }"#,
    );
    let opts = snap.options_api.unwrap();
    assert_eq!(opts.emits.len(), 3);

    assert_eq!(opts.emits[0].name, "select");
    assert_eq!(
        opts.emits[0].payload_type.as_deref(),
        Some("[item: string]"),
        "validator (item: string) => ... should extract payload_type"
    );

    assert_eq!(opts.emits[1].name, "clear");
    assert_eq!(
        opts.emits[1].payload_type.as_deref(),
        Some("[]"),
        "validator () => true should extract empty tuple payload_type"
    );

    assert_eq!(opts.emits[2].name, "update:count");
    assert_eq!(
        opts.emits[2].payload_type.as_deref(),
        Some("[value: number]"),
        "validator (value: number) => ... should extract payload_type"
    );
}

#[test]
fn emits_object_null_has_no_payload_type() {
    let snap = analyze("export default { emits: { click: null } }");
    let opts = snap.options_api.unwrap();
    assert_eq!(opts.emits[0].name, "click");
    assert!(
        opts.emits[0].payload_type.is_none(),
        "null validator should not have payload_type"
    );
}

#[test]
fn emits_array_has_no_payload_type() {
    let snap = analyze("export default { emits: ['click', 'update'] }");
    let opts = snap.options_api.unwrap();
    for emit in &opts.emits {
        assert!(
            emit.payload_type.is_none(),
            "array-form emits should not have payload_type"
        );
    }
}

#[test]
fn emits_object_multi_param_validator() {
    let snap = analyze(
        r#"export default {
            emits: {
                drop: (source: string, target: string, mode: number) => true,
            }
        }"#,
    );
    let opts = snap.options_api.unwrap();
    assert_eq!(
        opts.emits[0].payload_type.as_deref(),
        Some("[source: string, target: string, mode: number]"),
        "multi-param validator should extract all params as tuple"
    );
}

// ── JSDoc extraction ──

#[test]
fn jsdoc_on_options_api_props() {
    let snap = analyze(
        r#"export default defineComponent({
            props: {
                /** The display label */
                label: String,
                /** Size variant
                 * @default 'md'
                 */
                size: { type: String, default: 'md' },
                noDoc: Number,
            }
        })"#,
    );
    let opts = snap.options_api.unwrap();
    let props = &opts.props;
    assert_eq!(props.len(), 3);

    // Positive: label has description, no tags
    assert_eq!(props[0].description.as_deref(), Some("The display label"));
    assert!(props[0].tags.is_empty());

    // Positive: size has description and @default tag
    assert_eq!(props[1].description.as_deref(), Some("Size variant"));
    assert_eq!(props[1].tags.len(), 1);
    assert_eq!(props[1].tags[0].name, "default");
    assert_eq!(props[1].tags[0].text.as_deref(), Some("'md'"));

    // Negative: noDoc has no JSDoc
    assert!(props[2].description.is_none());
    assert!(props[2].tags.is_empty());
}

#[test]
fn jsdoc_on_bare_export_default_props() {
    let snap = analyze(
        r#"export default {
            props: {
                /** Required label */
                label: { type: String, required: true },
            }
        }"#,
    );
    let opts = snap.options_api.unwrap();
    assert_eq!(opts.props[0].description.as_deref(), Some("Required label"));
}
