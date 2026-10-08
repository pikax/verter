//! The flow-transparent callback check against BOTH real providers.
//!
//! Every fixture is an authored template; `verter_compiler::flow_check` plans
//! it from the real parse, generates the check through `CodeTransform`, and
//! the generated TypeScript is published into the `flow-check` fixture
//! project and checked by the provider. Each provider diagnostic is mapped
//! back to authored bytes through the generator's mappings and must equal the
//! fixture's exact `(code, authored span)` set — valid fixtures report
//! nothing, invalid twins report exactly their planted errors, and dropping an
//! essential condition reports exactly the reads it guarded.
//!
//! A trailing canary error proves the provider checked the published file
//! before an empty result is accepted.

use verter_compiler::flow_check::fixtures::{self, Fixture, Root, MATRIX_SIZES};
use verter_compiler::flow_check::generator::GuardStrategy;
use verter_compiler::flow_check::oracle::{check_exact, ProviderDiagnostic};
use verter_compiler::flow_check::seam::{GeneratedCheck, Mapping, Span};
use verter_type_runtime::protocol::TypeDiagnosticSeverity;

use crate::test_harness::{real_provider_test, RealProviderTestSession};

const FIXTURE: &str = "flow-check";
const CANARY: &str = "\nexport const __verter_flow_canary: number = \"checked\";\n";

