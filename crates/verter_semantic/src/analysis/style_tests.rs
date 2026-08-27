use super::*;

fn analyze_css(css: &str) -> StyleBlockAnalysis {
    build_css_style_analysis(css, VueStyleInput::default(), false, false, None, 0)
}

#[test]
fn test_css_analysis_selectors_and_specificity() {
    let analysis = analyze_css(
        r#"
            .btn { color: red; }
            #app .main { display: flex; }
            div > p.active { font-size: 14px; }
        "#,
    );

    let css = analysis.css.as_ref().expect("should have CSS analysis");
    assert_eq!(css.selectors.len(), 3);
    assert_eq!(css.rule_count, 3);

    // .btn → specificity (0, 1, 0)
    let btn = &css.selectors[0];
    assert_eq!(btn.text, ".btn");
    assert_eq!(btn.specificity, (0, 1, 0));

    // #app .main → specificity (1, 1, 0)
    let app_main = &css.selectors[1];
    assert_eq!(app_main.text, "#app .main");
    assert_eq!(app_main.specificity, (1, 1, 0));

    // div > p.active → specificity (0, 1, 2)
    let div_p = &css.selectors[2];
    assert_eq!(div_p.text, "div > p.active");
    assert_eq!(div_p.specificity, (0, 1, 2));
}

#[test]
fn test_css_analysis_classes() {
    let analysis = analyze_css(
        r#"
            .btn { color: red; }
            .active { display: none; }
            .btn.primary { background: blue; }
        "#,
    );

    let css = analysis.css.as_ref().unwrap();
    let class_names: Vec<&str> = css.classes.iter().map(|c| c.name.as_str()).collect();
    assert!(class_names.contains(&"btn"));
    assert!(class_names.contains(&"active"));
    assert!(class_names.contains(&"primary"));
}

#[test]
fn test_css_analysis_ids() {
    let analysis = analyze_css(
        r#"
            #app { margin: 0; }
            #main { display: flex; }
        "#,
    );

    let css = analysis.css.as_ref().unwrap();
    let id_names: Vec<&str> = css.ids.iter().map(|i| i.name.as_str()).collect();
    assert!(id_names.contains(&"app"));
    assert!(id_names.contains(&"main"));
}

#[test]
fn test_css_analysis_custom_properties() {
    let analysis = analyze_css(
        r#"
            :root {
                --primary-color: #333;
                --spacing-lg: 24px;
            }
        "#,
    );

    let css = analysis.css.as_ref().unwrap();
    let prop_names: Vec<&str> = css
        .custom_properties
        .iter()
        .map(|p| p.name.as_str())
        .collect();
    assert!(prop_names.contains(&"--primary-color"));
    assert!(prop_names.contains(&"--spacing-lg"));
}

#[test]
fn test_css_analysis_at_rules() {
    let analysis = analyze_css(
        r#"
            @media (max-width: 768px) { .btn { display: none; } }
            @keyframes fadeIn { from { opacity: 0; } to { opacity: 1; } }
            @layer utilities;
        "#,
    );

    let css = analysis.css.as_ref().unwrap();
    assert!(css.at_rules.iter().any(|r| r.kind == AtRuleKind::Media));
    assert!(css
        .at_rules
        .iter()
        .any(|r| r.kind == AtRuleKind::Keyframes && r.name == "fadeIn"));
    assert!(css.at_rules.iter().any(|r| r.kind == AtRuleKind::Layer));
}

#[test]
fn test_css_analysis_with_vue_input() {
    let vue_input = VueStyleInput {
        v_binds: vec![VBindInput {
            expression: "color".to_string(),
            quoted: false,
            start: 10,
            end: 25,
            generated_var_name: None,
            expr_roots: Vec::new(),
            roots_complete: true,
        }],
        special_pseudos: vec![SpecialPseudoInput {
            kind: SpecialPseudoKind::Deep,
            start: 30,
            end: 50,
            inner: Some(".inner".to_string()),
        }],
    };

    let analysis =
        build_css_style_analysis(".btn { color: red; }", vue_input, true, false, None, 0);

    assert_eq!(analysis.v_binds.len(), 1);
    assert_eq!(analysis.v_binds[0].expression, "color");
    assert_eq!(analysis.special_pseudos.len(), 1);
    assert_eq!(analysis.special_pseudos[0].kind, SpecialPseudoKind::Deep);
    assert_eq!(analysis.special_pseudos[0].inner.as_deref(), Some(".inner"));
}

#[test]
fn test_preprocessor_no_css_parsing() {
    let vue_input = VueStyleInput {
        v_binds: vec![VBindInput {
            expression: "color".to_string(),
            quoted: false,
            start: 10,
            end: 25,
            generated_var_name: None,
            expr_roots: Vec::new(),
            roots_complete: true,
        }],
        special_pseudos: Vec::new(),
    };

    let analysis = build_preprocessor_style_analysis(
        StyleAnalysisLang::Scss,
        vue_input,
        false,
        false,
        None,
        0,
    );

    assert_eq!(analysis.lang, StyleAnalysisLang::Scss);
    assert!(analysis.css.is_none(), "SCSS should not have CSS analysis");
    assert_eq!(analysis.v_binds.len(), 1);
}

#[test]
fn test_flags_derived_correctly() {
    let vue_input = VueStyleInput {
        v_binds: vec![VBindInput {
            expression: "x".to_string(),
            quoted: false,
            start: 0,
            end: 5,
            generated_var_name: None,
            expr_roots: Vec::new(),
            roots_complete: true,
        }],
        special_pseudos: vec![
            SpecialPseudoInput {
                kind: SpecialPseudoKind::Deep,
                start: 0,
                end: 5,
                inner: None,
            },
            SpecialPseudoInput {
                kind: SpecialPseudoKind::Global,
                start: 0,
                end: 5,
                inner: None,
            },
            SpecialPseudoInput {
                kind: SpecialPseudoKind::Slotted,
                start: 0,
                end: 5,
                inner: None,
            },
        ],
    };

    let analysis = build_css_style_analysis(
        r#"
            @import "base.css";
            @layer reset;
            :root { --my-var: red; }
            @keyframes slide { from {} to {} }
            @container sidebar (min-width: 300px) { .card { display: flex; } }
        "#,
        vue_input,
        true,
        true,
        Some("styles"),
        0,
    );

    let flags = analysis.analysis_flags();
    assert!(flags.contains(StyleAnalysisFlags::SCOPED));
    assert!(flags.contains(StyleAnalysisFlags::MODULE));
    assert!(flags.contains(StyleAnalysisFlags::HAS_V_BIND));
    assert!(flags.contains(StyleAnalysisFlags::HAS_DEEP));
    assert!(flags.contains(StyleAnalysisFlags::HAS_GLOBAL));
    assert!(flags.contains(StyleAnalysisFlags::HAS_SLOTTED));
    assert!(flags.contains(StyleAnalysisFlags::HAS_CUSTOM_PROPS));
    assert!(flags.contains(StyleAnalysisFlags::HAS_KEYFRAMES));
    assert!(flags.contains(StyleAnalysisFlags::HAS_IMPORTS));
    assert!(flags.contains(StyleAnalysisFlags::HAS_LAYERS));
    assert!(flags.contains(StyleAnalysisFlags::HAS_CONTAINER_QUERIES));
    assert_eq!(analysis.module_name.as_deref(), Some("styles"));
}

#[test]
fn test_empty_css() {
    let analysis = analyze_css("");
    let css = analysis.css.as_ref().unwrap();
    assert!(css.selectors.is_empty());
    assert!(css.classes.is_empty());
    assert!(css.ids.is_empty());
    assert!(css.custom_properties.is_empty());
    assert!(css.at_rules.is_empty());
    assert_eq!(css.rule_count, 0);
}

#[test]
fn test_malformed_css_graceful() {
    // Recovery is local, so broken syntax still produces partial results.
    let analysis = analyze_css("{{{invalid$$$css}}}");
    // Even if it fails to parse, we should get a valid StyleBlockAnalysis
    assert_eq!(analysis.lang, StyleAnalysisLang::Css);
}

#[test]
fn test_multiple_selectors_per_rule() {
    let analysis = analyze_css(".a, .b, .c { color: red; }");
    let css = analysis.css.as_ref().unwrap();
    assert_eq!(css.selectors.len(), 3);
    assert_eq!(css.rule_count, 1);

    let names: Vec<&str> = css.selectors.iter().map(|s| s.text.as_str()).collect();
    assert!(names.contains(&".a"));
    assert!(names.contains(&".b"));
    assert!(names.contains(&".c"));
}

