//! Unknown/non-native style dialects must not be parsed as CSS or yield
//! fabricated v-bind facts.

use std::sync::Arc;

use verter_semantic::analysis::{BlockContentAvailability, StyleAnalysisLang};
use verter_session::{FileLanguage, HostConfig, UpsertRequest, VerterHost};

fn analyze(source: &str) -> verter_session::FileAnalysisSnapshot {
    let host = VerterHost::new_standalone(HostConfig::default());
    let update = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/workspace/Dialect.vue".to_string(),
            source: Arc::from(source),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .expect("upsert");
    host.get_analysis(&update.canonical_id).expect("analysis")
}

#[test]
fn unknown_lang_does_not_fabricate_v_bind_facts_from_css_parse() {
    let analysis = analyze(
        "<template><div/></template>\n\
         <style lang=\"sugarss\">.card { color: v-bind(theme); }</style>",
    );
    assert_eq!(analysis.styles.len(), 1);
    let style = &analysis.styles[0];
    assert_eq!(style.lang, StyleAnalysisLang::Unknown);
    assert_eq!(
        style.content_availability,
        BlockContentAvailability::ProcessedContentRequired
    );
    assert!(
        style.css.is_none(),
        "unknown dialect must not parse as CSS: {:?}",
        style.css
    );
    assert!(
        style
            .v_binds
            .iter()
            .all(|vb| vb.expression != "theme" && vb.generated_var_name.is_none()),
        "must not fabricate a CSS-parsed v-bind fact: {:?}",
        style.v_binds
    );
    assert!(
        style.v_binds.iter().any(|vb| !vb.roots_complete),
        "unknown dialect with possible v-bind must stay incomplete: {:?}",
        style.v_binds
    );
}

#[test]
fn postcss_lang_is_not_admitted_as_native_css() {
    let analysis = analyze(
        "<template><div/></template>\n\
         <style lang=\"postcss\">.card { color: v-bind(accent); }</style>",
    );
    assert_eq!(analysis.styles.len(), 1);
    let style = &analysis.styles[0];
    assert_eq!(style.lang, StyleAnalysisLang::Unknown);
    assert!(
        style.css.is_none(),
        "postcss must not parse as CSS: {:?}",
        style.css
    );
    assert!(
        style.v_binds.iter().all(|vb| vb.expression != "accent"),
        "postcss must not be parsed as CSS v-bind: {:?}",
        style.v_binds
    );
    assert!(
        style.v_binds.iter().any(|vb| !vb.roots_complete),
        "postcss with possible v-bind must stay incomplete: {:?}",
        style.v_binds
    );
}

#[test]
fn native_css_still_extracts_trusted_v_bind_facts() {
    let analysis = analyze(
        "<template><div/></template>\n\
         <style>.card { color: v-bind(theme); }</style>",
    );
    let style = &analysis.styles[0];
    assert_eq!(style.lang, StyleAnalysisLang::Css);
    assert!(
        style.css.is_some(),
        "native CSS must project selector facts"
    );
    assert!(
        style
            .v_binds
            .iter()
            .any(|vb| vb.expression == "theme" && vb.roots_complete),
        "native CSS must extract the v-bind: {:?}",
        style.v_binds
    );
}
