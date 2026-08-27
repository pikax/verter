//! Same query, same content, different lifecycle ⇒ same resolved answer.
//!
//! One fixed corpus of components and their cross-file dependencies is driven
//! through the three production lifecycles a resolver-tier query can take:
//! the host-bound request (`HostResolverContext`, reached through the bare
//! `VerterHost` query entry points), an overlay-less session
//! (`SessionResolverContext` with no overlay), and a session whose overlay
//! republishes byte-identical content for every corpus file. The published
//! component-meta output, its selective surface, and the expanded type
//! evaluation must be structurally identical across all three.
//!
//! Discriminates: the same-content overlay routes every dependency read
//! through the overlay-rooted view (overlay hash rooting, session-scoped
//! caches, the request-bound bundle memo), so a lifecycle that leaked a
//! base-only answer, dropped a dependency edge, or materialised a different
//! projection depth would produce a different Debug rendering here. A
//! content change through the overlay is asserted to CHANGE the answer, so
//! the equality is not the trivial one of an overlay that is never consulted.

use std::sync::Arc;

use crate::meta::{MetaProject, MetaSession};
use crate::types::{AnalysisLevel, HostConfig};
use crate::VerterHost;

const CORPUS: &[(&str, &str)] = &[
    (
        "/proj/src/shared.ts",
        r#"export interface Size { width: number; height: number }
export type Variant = 'solid' | 'outline'
export interface BaseProps<T> { value: T; size?: Size }
export type Handler<E extends string> = (event: E, payload: Size) => void
"#,
    ),
    (
        "/proj/src/index.ts",
        r#"export type { Size, Variant, BaseProps, Handler } from './shared'
export { default as Child } from './Child.vue'
"#,
    ),
    (
        "/proj/src/Child.vue",
        r#"<script setup lang="ts" generic="T extends string">
import type { BaseProps, Variant } from './shared'
const props = defineProps<BaseProps<T> & { variant?: Variant }>()
const emit = defineEmits<{ change: [next: T]; resize: [w: number, h: number] }>()
defineSlots<{ default(props: { value: T }): any; footer(): any }>()
defineExpose({ focus: () => {} })
</script>
<template><div :class="props.variant"><slot :value="props.value" /><slot name="footer" /></div></template>"#,
    ),
    (
        "/proj/src/App.vue",
        r#"<script setup lang="ts">
import type { Size, Handler } from './index'
import { Child } from './index'
import type { Variant } from './shared'
const props = withDefaults(defineProps<{ size: Size; variant?: Variant; onPick?: Handler<'pick'>; items: Pick<Size, 'width'>[] }>(), { variant: 'solid' })
const emit = defineEmits<{ pick: [which: Variant] }>()
</script>
<template><Child :value="props.variant" @change="(v) => emit('pick', v)"><template #footer>x</template></Child></template>"#,
    ),
];

const COMPONENTS: &[&str] = &["/proj/src/App.vue", "/proj/src/Child.vue"];

fn make_project() -> Arc<MetaProject> {
    let host = VerterHost::new_standalone(HostConfig {
        analysis_level: AnalysisLevel::Full,
        ..HostConfig::default()
    });
    let project = MetaProject::new(host);
    project
        .configure_projects(vec![verter_workspace::ide_project_config(
            "/proj".to_string(),
            "/proj".to_string(),
            Some("/proj/tsconfig.json".to_string()),
        )])
        .expect("configure");
    for (path, source) in CORPUS {
        project.upsert_base(path, source).expect("base upsert");
    }
    project
}

/// One structural rendering per component: the published output envelope,
/// the selective surface, and the expanded types, concatenated.
#[derive(Debug, PartialEq, Eq)]
struct Answer {
    component: &'static str,
    output: String,
    surface: String,
    evaluated: String,
}

/// The published analysis plus its materialized type lanes, minus the
/// registered carrier-structure projection. That projection carries the
/// SOURCE SNAPSHOT'S registration identity (`artifact_token`,
/// `file_incarnation`, `generation`, per-block tokens), which an overlay
/// legitimately re-mints for its own snapshot of the same bytes: it is
/// lifecycle identity, not a resolved answer, and is the ONE field the
/// same-content overlay is entitled to differ in.
fn render_semantic_output(
    mut analysis: verter_semantic::analysis::component_meta::ComponentMetaAnalysis,
    types: crate::meta_resolve::MaterializedComponentMetaTypes,
) -> String {
    assert!(
        analysis.ordered_sfc_structure.is_some(),
        "the corpus components carry a registered structure projection"
    );
    analysis.ordered_sfc_structure = None;
    format!("{analysis:?}{:?}", types.into_lanes())
}