#[test]
fn test_nested_media_rules() {
    let analysis = analyze_css(
        r#"
            @media (max-width: 768px) {
                .mobile { display: block; }
                .desktop { display: none; }
            }
        "#,
    );

    let css = analysis.css.as_ref().unwrap();
    assert!(css.at_rules.iter().any(|r| r.kind == AtRuleKind::Media));
    let class_names: Vec<&str> = css.classes.iter().map(|c| c.name.as_str()).collect();
    assert!(class_names.contains(&"mobile"));
    assert!(class_names.contains(&"desktop"));
}

#[test]
fn test_native_css_nesting() {
    let analysis = analyze_css(
        r#"
            .parent {
                color: red;
                .child {
                    color: blue;
                }
            }
        "#,
    );

    let css = analysis.css.as_ref().unwrap();
    let class_names: Vec<&str> = css.classes.iter().map(|c| c.name.as_str()).collect();
    assert!(
        class_names.contains(&"parent"),
        "should extract parent class"
    );
    assert!(
        class_names.contains(&"child"),
        "should extract nested child class"
    );
    assert!(css.rule_count >= 1, "should count at least the outer rule");
}

#[test]
fn test_scope_at_rule() {
    let analysis = analyze_css(
        r#"
            @scope (.card) {
                .title { font-weight: bold; }
            }
        "#,
    );

    let css = analysis.css.as_ref().unwrap();
    assert!(
        css.at_rules.iter().any(|r| r.kind == AtRuleKind::Scope),
        "should detect @scope at-rule"
    );
    // Nested rules inside @scope should be walked
    let class_names: Vec<&str> = css.classes.iter().map(|c| c.name.as_str()).collect();
    assert!(
        class_names.contains(&"title"),
        "should extract classes inside @scope"
    );
}

#[test]
fn test_complex_selectors_with_combinators() {
    let analysis = analyze_css(
        r#"
            .a > .b { color: red; }
            .c + .d { color: green; }
            .e ~ .f { color: blue; }
        "#,
    );

    let css = analysis.css.as_ref().unwrap();
    assert_eq!(css.selectors.len(), 3);
    let class_names: Vec<&str> = css.classes.iter().map(|c| c.name.as_str()).collect();
    for name in &["a", "b", "c", "d", "e", "f"] {
        assert!(
            class_names.contains(name),
            "should extract class .{name} from combinator selector"
        );
    }
}

#[test]
fn test_container_with_name() {
    let analysis = analyze_css(
        r#"
            @container sidebar (min-width: 300px) {
                .card { display: flex; }
            }
        "#,
    );

    let css = analysis.css.as_ref().unwrap();
    let container_rule = css
        .at_rules
        .iter()
        .find(|r| r.kind == AtRuleKind::Container)
        .expect("should detect @container at-rule");
    assert_eq!(
        container_rule.name, "sidebar",
        "should capture container name"
    );
}

/// for compatibility with the TypeScript playground AnalysisPanel.
#[test]
fn test_style_block_analysis_serializes_camel_case() {
    let vue_input = VueStyleInput {
        v_binds: vec![VBindInput {
            expression: "color".to_string(),
            quoted: false,
            start: 10,
            end: 25,
            generated_var_name: None,
            expr_roots: Vec::new(),
            roots_complete: true,
        }],
        special_pseudos: vec![SpecialPseudoInput {
            kind: SpecialPseudoKind::Deep,
            start: 30,
            end: 50,
            inner: Some(".inner".to_string()),
        }],
    };

    let analysis = build_css_style_analysis(
        r#":root { --my-var: red; } @keyframes slide { from {} to {} }"#,
        vue_input,
        true,
        true,
        Some("styles"),
        0,
    );

    let json = serde_json::to_value(&analysis).expect("should serialize");
    let obj = json.as_object().expect("should be an object");

    // StyleBlockAnalysis fields must be camelCase
    assert!(
        obj.contains_key("isModule"),
        "expected 'isModule', got keys: {:?}",
        obj.keys().collect::<Vec<_>>()
    );
    assert!(
        obj.contains_key("moduleName"),
        "expected 'moduleName', got keys: {:?}",
        obj.keys().collect::<Vec<_>>()
    );
    assert!(
        obj.contains_key("vBinds"),
        "expected 'vBinds', got keys: {:?}",
        obj.keys().collect::<Vec<_>>()
    );
    assert!(
        obj.contains_key("specialPseudos"),
        "expected 'specialPseudos', got keys: {:?}",
        obj.keys().collect::<Vec<_>>()
    );

    // CssAnalysis fields must also be camelCase
    let css_obj = obj["css"].as_object().expect("css should be an object");
    assert!(
        css_obj.contains_key("customProperties"),
        "expected 'customProperties', got keys: {:?}",
        css_obj.keys().collect::<Vec<_>>()
    );
    assert!(
        css_obj.contains_key("atRules"),
        "expected 'atRules', got keys: {:?}",
        css_obj.keys().collect::<Vec<_>>()
    );
    assert!(
        css_obj.contains_key("ruleCount"),
        "expected 'ruleCount', got keys: {:?}",
        css_obj.keys().collect::<Vec<_>>()
    );
}

#[test]
fn test_font_face_detection() {
    let analysis = analyze_css(
        r#"
            @font-face {
                font-family: "CustomFont";
                src: url("font.woff2") format("woff2");
            }
        "#,
    );

    let css = analysis.css.as_ref().unwrap();
    assert!(
        css.at_rules.iter().any(|r| r.kind == AtRuleKind::FontFace),
        "should detect @font-face at-rule"
    );
}

#[test]
fn test_property_at_rule() {
    let analysis = analyze_css(
        r#"
            @property --my-color {
                syntax: "<color>";
                initial-value: red;
                inherits: false;
            }
        "#,
    );

    let css = analysis.css.as_ref().unwrap();
    let prop_rule = css
        .at_rules
        .iter()
        .find(|r| r.kind == AtRuleKind::Property)
        .expect("should detect @property at-rule");
    assert_eq!(prop_rule.name, "--my-color", "should capture property name");
}

#[test]
fn test_important_custom_properties() {
    let analysis = analyze_css(
        r#"
            .btn {
                --highlight: red !important;
            }
        "#,
    );

    let css = analysis.css.as_ref().unwrap();
    let prop_names: Vec<&str> = css
        .custom_properties
        .iter()
        .map(|p| p.name.as_str())
        .collect();
    assert!(
        prop_names.contains(&"--highlight"),
        "should extract !important custom properties, got: {:?}",
        prop_names
    );
}

// =====================================================================
// CSS selector span tests
// =====================================================================

#[test]
fn test_class_span_simple() {
    let css = ".btn { color: red; }";
    let analysis = analyze_css(css);
    let css_data = analysis.css.as_ref().unwrap();
    assert_eq!(css_data.classes.len(), 1);
    let cls = &css_data.classes[0];
    assert_eq!(cls.name, "btn");
    assert_eq!(&css[cls.span.start as usize..cls.span.end as usize], "btn");
}

#[test]
fn test_class_span_comma_separated() {
    let css = ".a, .b { color: red; }";
    let analysis = analyze_css(css);
    let css_data = analysis.css.as_ref().unwrap();
    assert_eq!(css_data.classes.len(), 2);

    assert_eq!(css_data.classes[0].name, "a");
    assert_eq!(
        &css[css_data.classes[0].span.start as usize..css_data.classes[0].span.end as usize],
        "a"
    );

    assert_eq!(css_data.classes[1].name, "b");
    assert_eq!(
        &css[css_data.classes[1].span.start as usize..css_data.classes[1].span.end as usize],
        "b"
    );
}

#[test]
fn test_class_span_nested() {
    let css = ".parent { .child { color: blue; } }";
    let analysis = analyze_css(css);
    let css_data = analysis.css.as_ref().unwrap();

    let parent = css_data
        .classes
        .iter()
        .find(|c| c.name == "parent")
        .unwrap();
    assert_eq!(
        &css[parent.span.start as usize..parent.span.end as usize],
        "parent"
    );

    let child = css_data.classes.iter().find(|c| c.name == "child").unwrap();
    assert_eq!(
        &css[child.span.start as usize..child.span.end as usize],
        "child"
    );
}

#[test]
fn test_id_span_simple() {
    let css = "#app { margin: 0; }";
    let analysis = analyze_css(css);
    let css_data = analysis.css.as_ref().unwrap();
    assert_eq!(css_data.ids.len(), 1);
    let id = &css_data.ids[0];
    assert_eq!(id.name, "app");
    assert_eq!(&css[id.span.start as usize..id.span.end as usize], "app");
}

