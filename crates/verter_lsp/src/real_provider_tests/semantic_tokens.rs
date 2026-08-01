//! LSP-boundary semantic-token kind assertions over REAL providers.
//!
//! The public contract under test: an identifier in a `.vue` / `.svelte`
//! carrier resolves to the SAME published token-type NAME the equivalent
//! `.ts` file gets under VS Code's TypeScript semantic highlighting —
//! interface / variable / function — delivered through the full pipeline
//! (provider classification → legend remap → TSX→carrier range mapping →
//! delta encoding against the advertised legend).
//!
//! Discrimination: the pre-fix tree fails these in TWO independent ways —
//! the managed tsserver lane sent `encodedSemanticClassifications-full` with
//! line/offset args (the protocol takes NUMERIC UTF-16 `start`/`length`, so
//! the engine answered `success: true` with zero spans → no token covers
//! anything), and the decode forwarded inverted/unremapped indices (so even
//! WITH spans the kind assert on `interface` fails).

use tower_lsp_server::ls_types::{
    InlayHintKind, InlayHintParams, Position, Range, SemanticTokensParams, SemanticTokensResult,
    TextDocumentIdentifier, Uri, WorkDoneProgressParams,
};
use tower_lsp_server::LanguageServer;

use crate::test_harness::{RealProviderTestSession, TestProviderKind, TestSessionBuilder};

const FIXTURE: &str = "external-ts-engine";

/// A decoded on-wire token: absolute position + legend NAMES.
#[derive(Debug)]
struct DecodedToken {
    line: u32,
    start_char: u32,
    length: u32,
    token_type: &'static str,
    modifiers: Vec<&'static str>,
}

/// Decode the delta-encoded wire stream against Verter's PUBLISHED legend
/// (the only legend the server ever advertises).
fn decode_wire_tokens(data: &[tower_lsp_server::ls_types::SemanticToken]) -> Vec<DecodedToken> {
    let types = verter_type_runtime::semantic_tokens::VERTER_TOKEN_TYPES;
    let modifiers = verter_type_runtime::semantic_tokens::VERTER_TOKEN_MODIFIERS;
    let mut decoded = Vec::with_capacity(data.len());
    let mut line = 0u32;
    let mut start_char = 0u32;
    for token in data {
        if token.delta_line > 0 {
            line += token.delta_line;
            start_char = token.delta_start;
        } else {
            start_char += token.delta_start;
        }
        let type_name = types
            .get(token.token_type as usize)
            .copied()
            .unwrap_or_else(|| {
                panic!(
                    "wire token type index {} is outside the advertised {}-entry legend — \
                     provider-space indices leaked to the wire: {token:?}",
                    token.token_type,
                    types.len()
                )
            });
        let modifier_names = modifiers
            .iter()
            .enumerate()
            .filter(|(bit, _)| token.token_modifiers_bitset & (1 << bit) != 0)
            .map(|(_, name)| *name)
            .collect();
        decoded.push(DecodedToken {
            line,
            start_char,
            length: token.length,
            token_type: type_name,
            modifiers: modifier_names,
        });
    }
    decoded
}

/// Request `textDocument/semanticTokens/full`, retrying while the provider
/// warms (tsserver loads projects asynchronously after open).
async fn semantic_tokens_with_retry(
    session: &RealProviderTestSession,
    uri: &Uri,
) -> Vec<DecodedToken> {
    let mut last: Vec<DecodedToken> = Vec::new();
    for delay_ms in [0u64, 500, 1000, 2000, 4000, 8000] {
        if delay_ms > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
        }
        let params = SemanticTokensParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: Default::default(),
        };
        let result = session.server().semantic_tokens_full(params).await;
        if let Ok(Some(SemanticTokensResult::Tokens(tokens))) = result {
            if !tokens.data.is_empty() {
                last = decode_wire_tokens(&tokens.data);
                break;
            }
        }
    }
    last
}

/// The decoded token covering `position`, if any.
fn token_at(tokens: &[DecodedToken], position: Position) -> Option<&DecodedToken> {
    tokens.iter().find(|t| {
        t.line == position.line
            && position.character >= t.start_char
            && position.character < t.start_char + t.length
    })
}