/// The selective surface minus the same registration identity, which it
/// carries as the encoded `ordered_sfc_structure_bytes`.
fn render_surface(surface: Option<crate::component_meta_payload::ComponentMetaSurface>) -> String {
    let mut surface = surface.expect("the corpus components resolve to a surface");
    assert!(surface.ordered_sfc_structure_bytes.is_some());
    surface.ordered_sfc_structure_bytes = None;
    format!("{surface:?}")
}

fn host_answers(host: &VerterHost) -> Vec<Answer> {
    COMPONENTS
        .iter()
        .map(|component| {
            let output = host
                .get_component_meta_output(component)
                .expect("host output ok")
                .expect("host output resolves");
            let (analysis, _, types) = output.into_parts();
            Answer {
                component,
                output: render_semantic_output(analysis, types),
                surface: render_surface(host.get_component_meta_surface(component)),
                evaluated: format!("{:?}", host.evaluate_types(component)),
            }
        })
        .collect()
}

fn session_answers(session: &MetaSession) -> Vec<Answer> {
    COMPONENTS
        .iter()
        .map(|component| {
            let output = session
                .get_component_meta_output(component)
                .expect("session output ok")
                .expect("session output resolves");
            let (analysis, _, types) = output.into_parts();
            Answer {
                component,
                output: render_semantic_output(analysis, types),
                surface: render_surface(
                    session
                        .get_component_meta_surface(component)
                        .expect("session surface ok"),
                ),
                evaluated: format!(
                    "{:?}",
                    session
                        .evaluate_types(component)
                        .expect("session evaluate ok")
                ),
            }
        })
        .collect()
}

fn assert_answers_substantive(answers: &[Answer]) {
    assert_eq!(answers.len(), COMPONENTS.len());
    for answer in answers {
        assert!(
            answer.output.contains("variant"),
            "{}: the corpus prop `variant` must reach the published output",
            answer.component
        );
        assert!(
            answer.evaluated.contains("Size"),
            "{}: the cross-file `Size` reference must reach the evaluated types",
            answer.component
        );
        assert!(
            answer.surface.contains("variant"),
            "{}: the surface must carry the corpus prop",
            answer.component
        );
    }
}

#[test]
fn host_and_overlay_less_session_answer_identically() {
    let project = make_project();
    let host = host_answers(project.host());
    assert_answers_substantive(&host);

    let session = project.open_session().expect("session");
    let overlay_less = session_answers(&session);

    assert_eq!(
        host, overlay_less,
        "an overlay-less session must publish exactly the host-bound answer"
    );
}

#[test]
fn same_content_overlay_session_answers_identically_to_host() {
    let project = make_project();
    let host = host_answers(project.host());
    assert_answers_substantive(&host);

    let session = project.open_session().expect("session");
    // Byte-identical content, republished through the overlay: every read
    // now roots on the overlay's own hash and travels the session lifecycle.
    for (path, source) in CORPUS {
        session
            .upsert(path, (*source).to_string())
            .expect("overlay upsert");
    }
    let overlaid = session_answers(&session);

    assert_eq!(
        host, overlaid,
        "same query, same content, different lifecycle must yield the same answer"
    );
}

#[test]
fn a_changed_dependency_through_the_overlay_changes_the_answer() {
    // Negative control for the equalities above: the overlay IS consulted.
    // Renaming the cross-file `BaseProps` member changes Child.vue's
    // published prop inventory even though Child.vue itself is untouched.
    let project = make_project();
    let host = host_answers(project.host());

    let session = project.open_session().expect("session");
    session
        .upsert(
            "/proj/src/shared.ts",
            CORPUS[0].1.replace("value: T;", "val: T;"),
        )
        .expect("overlay upsert");
    let overlaid = session_answers(&session);

    let child = COMPONENTS
        .iter()
        .position(|c| *c == "/proj/src/Child.vue")
        .unwrap();
    assert_ne!(
        host[child].output, overlaid[child].output,
        "a dependency edit through the overlay must change the importer's published output"
    );
    assert!(
        overlaid[child].output.contains("\"val\"") && !host[child].output.contains("\"val\""),
        "the overlay's member name must be the one published"
    );
    // The host-bound lifecycle is unaffected by the session's overlay.
    assert_eq!(
        host_answers(project.host()),
        host,
        "a session overlay must never leak into the base host's answer"
    );
}