#[test]
fn test_duplicate_class_distinct_spans() {
    let css = ".btn { color: red; } .btn { color: blue; }";
    let analysis = analyze_css(css);
    let css_data = analysis.css.as_ref().unwrap();

    let btns: Vec<&AnalyzedCssClass> = css_data
        .classes
        .iter()
        .filter(|c| c.name == "btn")
        .collect();
    assert_eq!(btns.len(), 2, "should have two .btn occurrences");
    assert_ne!(
        btns[0].span.start, btns[1].span.start,
        "each occurrence should have a distinct span"
    );
    assert_eq!(
        &css[btns[0].span.start as usize..btns[0].span.end as usize],
        "btn"
    );
    assert_eq!(
        &css[btns[1].span.start as usize..btns[1].span.end as usize],
        "btn"
    );
}

#[test]
fn test_class_span_inside_media() {
    let css = "@media (max-width: 768px) { .mobile { display: block; } }";
    let analysis = analyze_css(css);
    let css_data = analysis.css.as_ref().unwrap();

    let mobile = css_data
        .classes
        .iter()
        .find(|c| c.name == "mobile")
        .unwrap();
    assert_eq!(
        &css[mobile.span.start as usize..mobile.span.end as usize],
        "mobile"
    );
}

#[test]
fn test_no_spans_inside_keyframes() {
    // @keyframes blocks don't contain class/id selectors — just `from`, `to`, percentages
    let css = "@keyframes fadeIn { from { opacity: 0; } to { opacity: 1; } }";
    let analysis = analyze_css(css);
    let css_data = analysis.css.as_ref().unwrap();
    assert!(css_data.classes.is_empty());
    assert!(css_data.ids.is_empty());
}

#[test]
fn test_content_offset_stored() {
    let analysis = build_css_style_analysis(
        ".btn { color: red; }",
        VueStyleInput::default(),
        false,
        false,
        None,
        42,
    );
    assert_eq!(analysis.content_offset, 42);
}

#[test]
fn test_selector_class_and_id_spans_are_sfc_absolute() {
    let css = ".btn, #app { color: red; }";
    let content_offset = 100u32;
    let analysis = analyze_css_with_offset(css, content_offset);
    let css_data = analysis.css.as_ref().unwrap();

    let selector = css_data
        .selectors
        .iter()
        .find(|sel| sel.text == ".btn")
        .expect("should find selector");
    assert!(
        selector.span.start >= content_offset,
        "selector span should be SFC-absolute"
    );
    assert_eq!(
        &css[(selector.span.start - content_offset) as usize
            ..(selector.span.end - content_offset) as usize],
        ".btn"
    );

    let class = css_data
        .classes
        .iter()
        .find(|c| c.name == "btn")
        .expect("should find .btn");
    assert!(
        class.span.start >= content_offset,
        "class span should be SFC-absolute"
    );
    assert_eq!(
        &css[(class.span.start - content_offset) as usize
            ..(class.span.end - content_offset) as usize],
        "btn"
    );

    let id = css_data
        .ids
        .iter()
        .find(|i| i.name == "app")
        .expect("should find #app");
    assert!(
        id.span.start >= content_offset,
        "id span should be SFC-absolute"
    );
    assert_eq!(
        &css[(id.span.start - content_offset) as usize..(id.span.end - content_offset) as usize],
        "app"
    );
}

#[test]
fn test_compound_selector_spans() {
    let css = ".btn.primary { color: red; }";
    let analysis = analyze_css(css);
    let css_data = analysis.css.as_ref().unwrap();

    let btn = css_data.classes.iter().find(|c| c.name == "btn").unwrap();
    assert_eq!(&css[btn.span.start as usize..btn.span.end as usize], "btn");

    let primary = css_data
        .classes
        .iter()
        .find(|c| c.name == "primary")
        .unwrap();
    assert_eq!(
        &css[primary.span.start as usize..primary.span.end as usize],
        "primary"
    );
}

// =====================================================================
// Structured selector parser tests
// =====================================================================

#[test]
fn test_parse_selector_simple_class() {
    let sel = parse_selector(".btn").unwrap();
    assert_eq!(sel.compounds.len(), 1);
    assert!(sel.combinators.is_empty());
    assert_eq!(sel.compounds[0].classes, vec!["btn"]);
    assert!(sel.compounds[0].element.is_none());
    assert!(sel.compounds[0].id.is_none());
}

#[test]
fn test_parse_selector_compound_classes() {
    let sel = parse_selector(".foo.bar").unwrap();
    assert_eq!(sel.compounds.len(), 1);
    assert_eq!(sel.compounds[0].classes, vec!["foo", "bar"]);
}

#[test]
fn test_parse_selector_type_and_class() {
    let sel = parse_selector("div.active").unwrap();
    assert_eq!(sel.compounds.len(), 1);
    assert_eq!(sel.compounds[0].element.as_deref(), Some("div"));
    assert_eq!(sel.compounds[0].classes, vec!["active"]);
}

#[test]
fn test_parse_selector_id() {
    let sel = parse_selector("#app").unwrap();
    assert_eq!(sel.compounds.len(), 1);
    assert_eq!(sel.compounds[0].id.as_deref(), Some("app"));
}

#[test]
fn test_parse_selector_descendant() {
    let sel = parse_selector(".parent .child").unwrap();
    assert_eq!(sel.compounds.len(), 2);
    assert_eq!(sel.combinators, vec![SelectorCombinator::Descendant]);
    assert_eq!(sel.compounds[0].classes, vec!["parent"]);
    assert_eq!(sel.compounds[1].classes, vec!["child"]);
}

#[test]
fn test_parse_selector_child() {
    let sel = parse_selector(".parent > .child").unwrap();
    assert_eq!(sel.compounds.len(), 2);
    assert_eq!(sel.combinators, vec![SelectorCombinator::Child]);
    assert_eq!(sel.compounds[0].classes, vec!["parent"]);
    assert_eq!(sel.compounds[1].classes, vec!["child"]);
}

#[test]
fn test_parse_selector_next_sibling() {
    let sel = parse_selector(".a + .b").unwrap();
    assert_eq!(sel.compounds.len(), 2);
    assert_eq!(sel.combinators, vec![SelectorCombinator::NextSibling]);
}

#[test]
fn test_parse_selector_later_sibling() {
    let sel = parse_selector(".a ~ .b").unwrap();
    assert_eq!(sel.compounds.len(), 2);
    assert_eq!(sel.combinators, vec![SelectorCombinator::LaterSibling]);
}

#[test]
fn test_parse_selector_attribute() {
    let sel = parse_selector("[type=\"text\"]").unwrap();
    assert_eq!(sel.compounds.len(), 1);
    assert_eq!(sel.compounds[0].attributes.len(), 1);
    let attr = &sel.compounds[0].attributes[0];
    assert_eq!(attr.name, "type");
    assert_eq!(attr.operator, Some(AttributeOperator::Equal));
    assert_eq!(attr.value.as_deref(), Some("text"));
}

#[test]
fn test_parse_selector_attribute_presence() {
    let sel = parse_selector("[disabled]").unwrap();
    assert_eq!(sel.compounds[0].attributes.len(), 1);
    let attr = &sel.compounds[0].attributes[0];
    assert_eq!(attr.name, "disabled");
    assert!(attr.operator.is_none());
    assert!(attr.value.is_none());
}

#[test]
fn test_parse_selector_pseudo_hover() {
    let sel = parse_selector(".btn:hover").unwrap();
    assert_eq!(sel.compounds.len(), 1);
    assert_eq!(sel.compounds[0].classes, vec!["btn"]);
    assert_eq!(sel.compounds[0].pseudo_classes.len(), 1);
    assert!(matches!(
        &sel.compounds[0].pseudo_classes[0],
        SelectorPseudoClass::Runtime(name) if name == "hover"
    ));
}

#[test]
fn test_parse_selector_pseudo_element() {
    let sel = parse_selector(".btn::before").unwrap();
    assert_eq!(sel.compounds.len(), 1);
    assert!(sel.compounds[0].has_pseudo_element);
}

#[test]
fn test_parse_selector_not() {
    let sel = parse_selector(":not(.hidden)").unwrap();
    assert_eq!(sel.compounds.len(), 1);
    assert_eq!(sel.compounds[0].pseudo_classes.len(), 1);
    if let SelectorPseudoClass::Not(inner) = &sel.compounds[0].pseudo_classes[0] {
        assert_eq!(inner.len(), 1);
        assert_eq!(inner[0].compounds[0].classes, vec!["hidden"]);
    } else {
        panic!("expected :not()");
    }
}

