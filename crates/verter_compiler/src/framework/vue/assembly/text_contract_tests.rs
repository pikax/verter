use super::*;
use crate::compile_request::VueBackendRequest;

fn text_modules(source: &str) -> Vec<(bool, String)> {
    [
        (ProductKind::RuntimeClient, VueBackendRequest::Inferred),
        (ProductKind::RuntimeClient, VueBackendRequest::Vapor),
        (ProductKind::RuntimeServer, VueBackendRequest::Inferred),
    ]
    .into_iter()
    .map(|(product, backend)| {
        let request = CompileRequest::new(
            vec![runtime_product(product, true)],
            FrameworkCompileRequest::Vue(VueCompileRequest {
                backend,
                ..Default::default()
            }),
            None,
            Some("App.vue".into()),
            None,
            false,
            true,
        )
        .unwrap();
        let staged = compile_with(
            source,
            &request,
            &Default::default(),
            &mut CompilerWorkLedger::<Uncaptured>::new(),
        )
        .unwrap()
        .stage(product)
        .unwrap();
        let code = staged.code().to_string();
        let allocator = Allocator::default();
        let parsed =
            verter_parser::oxc_parse::Parser::new(&allocator, &code, oxc_span::SourceType::mjs())
                .parse();
        assert!(
            parsed.diagnostics.is_empty(),
            "{:?}\n{code}",
            parsed.diagnostics
        );
        (product == ProductKind::RuntimeServer, code)
    })
    .collect()
}

#[test]
fn nonbreaking_space_entities_keep_their_text_value() {
    for source in [
        "<template><p>&nbsp;</p></template>",
        "<template><p>&nbsp;· {{ a }}</p></template>",
    ] {
        for (_, code) in text_modules(source) {
            assert!(code.contains('\u{a0}') || code.contains("&nbsp;"), "{code}");
            assert!(!code.contains("\"&nbsp;\""), "{code}");
        }
    }
}

#[test]
fn preformatted_line_spans_keep_their_ending_newline() {
    for ending in ["\n", "\n  ", "\r\n"] {
        let source = format!("<template><pre><span>{{{{ a }}}}{ending}</span></pre></template>");
        let expected = ending.replace("\r\n", "\n");
        for (server, code) in text_modules(&source) {
            if server {
                assert!(code.contains(&format!("}}{expected}</span>")), "{code}");
            } else {
                let escaped = expected.replace('\n', "\\n");
                assert!(code.contains(&format!(" + \"{escaped}\"")), "{code}");
            }
        }
    }
    for (_, code) in text_modules("<template><div v-pre>a  b</div></template>") {
        assert!(code.contains("a b"), "{code}");
        assert!(!code.contains("a  b"), "{code}");
    }
}

#[test]
fn scoped_nested_slot_outlets_combine_own_and_inherited_scope() {
    let code = compile_with(
        "<template><Box><slot /></Box></template><style scoped>p{color:red}</style>",
        &server_request(),
        &Default::default(),
        &mut CompilerWorkLedger::<Uncaptured>::new(),
    )
    .unwrap()
    .stage(ProductKind::RuntimeServer)
    .unwrap()
    .code()
    .to_string();
    assert!(code.contains("\"data-v-0d04bfeb-s\" + _scopeId"), "{code}");
}