/// Shared body: open the carrier, pull tokens, assert the three kind anchors
/// (script region) plus a TEMPLATE/markup-region binding usage.
async fn assert_carrier_token_kinds(
    session: &RealProviderTestSession,
    relative: &str,
    call_needle: &str,
    template_needle: &str,
    expected_template_kind: &str,
) {
    let uri = session.open_fixture_file(relative).await;
    session.ensure_synced(&uri).await;
    let tokens = semantic_tokens_with_retry(session, &uri).await;
    assert!(
        !tokens.is_empty(),
        "{relative}: the carrier must produce semantic tokens through the full LSP \
         pipeline (provider classifications → legend remap → carrier mapping)"
    );

    // `AliasedShape` annotation → interface (the name a `.ts` file gets).
    let annotation = session.find_position(&uri, ": AliasedShape", 3);
    let interface_token = token_at(&tokens, annotation).unwrap_or_else(|| {
        panic!("{relative}: no token covers the `AliasedShape` annotation; got {tokens:?}")
    });
    assert_eq!(
        interface_token.token_type, "interface",
        "{relative}: `AliasedShape` must highlight as `interface`"
    );

    // `makeShape` call → function.
    let call = session.find_position(&uri, call_needle, 1);
    let function_token = token_at(&tokens, call).unwrap_or_else(|| {
        panic!("{relative}: no token covers the `makeShape` call; got {tokens:?}")
    });
    assert_eq!(
        function_token.token_type, "function",
        "{relative}: `makeShape` must highlight as `function`"
    );

    // `const shape` declaration → variable, carrying `declaration` +
    // `readonly` (const-ness survives the per-bit modifier remap; TS encodes
    // readonly on a DIFFERENT bit than the published legend).
    let declaration = session.find_position(&uri, "const shape:", 7);
    let variable_token = token_at(&tokens, declaration).unwrap_or_else(|| {
        panic!("{relative}: no token covers the `shape` declaration; got {tokens:?}")
    });
    assert_eq!(
        variable_token.token_type, "variable",
        "{relative}: `shape` must highlight as `variable`"
    );
    assert!(
        variable_token.modifiers.contains(&"declaration"),
        "{relative}: the `shape` declaration must carry the `declaration` modifier; \
         got {:?}",
        variable_token.modifiers
    );
    assert!(
        variable_token.modifiers.contains(&"readonly"),
        "{relative}: a `const` binding must carry the `readonly` modifier — losing it \
         means the modifier BITSET was forwarded instead of remapped per bit; got {:?}",
        variable_token.modifiers
    );

    // TEMPLATE/markup region: assert the name TypeScript assigns to the
    // equivalent expression in the generated TS surface. Vue preserves the
    // binding as a direct variable reference; Svelte lowers the template read
    // to a property access, so TypeScript correctly classifies the authored
    // identifier as `property` on that carrier.
    let template_use = session.find_position(&uri, template_needle, template_needle.len() - 2);
    let template_token = token_at(&tokens, template_use).unwrap_or_else(|| {
        panic!(
            "{relative}: no token covers the template-region `shape` usage \
             ({template_needle:?}); got {tokens:?}"
        )
    });
    assert_eq!(
        template_token.token_type, expected_template_kind,
        "{relative}: the template-region `shape` usage must highlight as \
         `{expected_template_kind}`"
    );

    // Inlay hints travel over the same carrier-serving provider lane. Ask for
    // the declaration line containing literal arguments and require a
    // parameter-name hint; the pre-fix tsgo initialize capabilities never
    // enabled the configuration channel, so the real engine returned none.
    let range_start = session.find_position(&uri, "const shape:", 0);
    let expected_parameter_position =
        session.find_position(&uri, call_needle, call_needle.len() - 1);
    let canonical_id = crate::documents::uri_to_canonical_id(&uri);
    let source = session
        .server()
        .test_documents()
        .host()
        .workspace_read()
        .read_file(&canonical_id)
        .expect("opened carrier fixture remains readable through the workspace");
    let line_length = source
        .lines()
        .nth(range_start.line as usize)
        .map(|line| line.encode_utf16().count() as u32)
        .expect("shape declaration line exists");
    let params = InlayHintParams {
        text_document: TextDocumentIdentifier { uri: uri.clone() },
        range: Range {
            start: range_start,
            end: Position::new(range_start.line, line_length),
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
    };
    let mut hints = Vec::new();
    for delay_ms in [0u64, 500, 1000, 2000, 4000] {
        if delay_ms > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
        }
        hints = session
            .server()
            .inlay_hint(params.clone())
            .await
            .unwrap_or_else(|error| panic!("{relative}: inlay-hint request failed: {error}"))
            .unwrap_or_default();
        if !hints.is_empty() {
            break;
        }
    }
    assert!(
        hints.iter().any(|hint| {
            hint.kind == Some(InlayHintKind::PARAMETER)
                && hint.position == expected_parameter_position
        }),
        "{relative}: expected a parameter-name inlay hint exactly on the first `makeShape` \
         argument at {expected_parameter_position:?}; got {hints:?}"
    );
}

async fn assert_carrier_lane(
    kind: TestProviderKind,
    relative: &str,
    call_needle: &str,
    template_needle: &str,
    expected_template_kind: &str,
) {
    let builder = TestSessionBuilder::new(kind)
        .fixture(FIXTURE)
        .tsgo_lsp_feature_only(matches!(kind, TestProviderKind::Tsgo));
    let session = builder.build().await.unwrap_or_else(|| {
        panic!(
            "{} is required for this real-provider semantic-token/inlay discriminator",
            kind.label()
        )
    });
    assert_carrier_token_kinds(
        &session,
        relative,
        call_needle,
        template_needle,
        expected_template_kind,
    )
    .await;
    session.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn vue_carrier_semantic_token_kinds_match_ts_baseline_tsserver() {
    assert_carrier_lane(
        TestProviderKind::Tsserver,
        "src/AliasConsumer.vue",
        "makeShape(1",
        // Delta 3 lands inside the `shape` identifier (skipping `{{ `).
        "{{ shape",
        "variable",
    )
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn svelte_carrier_semantic_token_kinds_match_ts_baseline_tsserver() {
    assert_carrier_lane(
        TestProviderKind::Tsserver,
        "src/AliasConsumer.svelte",
        "makeShape(2",
        "{shape.label}",
        "property",
    )
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn vue_carrier_semantic_token_kinds_match_ts_baseline_tsgo() {
    assert_carrier_lane(
        TestProviderKind::Tsgo,
        "src/AliasConsumer.vue",
        "makeShape(1",
        "{{ shape",
        "variable",
    )
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn svelte_carrier_semantic_token_kinds_match_ts_baseline_tsgo() {
    assert_carrier_lane(
        TestProviderKind::Tsgo,
        "src/AliasConsumer.svelte",
        "makeShape(2",
        "{shape.label}",
        "property",
    )
    .await;
}