#[test]
fn test_parse_selector_is() {
    let sel = parse_selector(":is(.a, .b)").unwrap();
    assert_eq!(sel.compounds.len(), 1);
    if let SelectorPseudoClass::Is(inner) = &sel.compounds[0].pseudo_classes[0] {
        assert_eq!(inner.len(), 2);
    } else {
        panic!("expected :is()");
    }
}

#[test]
fn test_parse_selector_where() {
    let sel = parse_selector(":where(.a)").unwrap();
    if let SelectorPseudoClass::Where(inner) = &sel.compounds[0].pseudo_classes[0] {
        assert_eq!(inner.len(), 1);
    } else {
        panic!("expected :where()");
    }
}

#[test]
fn test_parse_selector_has_returns_none() {
    assert!(parse_selector(".parent:has(.child)").is_none());
}

#[test]
fn test_parse_selector_universal() {
    let sel = parse_selector("*").unwrap();
    assert_eq!(sel.compounds.len(), 1);
    assert!(sel.compounds[0].element.is_none());
    assert!(sel.compounds[0].classes.is_empty());
}

#[test]
fn test_parse_selector_complex() {
    let sel = parse_selector("#app > .main .content:hover").unwrap();
    assert_eq!(sel.compounds.len(), 3);
    assert_eq!(
        sel.combinators,
        vec![SelectorCombinator::Child, SelectorCombinator::Descendant]
    );
    assert_eq!(sel.compounds[0].id.as_deref(), Some("app"));
    assert_eq!(sel.compounds[1].classes, vec!["main"]);
    assert_eq!(sel.compounds[2].classes, vec!["content"]);
}

#[test]
fn test_parse_selector_empty() {
    assert!(parse_selector("").is_none());
    assert!(parse_selector("  ").is_none());
}

#[test]
fn test_structured_specificity() {
    // .btn → (0, 1, 0)
    let sel = parse_selector(".btn").unwrap();
    assert_eq!(compute_structured_specificity(&sel), (0, 1, 0));

    // #app .main → (1, 1, 0)
    let sel = parse_selector("#app .main").unwrap();
    assert_eq!(compute_structured_specificity(&sel), (1, 1, 0));

    // div > p.active → (0, 1, 2)
    let sel = parse_selector("div > p.active").unwrap();
    assert_eq!(compute_structured_specificity(&sel), (0, 1, 2));

    // :where(.foo) → (0, 0, 0)
    let sel = parse_selector(":where(.foo)").unwrap();
    assert_eq!(compute_structured_specificity(&sel), (0, 0, 0));

    // :not(.foo) → (0, 1, 0)
    let sel = parse_selector(":not(.foo)").unwrap();
    assert_eq!(compute_structured_specificity(&sel), (0, 1, 0));

    // [type="text"] → (0, 1, 0)
    let sel = parse_selector("[type=\"text\"]").unwrap();
    assert_eq!(compute_structured_specificity(&sel), (0, 1, 0));

    // ::before → (0, 0, 1)
    let sel = parse_selector("::before").unwrap();
    assert_eq!(compute_structured_specificity(&sel), (0, 0, 1));
}

#[test]
fn test_selectors_have_structure() {
    let analysis = analyze_css(".btn { color: red; }");
    let css = analysis.css.as_ref().unwrap();
    let sel = &css.selectors[0];
    assert!(sel.structure.is_some());
    let s = sel.structure.as_ref().unwrap();
    assert_eq!(s.compounds[0].classes, vec!["btn"]);
}

#[test]
fn test_parse_attribute_operators() {
    // ~= includes
    let sel = parse_selector("[class~=\"active\"]").unwrap();
    assert_eq!(
        sel.compounds[0].attributes[0].operator,
        Some(AttributeOperator::Includes)
    );

    // ^= prefix
    let sel = parse_selector("[href^=\"https\"]").unwrap();
    assert_eq!(
        sel.compounds[0].attributes[0].operator,
        Some(AttributeOperator::Prefix)
    );

    // $= suffix
    let sel = parse_selector("[href$=\".pdf\"]").unwrap();
    assert_eq!(
        sel.compounds[0].attributes[0].operator,
        Some(AttributeOperator::Suffix)
    );

    // *= substring
    let sel = parse_selector("[data-id*=\"test\"]").unwrap();
    assert_eq!(
        sel.compounds[0].attributes[0].operator,
        Some(AttributeOperator::Substring)
    );

    // |= dash-match
    let sel = parse_selector("[lang|=\"en\"]").unwrap();
    assert_eq!(
        sel.compounds[0].attributes[0].operator,
        Some(AttributeOperator::DashMatch)
    );
}

#[test]
fn selector_with_inline_comment_parsed() {
    let analysis = analyze_css(".a /* comment */ > .b { color: red; }");
    let css = analysis.css.as_ref().expect("should have CSS analysis");
    assert_eq!(css.selectors.len(), 1);
    // The syntax authority is lossless: comments stay in the selector span.
    assert!(
        css.selectors[0].text.contains("/* comment */"),
        "selector text should retain CSS comments"
    );
    // Structural parse should succeed
    assert!(
        css.selectors[0].structure.is_some(),
        "selector with comment should still parse structurally"
    );
}

#[test]
fn nested_selector_text_does_not_contain_declarations() {
    // Bug: hovering `background-color` in nested CSS shows entire rule block content
    // because selector_start includes property declarations before the nested selector.
    let analysis = analyze_css(
        r#".ns-popover {
  background-color: transparent;
  overflow: visible;
  &__content {
    padding: 8px;
  }
}"#,
    );

    let css = analysis.css.as_ref().expect("should have CSS analysis");

    // Find all selectors
    let selector_texts: Vec<&str> = css.selectors.iter().map(|s| s.text.as_str()).collect();

    // Positive: should contain the parent and nested selectors.
    assert!(
        selector_texts.contains(&".ns-popover"),
        "should contain .ns-popover selector: {:?}",
        selector_texts
    );
    assert!(
        selector_texts.iter().any(|s| s.contains("&__content")),
        "should contain &__content nested selector: {:?}",
        selector_texts
    );

    // Negative: no selector text should contain CSS property declarations
    for sel in &css.selectors {
        assert!(
            !sel.text.contains("background-color"),
            "selector '{}' should NOT contain property declarations",
            sel.text
        );
        assert!(
            !sel.text.contains("overflow"),
            "selector '{}' should NOT contain property declarations",
            sel.text
        );
    }
}

// =============================================================================
// CSS Variable Analysis Tests
// =============================================================================

fn analyze_css_with_offset(css: &str, content_offset: u32) -> StyleBlockAnalysis {
    build_css_style_analysis(
        css,
        VueStyleInput::default(),
        false,
        false,
        None,
        content_offset,
    )
}

/// @ai-generated - Custom properties include name spans, values, and value spans
#[test]
fn test_custom_property_full_details() {
    let css = ":root { --color: red; --spacing: 10px; }";
    let analysis = analyze_css_with_offset(css, 100);
    let css_analysis = analysis.css.as_ref().unwrap();

    assert_eq!(css_analysis.custom_properties.len(), 2);

    let color_prop = &css_analysis.custom_properties[0];
    assert_eq!(color_prop.name, "--color");
    assert_eq!(color_prop.value, "red");
    // name_span should be SFC-absolute (content_offset + position in css)
    assert_eq!(
        &css[color_prop.name_span.start as usize - 100..color_prop.name_span.end as usize - 100],
        "--color"
    );
    assert_eq!(
        &css[color_prop.value_span.start as usize - 100..color_prop.value_span.end as usize - 100],
        "red"
    );

    let spacing_prop = &css_analysis.custom_properties[1];
    assert_eq!(spacing_prop.name, "--spacing");
    assert_eq!(spacing_prop.value, "10px");
}

/// @ai-generated - Custom properties have selector_index linking back to their rule
#[test]
fn test_custom_property_selector_index() {
    let css = ".dark { --bg: black; } .light { --bg: white; }";
    let analysis = analyze_css_with_offset(css, 0);
    let css_analysis = analysis.css.as_ref().unwrap();

    assert_eq!(css_analysis.custom_properties.len(), 2);

    let dark_prop = &css_analysis.custom_properties[0];
    assert_eq!(dark_prop.name, "--bg");
    assert_eq!(dark_prop.value, "black");
    assert!(dark_prop.selector_index.is_some());
    let dark_idx = dark_prop.selector_index.unwrap() as usize;
    assert_eq!(css_analysis.selectors[dark_idx].text, ".dark");

    let light_prop = &css_analysis.custom_properties[1];
    assert_eq!(light_prop.name, "--bg");
    assert_eq!(light_prop.value, "white");
    assert!(light_prop.selector_index.is_some());
    let light_idx = light_prop.selector_index.unwrap() as usize;
    assert_eq!(css_analysis.selectors[light_idx].text, ".light");
}