/// Publish `code` as a configured-project member, open it in the provider
/// and return its error diagnostics once the canary proves it was checked.
async fn provider_errors(
    session: &RealProviderTestSession,
    name: &str,
    code: &str,
) -> Vec<ProviderDiagnostic> {
    let relative = format!("src/{name}.ts");
    let content = format!("{code}{CANARY}");
    let canary_start = code.len() as u32;
    let _published = session.publish_workspace_file(&relative, &content);
    let (path, _) = session.open_fixture_in_provider(&relative).await;
    for attempt in 0..120 {
        if let Ok(diagnostics) = session.provider().get_diagnostics(&path).await {
            let errors: Vec<ProviderDiagnostic> = diagnostics
                .into_iter()
                .filter(|d| matches!(d.severity, TypeDiagnosticSeverity::Error))
                .map(|d| ProviderDiagnostic {
                    code: d.code.as_deref().and_then(|c| c.parse().ok()).unwrap_or(0),
                    start: d.start,
                    end: d.end,
                    message: d.message,
                })
                .collect();
            if errors
                .iter()
                .any(|d| d.start >= canary_start && d.code == 2322)
            {
                session.provider().close_file(&path).await.ok();
                return errors
                    .into_iter()
                    .filter(|d| d.start < canary_start)
                    .collect();
            }
        }
        if attempt < 119 {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    }
    panic!("{name}: the provider never reported the canary, so the check was not executed");
}

async fn assert_fixture(
    session: &RealProviderTestSession,
    fixture: &Fixture,
    strategy: GuardStrategy,
) -> GeneratedCheck {
    let check = fixture.generate_with(strategy);
    let name = match strategy {
        GuardStrategy::Snapshot => fixture.name.clone(),
        GuardStrategy::ReplayPath => format!("{}-replay", fixture.name),
    };
    let errors = provider_errors(session, &name, &check.code).await;
    if let Err(mismatch) = check_exact(fixture, &check, &errors) {
        panic!("[{}] {mismatch}", session.provider_kind().label());
    }
    check
}

fn hover_offset(check: &GeneratedCheck, authored: Span) -> u32 {
    *check
        .generated_offsets(authored.start)
        .first()
        .expect("a hovered authored identifier is emitted")
}

real_provider_test!(
    flow_check_semantics_are_exact,
    fixture = FIXTURE,
    async fn run(session) {
        for fixture in fixtures::semantic_suite() {
            let check = assert_fixture(session, &fixture, GuardStrategy::Snapshot).await;
            for hover in &fixture.hovers {
                let relative = format!("src/{}-hover.ts", fixture.name);
                let _published = session.publish_workspace_file(&relative, &check.code);
                let (path, _) = session.open_fixture_in_provider(&relative).await;
                let mut text = String::new();
                for _ in 0..40 {
                    if let Ok(Some(info)) = session
                        .provider()
                        .get_hover(&path, hover_offset(&check, hover.authored))
                        .await
                    {
                        text = info.contents;
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                }
                assert!(
                    text.contains(hover.contains),
                    "[{}] {}: the contextually typed parameter `{}` must hover as `{}`; got {text:?}",
                    session.provider_kind().label(),
                    fixture.name,
                    &fixture.source[hover.authored.start as usize..hover.authored.end as usize],
                    hover.contains,
                );
                session.provider().close_file(&path).await.ok();
            }
        }
    }
);

real_provider_test!(
    flow_check_replays_the_existing_narrowing_for_property_roots,
    fixture = FIXTURE,
    async fn run(session) {
        // Replaying the whole condition path inside each callback is the
        // existing emitter's semantics. For property-rooted references it is
        // sound, so the snapshot representation must report exactly what it
        // reports — including the mutation and nested-closure cases, where both
        // lose the narrowing.
        for fixture in [
            fixtures::three_branch_discriminator(Root::MutableProperty, true),
            fixtures::flat_chain(Root::MutableProperty, 40, true),
            fixtures::deep_positives(40, true),
            fixtures::mutation_and_nested_closure(),
            fixtures::mapping(),
        ] {
            assert_fixture(session, &fixture, GuardStrategy::ReplayPath).await;
            assert_fixture(session, &fixture, GuardStrategy::Snapshot).await;
        }
    }
);

real_provider_test!(
    flow_check_linear_matrix_checks_every_callback,
    fixture = FIXTURE,
    async fn run(session) {
        for &n in &MATRIX_SIZES {
            for fixture in [
                fixtures::flat_matrix(n, 1, false),
                fixtures::flat_matrix(n, 1, true),
                fixtures::nested_matrix(n, false),
                fixtures::nested_matrix(n, true),
            ] {
                let check = assert_fixture(session, &fixture, GuardStrategy::Snapshot).await;
                assert_eq!(
                    check.layout.callbacks.len(),
                    fixture.authored_callbacks,
                    "{}: every authored callback is emitted",
                    fixture.name
                );
                if fixture.name.ends_with("-broken") {
                    // One planted error per callback, each inside its own
                    // callback: every callback body was type-checked.
                    assert_eq!(fixture.expected.len(), fixture.authored_callbacks);
                    for site in &check.layout.callbacks {
                        let inside = fixture
                            .expected
                            .iter()
                            .filter(|e| site.authored.contains(e.authored))
                            .count();
                        assert_eq!(inside, 1, "{}: callback {site:?}", fixture.name);
                    }
                }
            }
        }
    }
);

real_provider_test!(
    flow_check_maps_provider_ranges_exactly,
    fixture = FIXTURE,
    async fn run(session) {
        // Non-ASCII text before the reported identifiers and a condition moved
        // ahead of its element's `v-for`: the provider's ranges must land on
        // the exact authored bytes.
        let fixture = fixtures::mapping();
        let check = fixture.generate();
        let errors = provider_errors(session, "mapping-provider", &check.code).await;
        check_exact(&fixture, &check, &errors).unwrap_or_else(|mismatch| {
            panic!("[{}] {mismatch}", session.provider_kind().label())
        });
        // A mapping shifted by one authored byte must fail the same oracle on
        // the same provider result.
        let mut perturbed = check.clone();
        let target = perturbed
            .mappings
            .iter()
            .position(|m| fixture.expected.iter().any(|e| m.source.contains(e.authored)))
            .expect("an expected diagnostic lies inside a mapping");
        let Mapping { generated, source } = perturbed.mappings[target];
        perturbed.mappings[target] = Mapping {
            generated,
            source: Span::new(source.start + 1, source.end + 1),
        };
        assert!(
            check_exact(&fixture, &perturbed, &errors).is_err(),
            "[{}] a perturbed mapping must fail the exact-range oracle",
            session.provider_kind().label()
        );
    }
);
