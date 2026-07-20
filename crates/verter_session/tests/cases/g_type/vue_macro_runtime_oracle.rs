use serde_json::Value;

fn oracle() -> Value {
    serde_json::from_str(include_str!("../../fixtures/vue_macro_runtime_oracle.json"))
        .expect("checked-in Vue macro runtime oracle must be valid JSON")
}

fn case<'a>(oracle: &'a Value, id: &str) -> &'a Value {
    oracle["cases"]
        .as_array()
        .expect("oracle cases must be an array")
        .iter()
        .find(|case| case["id"] == id)
        .unwrap_or_else(|| panic!("oracle case {id:?} must exist"))
}

fn prop<'a>(case: &'a Value, name: &str) -> &'a Value {
    case["runtime"]["props"]
        .as_array()
        .expect("runtime props must be an array")
        .iter()
        .find(|prop| prop["name"] == name)
        .unwrap_or_else(|| panic!("oracle prop {name:?} must exist"))
}

fn profile<'a>(case: &'a Value, name: &str) -> &'a Value {
    case["profiles"]
        .as_array()
        .expect("oracle profiles must be an array")
        .iter()
        .find(|profile| profile["name"] == name)
        .unwrap_or_else(|| panic!("oracle profile {name:?} must exist"))
}

fn profile_prop<'a>(case: &'a Value, profile_name: &str, prop_name: &str) -> &'a Value {
    profile(case, profile_name)["runtime"]["props"]
        .as_array()
        .expect("profile runtime props must be an array")
        .iter()
        .find(|prop| prop["name"] == prop_name)
        .unwrap_or_else(|| panic!("oracle prop {prop_name:?} must exist"))
}

#[test]
fn pinned_vue_macro_oracle_carries_provenance_and_discriminating_runtime_facts() {
    let oracle = oracle();
    assert_eq!(oracle["schemaVersion"], 2);
    assert_eq!(oracle["provenance"]["compiler"], "@vue/compiler-sfc");
    assert_eq!(oracle["provenance"]["version"], "3.5.34");
    assert_eq!(
        oracle["provenance"]["fixtureSha256"]
            .as_str()
            .expect("fixture hash must be text")
            .len(),
        64
    );

    let primitive = case(&oracle, "primitive-and-bigint-props");
    assert_eq!(
        prop(primitive, "text")["constructors"],
        serde_json::json!(["String"])
    );
    assert_eq!(
        prop(primitive, "huge")["constructors"],
        serde_json::json!([])
    );

    let unions = case(&oracle, "ordered-unions-and-skip-check");
    assert_eq!(
        prop(unions, "ordered")["constructors"],
        serde_json::json!(["String", "Boolean"])
    );
    assert_eq!(prop(unions, "booleanUnknown")["skipCheck"], true);
    assert_eq!(prop(unions, "functionUnknown")["skipCheck"], true);
    assert_eq!(
        prop(unions, "numberUnknown")["constructors"],
        serde_json::json!([])
    );

    let containers = case(&oracle, "containers-callables-and-nominals");
    for (name, constructor) in [
        ("array", "Array"),
        ("tuple", "Array"),
        ("callable", "Function"),
        ("date", "Date"),
        ("map", "Map"),
        ("set", "Set"),
        ("weakMap", "WeakMap"),
        ("weakSet", "WeakSet"),
        ("promise", "Promise"),
        ("error", "Error"),
        ("userClass", "Object"),
        ("userObject", "Object"),
    ] {
        assert_eq!(
            prop(containers, name)["constructors"],
            serde_json::json!([constructor])
        );
    }

    let model = case(&oracle, "define-model-default-and-named");
    assert_eq!(
        model["runtime"]["emits"],
        serde_json::json!(["update:modelValue", "update:count"])
    );
    assert_eq!(
        prop(model, "modelModifiers")["constructors"],
        serde_json::json!([])
    );
    assert_eq!(
        prop(model, "countModifiers")["constructors"],
        serde_json::json!([])
    );

    let ignored = case(&oracle, "vue-ignore");
    assert!(ignored["runtime"]["props"]
        .as_array()
        .expect("runtime props must be an array")
        .iter()
        .all(|prop| prop["name"] != "ignored"));

    let imported = case(&oracle, "imported-utility-and-indexed");
    assert_eq!(
        prop(imported, "count")["constructors"],
        serde_json::json!(["Number"])
    );
    assert_eq!(
        prop(imported, "options")["constructors"],
        serde_json::json!(["Object"])
    );
    assert_eq!(
        prop(imported, "selected")["constructors"],
        serde_json::json!(["Object"])
    );

    let profiles = case(&oracle, "profile-default-rendering");
    assert_eq!(
        profile_prop(profiles, "development", "text"),
        &serde_json::json!({
            "name": "text",
            "typePresent": true,
            "constructors": ["String"],
            "required": true,
            "skipCheck": false,
            "defaultKind": "property",
            "default": "'fallback'"
        })
    );
    assert_eq!(
        profile_prop(profiles, "production", "text")["constructors"],
        serde_json::json!([])
    );
    assert_eq!(
        profile_prop(profiles, "production-custom-element", "text")["constructors"],
        serde_json::json!(["String"])
    );
    assert_eq!(
        profile_prop(profiles, "production", "enabled")["constructors"],
        serde_json::json!(["Boolean"])
    );
    assert_eq!(
        profile_prop(profiles, "production-custom-element", "opaque")["constructors"],
        serde_json::json!([])
    );
    assert_eq!(
        profile_prop(profiles, "production", "opaque")["typePresent"],
        false,
        "production strips the Unknown runtime type field"
    );
    assert_eq!(
        profile_prop(profiles, "production-custom-element", "opaque")["typePresent"],
        true,
        "custom-element production retains the explicit type: null field"
    );
    for profile_name in ["development", "production", "production-custom-element"] {
        let method = profile_prop(profiles, profile_name, "method");
        assert_eq!(method["defaultKind"], "method");
        assert_eq!(method["default"], "default() { return 2 }");
        assert_eq!(method["constructors"], serde_json::json!(["Function"]));
    }

    let extension = case(&oracle, "complete-imported-extension");
    assert_eq!(extension["contract"], "verter-complete-extension");
    assert_eq!(extension["extensionPolicy"], "refine-only-on-complete");
    assert_eq!(
        profile_prop(extension, "development", "payload")["constructors"],
        serde_json::json!([]),
        "the official baseline is Unknown; Verter may refine only after a complete canonical result"
    );
}