/// @ai-generated - var() references within custom property values are extracted
#[test]
fn test_custom_property_with_var_references() {
    let css = ":root { --accent: var(--primary-color); }";
    let analysis = analyze_css_with_offset(css, 0);
    let css_analysis = analysis.css.as_ref().unwrap();

    assert_eq!(css_analysis.custom_properties.len(), 1);
    let prop = &css_analysis.custom_properties[0];
    assert_eq!(prop.name, "--accent");
    assert_eq!(prop.value, "var(--primary-color)");
    assert_eq!(prop.var_references.len(), 1);

    let var_ref = &prop.var_references[0];
    assert_eq!(var_ref.name, "--primary-color");
    assert!(var_ref.fallback.is_none());
}

/// @ai-generated - var() with fallback is parsed correctly
#[test]
fn test_var_reference_with_fallback() {
    let css = ".box { color: var(--text, black); }";
    let analysis = analyze_css_with_offset(css, 0);
    let css_analysis = analysis.css.as_ref().unwrap();

    assert_eq!(css_analysis.var_usages.len(), 1);
    let usage = &css_analysis.var_usages[0];
    assert_eq!(usage.property_name, "color");
    assert_eq!(usage.reference.name, "--text");
    assert!(usage.reference.fallback.is_some());
    let fallback = usage.reference.fallback.as_ref().unwrap();
    assert_eq!(fallback.text, "black");
    assert!(fallback.nested_var_references.is_empty());
}

/// @ai-generated - Nested var() in fallback is parsed
#[test]
fn test_nested_var_in_fallback() {
    let css = ".box { color: var(--text, var(--fallback-text, blue)); }";
    let analysis = analyze_css_with_offset(css, 0);
    let css_analysis = analysis.css.as_ref().unwrap();

    assert_eq!(css_analysis.var_usages.len(), 1);
    let usage = &css_analysis.var_usages[0];
    assert_eq!(usage.reference.name, "--text");

    let fallback = usage.reference.fallback.as_ref().unwrap();
    assert_eq!(fallback.nested_var_references.len(), 1);
    let nested = &fallback.nested_var_references[0];
    assert_eq!(nested.name, "--fallback-text");
    assert!(nested.fallback.is_some());
    assert_eq!(nested.fallback.as_ref().unwrap().text, "blue");
}

/// @ai-generated - var_usages tracks non-custom-property declarations using var()
#[test]
fn test_var_usages_for_regular_properties() {
    let css = ".btn { color: var(--primary); background: var(--bg, white); }";
    let analysis = analyze_css_with_offset(css, 0);
    let css_analysis = analysis.css.as_ref().unwrap();

    // No custom properties defined
    assert!(css_analysis.custom_properties.is_empty());
    // Two var() usages
    assert_eq!(css_analysis.var_usages.len(), 2);

    let color_usage = &css_analysis.var_usages[0];
    assert_eq!(color_usage.property_name, "color");
    assert_eq!(color_usage.reference.name, "--primary");

    let bg_usage = &css_analysis.var_usages[1];
    assert_eq!(bg_usage.property_name, "background");
    assert_eq!(bg_usage.reference.name, "--bg");
}

/// @ai-generated - AnalyzedVBind carries generated_var_name
#[test]
fn test_v_bind_generated_var_name() {
    let vue_input = VueStyleInput {
        v_binds: vec![VBindInput {
            expression: "color".to_string(),
            quoted: false,
            start: 10,
            end: 25,
            generated_var_name: Some("--a4f2eed6-color".to_string()),
            expr_roots: Vec::new(),
            roots_complete: true,
        }],
        special_pseudos: vec![],
    };
    let analysis =
        build_css_style_analysis(".btn { color: red; }", vue_input, true, false, None, 0);
    assert_eq!(analysis.v_binds.len(), 1);
    assert_eq!(analysis.v_binds[0].expression, "color");
    assert_eq!(
        analysis.v_binds[0].generated_var_name.as_deref(),
        Some("--a4f2eed6-color")
    );
}

/// @ai-generated - extract_var_references parses simple var()
#[test]
fn test_extract_var_references_simple() {
    let refs = extract_var_references("var(--color)", 0, 0);
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].name, "--color");
    assert!(refs[0].fallback.is_none());
}

/// @ai-generated - extract_var_references parses var() with fallback
#[test]
fn test_extract_var_references_with_fallback() {
    let refs = extract_var_references("var(--color, red)", 0, 0);
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].name, "--color");
    let fb = refs[0].fallback.as_ref().unwrap();
    assert_eq!(fb.text, "red");
}

/// @ai-generated - extract_var_references handles multiple var() in one value
#[test]
fn test_extract_var_references_multiple() {
    let refs = extract_var_references("var(--a) 10px var(--b)", 0, 0);
    assert_eq!(refs.len(), 2);
    assert_eq!(refs[0].name, "--a");
    assert_eq!(refs[1].name, "--b");
}

/// @ai-generated - extract_var_references handles nested var() in fallback
#[test]
fn test_extract_var_references_nested() {
    let refs = extract_var_references("var(--a, var(--b, blue))", 0, 0);
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].name, "--a");
    let fb = refs[0].fallback.as_ref().unwrap();
    assert_eq!(fb.nested_var_references.len(), 1);
    assert_eq!(fb.nested_var_references[0].name, "--b");
}

/// @ai-generated - Custom property value with !important is trimmed correctly
#[test]
fn test_custom_property_value_important() {
    let css = ":root { --color: red !important; }";
    let analysis = analyze_css_with_offset(css, 0);
    let css_analysis = analysis.css.as_ref().unwrap();
    assert_eq!(css_analysis.custom_properties.len(), 1);
    assert_eq!(css_analysis.custom_properties[0].value, "red !important");
}

/// @ai-generated - var() spans are SFC-absolute when content_offset is set
#[test]
fn test_var_reference_spans_are_sfc_absolute() {
    let css = ".box { color: var(--c); }";
    let content_offset = 200u32;
    let analysis = analyze_css_with_offset(css, content_offset);
    let css_analysis = analysis.css.as_ref().unwrap();

    assert_eq!(css_analysis.var_usages.len(), 1);
    let var_ref = &css_analysis.var_usages[0].reference;
    // The var(...) starts at position 14 in the CSS string
    assert!(var_ref.span.start >= content_offset);
    assert!(var_ref.span.end > var_ref.span.start);
    // Verify the name_span points to --c
    let name_start = (var_ref.name_span.start - content_offset) as usize;
    let name_end = (var_ref.name_span.end - content_offset) as usize;
    assert_eq!(&css[name_start..name_end], "--c");
}

#[test]
fn test_debug_assert_valid_spans_passes_for_correct_offset() {
    // SFC source: "<style>\n.btn { color: red; }\n</style>"
    let sfc = "<style>\n.btn { color: red; }\n</style>";
    let css = ".btn { color: red; }\n";
    let content_offset = 8u32; // length of "<style>\n"
    let sfc_source_len = sfc.len() as u32;

    let analysis = build_css_style_analysis(
        css,
        VueStyleInput::default(),
        false,
        false,
        None,
        content_offset,
    );
    let css_analysis = analysis.css.as_ref().expect("should have CSS analysis");

    // This must not panic — all spans are within [content_offset, sfc_source_len)
    css_analysis.debug_assert_valid_spans(sfc_source_len);

    // Verify spans are actually SFC-absolute (>= content_offset)
    assert!(
        !css_analysis.classes.is_empty(),
        "should have at least one class"
    );
    for cls in &css_analysis.classes {
        assert!(
            cls.span.start >= content_offset,
            "class span start should be SFC-absolute"
        );
        assert!(
            cls.span.end <= sfc_source_len,
            "class span end should be within SFC"
        );
    }
    for sel in &css_analysis.selectors {
        assert!(
            sel.span.start >= content_offset,
            "selector span start should be SFC-absolute"
        );
        assert!(
            sel.span.end <= sfc_source_len,
            "selector span end should be within SFC"
        );
    }
}

#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "CSS span out of bounds")]
fn test_debug_assert_valid_spans_panics_for_double_offset() {
    // Simulate a double-offset bug: content_offset is applied twice
    // by passing a large content_offset that pushes spans beyond the SFC length
    let css = ".btn { color: red; }";
    let content_offset = 500u32; // way beyond the actual SFC length
    let sfc_source_len = 100u32; // smaller than content_offset

    let analysis = build_css_style_analysis(
        css,
        VueStyleInput::default(),
        false,
        false,
        None,
        content_offset,
    );
    let css_analysis = analysis.css.as_ref().expect("should have CSS analysis");

    // This should panic because spans (500+) exceed sfc_source_len (100)
    css_analysis.debug_assert_valid_spans(sfc_source_len);
}

