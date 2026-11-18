use super::*;

#[test]
fn numeric_newline_entities_between_interpolations_match_vue() {
    let case = Case {
        name: "numeric-newline.vue".into(),
        source: "<template><span>{{ a }}&#10;{{ b }}</span></template>".into(),
    };
    let authored = verter_vue_conformance::authored_identifiers(&case.source);
    for (target, golden) in [
        (
            Target::ClientTree,
            include_str!("goldens/numeric-newline-vdom.js"),
        ),
        (
            Target::Server,
            include_str!("goldens/numeric-newline-ssr.js"),
        ),
    ] {
        let RouteResult::Compiled(view) =
            run_route(RenderRoute::Targets, &case, target, Mode::Separate)
        else {
            panic!("text entity must compile");
        };
        let comparison = crate::common::compare_code(&view.render, golden, &authored);
        assert!(
            comparison.passed(),
            "{:?}\n{}",
            comparison.reasons,
            view.render
        );
    }
    let RouteResult::Compiled(view) = run_route(
        RenderRoute::Targets,
        &case,
        Target::FineGrained,
        Mode::Separate,
    ) else {
        panic!("text entity must compile");
    };
    assert!(view.render.contains(" + \" \" + "), "{}", view.render);
    assert!(!view.render.contains("&#10;"), "{}", view.render);
}
