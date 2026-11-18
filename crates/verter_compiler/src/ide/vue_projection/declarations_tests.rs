//! Unit tests for the declarations module.

use super::binder_capture::capture_binder_plan;
use super::checking_module::SETUP_SCOPE;
use super::declarations::project_vue_declarations;
use super::public_constructor::{
    project_public_constructor, PublicDeclarationExtras, EXPOSE_PROVIDER, PUBLIC_COMPONENT,
    PUBLIC_INSTANCE, PUBLIC_PROPS,
};
use super::script_setup::ScriptBlockInput;
use crate::cursor::ScriptLanguage;
use crate::ide::projection_declarations::{ClosureRefusal, DeclarationProjectionProduct};

const CANONICAL: &str = "/ws/Counter.vue";

/// The declaration product of a carrier with one `<script setup lang="ts">`.
fn declarations(sfc: &str) -> DeclarationProjectionProduct {
    generic_declarations(sfc, None)
}

fn generic_declarations(sfc: &str, generic: Option<&str>) -> DeclarationProjectionProduct {
    let open = "<script setup lang=\"ts\">";
    let start = sfc.find(open).expect("setup block") + open.len();
    let end = sfc.find("</script>").expect("setup block end");
    let block = ScriptBlockInput {
        content: &sfc[start..end],
        content_start: start as u32,
        lang: Some(ScriptLanguage::TypeScript),
    };
    let public = project_public_constructor(None, Some(block), generic).expect("public contract");
    let capture = capture_binder_plan(None, Some(block), generic).expect("capture");
    project_vue_declarations(
        sfc,
        CANONICAL,
        &public,
        &capture,
        &PublicDeclarationExtras::default(),
    )
}

#[test]
fn vue_declarations_support_instance_type_and_exact_exposed_members() {
    let sfc = concat!(
        "<script setup lang=\"ts\">\n",
        "import { ref, type Ref } from 'vue';\n",
        "const count: Ref<number> = ref(0);\n",
        "function reset(): void { count.value = 0; }\n",
        "const hidden: string = 'internal';\n",
        "defineProps<{ label: string }>();\n",
        "defineExpose({ count, reset });\n",
        "</script>\n",
        "<template><button @click=\"reset\">{{ label }} {{ count }} {{ hidden }}</button></template>\n",
    );
    let product = declarations(sfc);
    assert!(product.is_publishable(), "{:?}", product.diagnostics);
    assert_eq!(product.module.canonical_id, CANONICAL);
    assert_eq!(product.module.declaration_file, "/ws/Counter.vue.d.ts");
    let text = &product.declaration_source;
    // The exposed surface is the authored argument over the lifted bindings.
    assert_eq!(
        text.matches(&format!(
            "function {EXPOSE_PROVIDER}() {{\nreturn ({{ count, reset }});\n}}\n"
        ))
        .count(),
        1,
        "{text}"
    );
    assert!(
        text.contains(&format!(
            "import(\"vue\").ShallowUnwrapRef<ReturnType<typeof {EXPOSE_PROVIDER}>>"
        )),
        "{text}"
    );
    // One construct signature, so `InstanceType<typeof Counter>` names the instance.
    assert!(
        text.contains(&format!(
            "declare const {PUBLIC_COMPONENT}: {{\n  new (props: {PUBLIC_PROPS}): {PUBLIC_INSTANCE};\n"
        )),
        "{text}"
    );
    assert!(text.ends_with(&format!("export default {PUBLIC_COMPONENT};\n")));
    assert_eq!(
        product.closure.retained_imports,
        ["import { ref, type Ref } from 'vue';"]
    );
    assert!(product
        .closure
        .lifted
        .iter()
        .any(|lifted| lifted.contains("count: Ref<number>")));
    assert!(product
        .closure
        .lifted
        .iter()
        .any(|lifted| lifted.contains("reset(): void")));
    // A binding the template reads but `defineExpose` does not name stays private.
    assert!(!text.contains("hidden"), "{text}");
    // The lifted declaration maps to its authored bytes in the carrier.
    let generated = text.find("count: Ref<number>").expect("lifted count") as u32;
    let authored = sfc.find("count: Ref<number>").expect("authored count") as u32;
    assert!(product.map.mappings.iter().any(|mapping| {
        mapping.generated_start <= generated
            && generated < mapping.generated_end
            && mapping.source_start + generated - mapping.generated_start == authored
    }));
}

#[test]
fn vue_declarations_contain_no_setup_body_template_scope_or_new_expression() {
    let sfc = concat!(
        "<script setup lang=\"ts\">\n",
        "import { onMounted, ref, type Ref } from 'vue';\n",
        "const count: Ref<number> = ref(0);\n",
        "const doubled = count.value * 2;\n",
        "onMounted(() => { count.value = new Date().getFullYear(); });\n",
        "defineExpose({ count });\n",
        "</script>\n",
        "<template><p v-if=\"doubled > 1\">{{ count }}</p></template>\n",
    );
    let product = declarations(sfc);
    assert!(product.is_publishable(), "{:?}", product.diagnostics);
    let text = &product.declaration_source;
    for private in [
        "ref(0)",
        "doubled",
        "onMounted(",
        "getFullYear",
        "defineExpose",
        SETUP_SCOPE,
    ] {
        assert!(!text.contains(private), "{private} leaked into:\n{text}");
    }
    // The only `new` is the constructor's construct signature.
    let signature = text
        .find(&format!("declare const {PUBLIC_COMPONENT}: {{\n  new ("))
        .expect("construct signature")
        + format!("declare const {PUBLIC_COMPONENT}: {{\n  ").len();
    assert_eq!(
        text.match_indices("new")
            .map(|(at, _)| at)
            .collect::<Vec<_>>(),
        [signature],
        "{text}"
    );
}

#[test]
fn vue_declarations_refuse_authored_names_the_closure_does_not_carry() {
    // Each public surface names an authored binding the dependency closure
    // cannot carry. At the declaration's module scope a lib global of the
    // same name (`status`, `name`) would answer, so the product is refused.
    let cases: [(&str, Option<&str>, &str); 4] = [
        (
            "const status: number = 1;\ndefineExpose({ read: () => status });\n",
            None,
            "status",
        ),
        (
            "const { status } = defineProps<{ status: number }>();\ndefineExpose({ status });\n",
            None,
            "status",
        ),
        (
            "import type { PropType } from 'vue';\ndefineProps({ value: Object as PropType<{ id: number }> });\n",
            None,
            "PropType",
        ),
        (
            "import type { Row } from './types';\ndefineProps<{ value: T }>();\n",
            Some("T extends Row = Row"),
            "Row",
        ),
    ];
    for (setup, generic, name) in cases {
        let sfc = format!("<script setup lang=\"ts\">\n{setup}</script>\n");
        let product = generic_declarations(&sfc, generic);
        assert!(
            product
                .closure
                .refused
                .contains(&ClosureRefusal::InaccessibleCapture { name: name.into() }),
            "{setup}\n{:?}\n{}",
            product.closure.refused,
            product.declaration_source
        );
        assert!(!product.is_publishable());
    }
}