// ── B4: class → selector join, rule body spans, SCSS/Less dialect ──

fn analyze_scss(scss: &str) -> StyleBlockAnalysis {
    build_scanned_style_analysis(
        StyleAnalysisLang::Scss,
        scss,
        VueStyleInput::default(),
        true,
        false,
        None,
        0,
    )
}

#[test]
fn class_selector_index_joins_each_comma_part() {
    let analysis = analyze_css(".a, .b .a { color: red; }\n.solo { color: blue; }");
    let css = analysis.css.as_ref().unwrap();
    assert_eq!(css.selectors.len(), 3);
    assert_eq!(css.selectors[0].text, ".a");
    assert_eq!(css.selectors[1].text, ".b .a");
    assert_eq!(css.selectors[2].text, ".solo");

    // classes in scan order: a (part 0), b (part 1), a (part 1), solo (sel 2)
    let joins: Vec<(&str, Option<u32>)> = css
        .classes
        .iter()
        .map(|c| (c.name.as_str(), c.selector_index))
        .collect();
    assert_eq!(
        joins,
        vec![
            ("a", Some(0)),
            ("b", Some(1)),
            ("a", Some(1)),
            ("solo", Some(2)),
        ]
    );
}

#[test]
fn selector_rule_body_span_covers_braces() {
    let src = ".btn { color: red; }";
    let analysis = analyze_css(src);
    let css = analysis.css.as_ref().unwrap();
    let body = css.selectors[0].rule_body_span.expect("body span");
    assert_eq!(
        &src[body.start as usize..body.end as usize],
        "{ color: red; }"
    );
}

#[test]
fn nested_rule_body_spans_are_exact_inner_and_outer() {
    let src = ".outer { color: red; .inner { color: blue; } }";
    let analysis = analyze_scss(src);
    let css = analysis.css.as_ref().unwrap();
    let outer = css.selectors.iter().find(|s| s.text == ".outer").unwrap();
    let inner = css.selectors.iter().find(|s| s.text == ".inner").unwrap();
    let outer_body = outer.rule_body_span.unwrap();
    let inner_body = inner.rule_body_span.unwrap();
    assert_eq!(
        &src[inner_body.start as usize..inner_body.end as usize],
        "{ color: blue; }"
    );
    assert_eq!(
        &src[outer_body.start as usize..outer_body.end as usize],
        "{ color: red; .inner { color: blue; } }"
    );
}

#[test]
fn unclosed_rule_has_no_body_span() {
    let analysis = analyze_css(".open { color: red;");
    let css = analysis.css.as_ref().unwrap();
    assert_eq!(css.selectors[0].text, ".open");
    assert!(css.selectors[0].rule_body_span.is_none());
}

#[test]
fn scss_nested_classes_have_exact_spans_and_css_some() {
    let src = ".card {\n  .title { color: red; }\n  &.active { color: blue; }\n}";
    let analysis = analyze_scss(src);
    let css = analysis.css.as_ref().expect("scss must scan to css facts");
    let class_at = |name: &str| {
        css.classes
            .iter()
            .find(|c| c.name == name)
            .unwrap_or_else(|| panic!("class {name} missing"))
    };
    for name in ["card", "title", "active"] {
        let cls = class_at(name);
        assert_eq!(
            &src[cls.span.start as usize..cls.span.end as usize],
            name,
            "span of {name} must cover exactly the authored name"
        );
    }
}

#[test]
fn scss_amp_selector_fails_closed_on_structure_but_extracts_literal_class() {
    let analysis = analyze_scss(".card { &.active { color: blue; } }");
    let css = analysis.css.as_ref().unwrap();
    let amp_sel = css.selectors.iter().find(|s| s.text == "&.active").unwrap();
    assert!(
        amp_sel.structure.is_none(),
        "an &-selector has no self-contained structure"
    );
    assert!(css.classes.iter().any(|c| c.name == "active"));
}

#[test]
fn scss_line_comment_never_yields_a_class() {
    let src = "// .ghost { color: red; }\n.real { color: blue; }";
    let analysis = analyze_scss(src);
    let css = analysis.css.as_ref().unwrap();
    let names: Vec<&str> = css.classes.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, vec!["real"], "commented-out selector is not a class");
    // The real selector keeps an exact span even with the leading comment.
    let real = &css.selectors[0];
    assert_eq!(
        &src[real.span.start as usize..real.span.end as usize],
        ".real"
    );
}

#[test]
fn scss_interpolated_selector_fails_closed() {
    let src = ".icon-#{$name} { color: red; }\n.real { color: blue; }";
    let analysis = analyze_scss(src);
    let css = analysis.css.as_ref().unwrap();
    assert!(
        !css.classes.iter().any(|c| c.name.starts_with("icon")),
        "an interpolated selector must not yield a partial class name"
    );
    assert!(css.classes.iter().any(|c| c.name == "real"));
    let interp = css.selectors.iter().find(|s| s.text.contains("#{"));
    if let Some(sel) = interp {
        assert!(
            sel.structure.is_none(),
            "interpolated structure fails closed"
        );
    }
}

#[test]
fn scss_variables_and_mixins_do_not_yield_classes() {
    let src =
        "$primary: #333;\n@mixin pad { padding: 4px; }\n.uses { @include pad; color: $primary; }";
    let analysis = analyze_scss(src);
    let css = analysis.css.as_ref().unwrap();
    let names: Vec<&str> = css.classes.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, vec!["uses"]);
}

#[test]
fn selector_comment_class_is_not_extracted() {
    let analysis = analyze_css(".a /* .ghost */ .b { color: red; }");
    let css = analysis.css.as_ref().unwrap();
    let names: Vec<&str> = css.classes.iter().map(|c| c.name.as_str()).collect();
    assert!(names.contains(&"a"));
    assert!(names.contains(&"b"));
    assert!(!names.contains(&"ghost"), "comment content is not a class");
}

#[test]
fn special_pseudo_global_recorded_with_span_and_inner() {
    let src = ".a :global(.g) { color: red; }";
    let analysis = build_css_style_analysis(src, VueStyleInput::default(), true, false, None, 0);
    let pseudo = analysis
        .special_pseudos
        .iter()
        .find(|p| p.kind == SpecialPseudoKind::Global)
        .expect(":global must be recorded by the syntax projection");
    assert_eq!(
        &src[pseudo.start as usize..pseudo.end as usize],
        ":global(.g)"
    );
    assert_eq!(pseudo.inner.as_deref(), Some(".g"));
    assert!(analysis
        .analysis_flags()
        .contains(StyleAnalysisFlags::HAS_GLOBAL));
    // The inner class is still extracted with an exact span.
    let css = analysis.css.as_ref().unwrap();
    let g = css.classes.iter().find(|c| c.name == "g").unwrap();
    assert_eq!(&src[g.span.start as usize..g.span.end as usize], "g");
}

#[test]
fn special_pseudo_deep_and_slotted_recorded() {
    let src = ":deep(.d) { color: red; }\n:slotted(.s) { color: blue; }";
    let analysis = build_css_style_analysis(src, VueStyleInput::default(), true, false, None, 0);
    let kinds: Vec<SpecialPseudoKind> = analysis.special_pseudos.iter().map(|p| p.kind).collect();
    assert!(kinds.contains(&SpecialPseudoKind::Deep));
    assert!(kinds.contains(&SpecialPseudoKind::Slotted));
    let flags = analysis.analysis_flags();
    assert!(flags.contains(StyleAnalysisFlags::HAS_DEEP));
    assert!(flags.contains(StyleAnalysisFlags::HAS_SLOTTED));
}

// @ai-generated - Exact r2 pair: interpolation taints only its component, not the pseudo kind or literal siblings.
#[test]
fn interpolated_special_pseudos_preserve_kind_flags_and_disjoint_classes() {
    for (name, kind, flag) in [
        (
            "global",
            SpecialPseudoKind::Global,
            StyleAnalysisFlags::HAS_GLOBAL,
        ),
        (
            "deep",
            SpecialPseudoKind::Deep,
            StyleAnalysisFlags::HAS_DEEP,
        ),
    ] {
        for (argument, expected_classes) in [(".a .b", vec!["a", "b"]), (".a .#{$x}", vec!["a"])] {
            let source = format!(":{name}({argument}) {{ color: red; }}");
            let analysis = build_scanned_style_analysis(
                StyleAnalysisLang::Scss,
                &source,
                VueStyleInput::default(),
                true,
                false,
                None,
                0,
            );
            let css = analysis.css.as_ref().expect("SCSS analysis");
            let classes: Vec<_> = css
                .classes
                .iter()
                .map(|class| class.name.as_str())
                .collect();
            assert_eq!(classes, expected_classes, "{source}");
            assert_eq!(
                analysis
                    .special_pseudos
                    .iter()
                    .map(|pseudo| pseudo.kind)
                    .collect::<Vec<_>>(),
                vec![kind],
                "{source}"
            );
            assert!(analysis.analysis_flags().contains(flag), "{source}");
        }
    }
}

