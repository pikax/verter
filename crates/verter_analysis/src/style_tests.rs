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
    // The scanner is lenient — broken syntax still produces partial results
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

// ── strip_css_comments tests ──

#[test]
fn strip_css_comments_no_comments() {
    assert!(strip_css_comments(".a > .b").is_none());
}

#[test]
fn strip_css_comments_inline_comment() {
    let result = strip_css_comments(".a /* comment */ > .b").unwrap();
    assert_eq!(result, ".a   > .b");
}

#[test]
fn strip_css_comments_multiple() {
    let result = strip_css_comments("/* x */.a/* y */.b").unwrap();
    assert_eq!(result, " .a .b");
}

#[test]
fn selector_with_inline_comment_parsed() {
    let analysis = analyze_css(".a /* comment */ > .b { color: red; }");
    let css = analysis.css.as_ref().expect("should have CSS analysis");
    assert_eq!(css.selectors.len(), 1);
    // The selector text should have the comment stripped
    assert!(
        !css.selectors[0].text.contains("/*"),
        "selector text should not contain CSS comments"
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

    // Positive: should contain the parent and nested selectors
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
// CSS Variable Analysis Tests (Phase 1+2)
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