// @ai-generated - Exact r2 pair: an ambiguous Stylus child cannot recover its intact rule owner.
#[test]
fn stylus_colonless_declaration_preserves_rule_body_span() {
    for source in [".a\n  color: red", ".a\n  color red"] {
        let analysis = build_scanned_style_analysis(
            StyleAnalysisLang::Stylus,
            source,
            VueStyleInput::default(),
            true,
            false,
            None,
            0,
        );
        let css = analysis.css.as_ref().expect("Stylus analysis");
        let selector = css.selectors.first().expect(".a selector");
        assert_eq!(selector.text, ".a", "{source}");
        assert_eq!(
            selector.rule_body_span,
            Some(Span::new(2, source.len() as u32)),
            "{source}"
        );
    }
}

#[test]
fn plain_selector_records_no_special_pseudos() {
    let analysis = build_css_style_analysis(
        ".plain { color: red; }",
        VueStyleInput::default(),
        true,
        false,
        None,
        0,
    );
    assert!(analysis.special_pseudos.is_empty());
    assert!(!analysis
        .analysis_flags()
        .contains(StyleAnalysisFlags::HAS_GLOBAL));
}

#[test]
fn analyzed_selector_serde_roundtrips_body_span_and_class_join() {
    let analysis = analyze_css(".btn { color: red; }");
    let css = analysis.css.as_ref().unwrap();
    let json = serde_json::to_string(css).unwrap();
    let back: CssAnalysis = serde_json::from_str(&json).unwrap();
    assert_eq!(
        back.selectors[0].rule_body_span,
        css.selectors[0].rule_body_span
    );
    assert_eq!(
        back.classes[0].selector_index,
        css.classes[0].selector_index
    );
}

#[test]
fn sass_indented_uses_shared_syntax_projection() {
    let analysis = build_scanned_style_analysis(
        StyleAnalysisLang::Sass,
        ".a\n  color: red",
        VueStyleInput::default(),
        true,
        false,
        None,
        0,
    );
    let css = analysis.css.expect("indented Sass is structurally parsed");
    assert_eq!(css.classes.len(), 1);
    assert_eq!(css.classes[0].name, "a");
    assert_eq!(css.classes[0].span, Span::new(1, 2));
}

// @ai-generated - Proves every authored dialect uses one authority and damaged nodes stay local.
#[test]
fn all_five_dialects_extract_disjoint_complete_classes_after_damage() {
    for (lang, input, expected) in [
        (
            StyleAnalysisLang::Css,
            ".bad { color: \"oops\n}\n.css { color: red; }",
            "css",
        ),
        (
            StyleAnalysisLang::Scss,
            ".bad { color: \"oops\n}\n.scss { color: red; }",
            "scss",
        ),
        (
            StyleAnalysisLang::Less,
            ".bad { color: \"oops\n}\n.less { color: red; }",
            "less",
        ),
        (
            StyleAnalysisLang::Sass,
            ".bad\n  color: #{$x\n.sass\n  color: red\n",
            "sass",
        ),
        (
            StyleAnalysisLang::Stylus,
            ".bad\n  color ${x\n.stylus\n  color red\n",
            "stylus",
        ),
    ] {
        let analysis = build_scanned_style_analysis(
            lang,
            input,
            VueStyleInput::default(),
            true,
            false,
            None,
            0,
        );
        let css = analysis.css.expect("dialect syntax projection");
        assert!(
            css.classes.iter().any(|class| class.name == expected),
            "{lang:?}: {:?}",
            css.classes
                .iter()
                .map(|class| &class.name)
                .collect::<Vec<_>>()
        );
    }
}

// =============================================================================
// `CssAnalysis.declarations` — per-declaration record (name_span, value_span,
// selector_index, color_candidates), populated for every complete declaration.
// =============================================================================

/// A plain (non-`--`) declaration's value is retained in `declarations` — the
/// gap this record closes. Before this record existed, a plain declaration's
/// value text was not retained anywhere in `CssAnalysis`.
#[test]
fn test_declarations_populated_for_plain_property() {
    let css = ".box { color: red; }";
    let analysis = analyze_css(css);
    let css_analysis = analysis.css.as_ref().unwrap();

    assert_eq!(
        css_analysis.declarations.len(),
        1,
        "a plain declaration must be recorded in `declarations`: {:?}",
        css_analysis.declarations
    );
    let decl = &css_analysis.declarations[0];
    assert_eq!(
        &css[decl.name_span.start as usize..decl.name_span.end as usize],
        "color"
    );
    assert_eq!(
        &css[decl.value_span.start as usize..decl.value_span.end as usize],
        "red"
    );
    assert!(decl.selector_index.is_some());
    assert!(decl.color_candidates.is_empty());
}

/// Custom-property declarations are ALSO retained in `declarations` (both
/// branches populate the shared record now — not just the `--`-prefixed one).
#[test]
fn test_declarations_populated_for_custom_property() {
    let css = ":root { --color: blue; }";
    let analysis = analyze_css(css);
    let css_analysis = analysis.css.as_ref().unwrap();

    assert_eq!(css_analysis.declarations.len(), 1);
    let decl = &css_analysis.declarations[0];
    assert_eq!(
        &css[decl.name_span.start as usize..decl.name_span.end as usize],
        "--color"
    );
    assert_eq!(
        &css[decl.value_span.start as usize..decl.value_span.end as usize],
        "blue"
    );
}

/// A hex literal in a declaration value is classified as a color candidate.
#[test]
fn test_color_candidate_hex_literal() {
    let css = ".box { color: #ff0000; }";
    let analysis = analyze_css(css);
    let css_analysis = analysis.css.as_ref().unwrap();

    let decl = &css_analysis.declarations[0];
    assert_eq!(decl.color_candidates.len(), 1);
    let candidate = &decl.color_candidates[0];
    assert_eq!(candidate.kind, ColorCandidateKind::Hex);
    assert_eq!(
        &css[candidate.span.start as usize..candidate.span.end as usize],
        "#ff0000"
    );
    assert!(candidate.numeric_args.is_empty());
}

/// The verified discriminator: a comment INSIDE a color function's argument
/// list. The current `color_info.rs` scanner re-slices the raw byte span and
/// `.split(',')`/`.parse()`s it, so the comment breaks numeric parsing and
/// silently produces zero usable arguments. The typed component-value walk
/// skips `ComponentValue::Comment` entries structurally and extracts the
/// numeric arguments correctly.
#[test]
fn test_color_candidate_rgb_function_numeric_args_skip_comment() {
    let css = ".box { color: rgb(255, /* not blue */ 0, 0); }";
    let analysis = analyze_css(css);
    let css_analysis = analysis.css.as_ref().unwrap();

    let decl = &css_analysis.declarations[0];
    assert_eq!(decl.color_candidates.len(), 1);
    let candidate = &decl.color_candidates[0];
    assert_eq!(candidate.kind, ColorCandidateKind::Function);
    assert_eq!(candidate.function_name.as_deref(), Some("rgb"));
    assert_eq!(
        candidate.numeric_args,
        vec![
            NumericArg::Number(255.0),
            NumericArg::Number(0.0),
            NumericArg::Number(0.0)
        ]
    );
}

/// `rgba`/`hsl`/`hsla` all match case-insensitively.
#[test]
fn test_color_candidate_function_names_case_insensitive_and_hsla() {
    let css = ".box { color: HSLA(120, 50%, 50%, 0.5); }";
    let analysis = analyze_css(css);
    let css_analysis = analysis.css.as_ref().unwrap();

    let decl = &css_analysis.declarations[0];
    assert_eq!(decl.color_candidates.len(), 1);
    let candidate = &decl.color_candidates[0];
    assert_eq!(candidate.function_name.as_deref(), Some("hsla"));
    assert_eq!(
        candidate.numeric_args,
        vec![
            NumericArg::Number(120.0),
            NumericArg::Percentage(50.0),
            NumericArg::Percentage(50.0),
            NumericArg::Number(0.5)
        ]
    );
}

/// A22: CSS relative-color syntax (`rgb(from red 255 0 0)`) is out of scope. The `from`/`red`
/// identifiers must invalidate the WHOLE candidate's `numeric_args`, not just be skipped while
/// the surrounding `255 0 0` numbers survive — a partial list would let `color_info.rs`
/// fabricate a color for a shape this producer does not support.
#[test]
fn test_color_candidate_relative_color_syntax_invalidates_numeric_args() {
    let css = ".box { color: rgb(from red 255 0 0); }";
    let analysis = analyze_css(css);
    let css_analysis = analysis.css.as_ref().unwrap();

    let decl = &css_analysis.declarations[0];
    assert_eq!(decl.color_candidates.len(), 1);
    let candidate = &decl.color_candidates[0];
    assert_eq!(candidate.function_name.as_deref(), Some("rgb"));
    assert!(
        candidate.numeric_args.is_empty(),
        "relative-color syntax must invalidate the whole candidate, not leak a partial list"
    );
}

/// A22: a nested math function (`calc()`) inside a color function's argument list is out of
/// scope and must likewise invalidate the whole candidate.
#[test]
fn test_color_candidate_nested_calc_invalidates_numeric_args() {
    let css = ".box { color: rgb(calc(255), 0, 0); }";
    let analysis = analyze_css(css);
    let css_analysis = analysis.css.as_ref().unwrap();

    let decl = &css_analysis.declarations[0];
    assert_eq!(decl.color_candidates.len(), 1);
    let candidate = &decl.color_candidates[0];
    assert!(
        candidate.numeric_args.is_empty(),
        "a nested calc() argument must invalidate the whole candidate"
    );
}

/// Comments and strings never contribute a spurious color candidate: a
/// string literal containing `#`-shaped text, and a comment containing
/// `rgb(...)`-shaped text, are structurally excluded because the walk never
/// visits `ComponentValue::Comment`/`ComponentValue::String` variants at all
/// (never a byte mask over the raw text).
#[test]
fn test_color_candidates_exclude_comment_and_string_content() {
    let css = ".box { content: \"#fake rgb(1,2,3)\"; }";
    let analysis = analyze_css(css);
    let css_analysis = analysis.css.as_ref().unwrap();
    let content_decl = css_analysis
        .declarations
        .iter()
        .find(|decl| &css[decl.name_span.start as usize..decl.name_span.end as usize] == "content")
        .expect("content declaration recorded");
    assert!(
        content_decl.color_candidates.is_empty(),
        "a quoted string must never contribute a color candidate: {:?}",
        content_decl.color_candidates
    );

    let css_with_comment = ".box { color: red /* rgb(1,2,3) #fake */; }";
    let analysis = analyze_css(css_with_comment);
    let css_analysis = analysis.css.as_ref().unwrap();
    let color_decl = &css_analysis.declarations[0];
    assert_eq!(
        color_decl.color_candidates.len(),
        0,
        "a comment must never contribute a color candidate: {:?}",
        color_decl.color_candidates
    );
}

// ── the per-declaration record's SCHEMA ──
//
// `AnalyzedDeclaration` is the contract two LSP readers (the color-picker's
// candidate source and the completion classifier's value-position signal)
// were converged onto. Behavioral fixtures over those readers observe the
// values that flow through today; they do not observe the record's REQUIRED
// SHAPE, so a field could be renamed, dropped, or silently repurposed while
// every colour chip and completion stayed green. These pins close that: the
// exhaustive destructurings below are compile-time closure over the field
// set (adding a field is `E0027`, removing or renaming one is `E0026`/
// `E0609`), and the value assertions pin what each field must mean.

/// Every field of `AnalyzedDeclaration` is required, and each carries the
/// meaning its consumers depend on: `name_span` delimits the property name
/// exactly, `value_span` the trimmed value text, `selector_index` the
/// ENCLOSING rule's entry in `selectors`.
#[test]
fn analyzed_declaration_schema_is_name_span_value_span_selector_index() {
    let css = ".a { color:   #f00  ; }\n.b { --tone: blue; }";
    let analysis = analyze_css(css);
    let css_analysis = analysis.css.as_ref().expect("css analysis");
    assert_eq!(
        css_analysis.declarations.len(),
        2,
        "every Complete declaration is recorded, custom-property or not: {:?}",
        css_analysis.declarations
    );

    for (index, expected_name, expected_value, expected_selector) in
        [(0usize, "color", "#f00", ".a"), (1, "--tone", "blue", ".b")]
    {
        // Exhaustive destructuring: the required field set, closed by the
        // compiler. A new field forces this pin to be revisited.
        let AnalyzedDeclaration {
            name_span,
            value_span,
            selector_index,
            color_candidates: _,
        } = &css_analysis.declarations[index];

        assert_eq!(
            &css[name_span.start as usize..name_span.end as usize],
            expected_name,
            "name_span must delimit the property name exactly"
        );
        assert_eq!(
            &css[value_span.start as usize..value_span.end as usize],
            expected_value,
            "value_span must delimit the TRIMMED value text — the completion \
             classifier treats `value_span.end` as the last offset still inside \
             the value"
        );
        let selector_index = selector_index.expect("a rule-body declaration has an enclosing rule");
        assert_eq!(
            css_analysis.selectors[selector_index as usize].text, expected_selector,
            "selector_index must index back into `selectors` at the ENCLOSING rule"
        );
        assert!(
            css_analysis.selectors[selector_index as usize]
                .rule_body_span
                .is_some_and(|body| name_span.start >= body.start && value_span.end <= body.end),
            "the declaration must lie inside its own selector's rule_body_span — \
             the pair the completion classifier joins on"
        );
    }
}

/// Every field of `AnalyzedColorCandidate` is required, and color-function
/// classification is CASE-INSENSITIVE with a lowercase-normalised
/// `function_name` — the form `color_info.rs` matches on (`starts_with("rgb")`)
/// and could not itself detect regressing to case-sensitive.
#[test]
fn analyzed_color_candidate_schema_and_case_insensitive_function_names() {
    for (value, expected_name) in [
        ("RGB(255, 0, 0)", "rgb"),
        ("Rgb(255, 0, 0)", "rgb"),
        ("rGbA(255, 0, 0, 1)", "rgba"),
        ("HSL(0, 100%, 50%)", "hsl"),
        ("HsLa(0, 100%, 50%, 1)", "hsla"),
    ] {
        let css = format!(".a {{ color: {value}; }}");
        let analysis = analyze_css(&css);
        let css_analysis = analysis.css.as_ref().expect("css analysis");
        let candidates = &css_analysis.declarations[0].color_candidates;
        assert_eq!(
            candidates.len(),
            1,
            "`{value}` must classify as one color-function candidate whatever \
             its casing: {candidates:?}"
        );

        // Exhaustive destructuring: the required field set, closed by the compiler.
        let AnalyzedColorCandidate {
            span,
            kind,
            function_name,
            numeric_args,
        } = &candidates[0];

        assert_eq!(*kind, ColorCandidateKind::Function);
        assert_eq!(
            &css[span.start as usize..span.end as usize],
            value,
            "span must cover the whole call including its parentheses"
        );
        assert_eq!(
            function_name.as_deref(),
            Some(expected_name),
            "function_name must be lowercase-normalised, whatever the source casing"
        );
        assert!(
            numeric_args.len() >= 3,
            "the channel arguments must be extracted: {numeric_args:?}"
        );
    }

    // The hex arm's own field shape, and the closed kind taxonomy.
    let css = ".a { color: #F00; }";
    let analysis = analyze_css(css);
    let css_analysis = analysis.css.as_ref().expect("css analysis");
    let AnalyzedColorCandidate {
        span,
        kind,
        function_name,
        numeric_args,
    } = &css_analysis.declarations[0].color_candidates[0];
    match kind {
        ColorCandidateKind::Hex => {
            assert_eq!(&css[span.start as usize..span.end as usize], "#F00");
            assert!(function_name.is_none(), "a hex token names no function");
            assert!(numeric_args.is_empty(), "a hex token carries no arguments");
        }
        ColorCandidateKind::Function => panic!("`#F00` is a hex candidate, not a function"),
    }

    // Negative: a case-variant of a NON-color function is still not a color.
    let css = ".a { color: URL(x.png); }";
    let analysis = analyze_css(css);
    let css_analysis = analysis.css.as_ref().expect("css analysis");
    assert!(
        css_analysis.declarations[0].color_candidates.is_empty(),
        "case-insensitivity must not widen the closed color-function set: {:?}",
        css_analysis.declarations[0].color_candidates
    );
}
