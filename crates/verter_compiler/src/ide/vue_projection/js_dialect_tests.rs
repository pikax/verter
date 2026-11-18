use super::checking_fragment::{CheckingFragment, CheckingPiece};
use super::js_dialect::{javascript_declarations, javascript_statements};
use super::script_setup::SourceRange;

fn parses_as_javascript(text: &str) -> bool {
    let allocator = oxc_allocator::Allocator::default();
    let parsed = verter_parser::oxc_parse::Parser::new(
        &allocator,
        text,
        oxc_span::SourceType::jsx().with_module(true),
    )
    .parse();
    !parsed.fatal_error && parsed.diagnostics.is_empty()
}

#[test]
fn every_generated_declaration_block_has_a_javascript_form() {
    let blocks = [
        super::template_context::TEMPLATE_CONTEXT_PRELUDE.text,
        super::iteration::ITERATION_PRELUDE.text,
        super::event_payloads::EVENT_PAYLOAD_PRELUDE.text,
        super::models::MODEL_PRELUDE.text,
        super::provided_slots::PROVIDED_SLOT_PRELUDE.text,
        super::slot_outlets::SLOT_OUTLET_PRELUDE.text,
        super::native_elements::NATIVE_ELEMENT_PRELUDE.text,
        super::native_models::NATIVE_MODEL_PRELUDE.text,
        super::directives::DIRECTIVE_PRELUDE.text,
        super::fallthrough::FALLTHROUGH_PRELUDE.text,
        super::public_instance::TEMPLATE_REF_PRELUDE.text,
        super::attribute_operations::ATTRIBUTE_PAYLOAD_PRELUDE.text,
        super::component_use::USE_PRELUDE,
        crate::ide::script::VUE_GLOBAL_COMPONENTS_AUGMENTATION,
        crate::ide::script::GLOBAL_COMPONENT_CHECKING_HELPERS,
    ];
    for block in blocks {
        let javascript =
            javascript_declarations(block).unwrap_or_else(|refusal| panic!("{refusal:?}\n{block}"));
        assert!(parses_as_javascript(&javascript), "{javascript}");
        assert!(
            !javascript.contains("declare "),
            "no ambient declaration is left:\n{javascript}"
        );
    }
}

#[test]
fn typescript_declarations_take_their_jsdoc_form() {
    let javascript = javascript_declarations(concat!(
        "type Pick2<T extends object, K = keyof T> = { [P in K & keyof T]: T[P] };\n",
        "declare function pick<T>(value: readonly T[]): T;\n",
        "declare function pick(value: string): string;\n",
        "declare function named<const K extends PropertyKey>(key: K): K;\n",
        "declare const component: { new (): Instance };\n",
        "void (null! as Pick2<{ a: 1 }>);\n",
        "declare module \"vue\" {\n  interface GlobalComponents {}\n}\n",
        "export default component;\n",
    ))
    .expect("the declarations have a JavaScript form");
    for expected in [
        "/**\n * @template {object} T\n * @template [K=keyof T]\n * @typedef {{ [P in K & keyof T]: T[P] }} Pick2\n */",
        "/** @type {(<T>(value: readonly T[]) => T) & ((value: string) => string)} */\nvar pick = /** @type {any} */ (null);",
        "/** @type {(<const K extends PropertyKey>(key: K) => K)} */\nvar named = /** @type {any} */ (null);",
        "/** @type {{ new (): Instance }} */\nvar component = /** @type {any} */ (null);",
        "void (/** @type {Pick2<{ a: 1 }>} */ (/** @type {never} */ (/** @type {unknown} */ (null))));",
        "export default component;",
        "void [pick, named, component];",
    ] {
        assert!(javascript.contains(expected), "{expected}\nin:\n{javascript}");
    }
    assert!(
        !javascript.contains("GlobalComponents"),
        "an augmentation of empty interfaces merges nothing:\n{javascript}"
    );
}

#[test]
fn statements_keep_their_authored_pieces_and_cast_generated_types() {
    let authored = SourceRange { start: 40, end: 45 };
    let mut call = CheckingFragment::synth("const ctx = context({ a }, null! as Instance);\n");
    call.push_synth("(");
    call.pieces.push(CheckingPiece::Verbatim(authored));
    call.push_synth(" satisfies Target);\n");
    let mut typed = CheckingFragment::synth("const value: Value = ");
    typed
        .pieces
        .push(CheckingPiece::Verbatim(SourceRange { start: 50, end: 51 }));
    typed.push_synth(";\nhelper<Arg>(value as const);\n");
    let rendered = javascript_statements(&[call, typed]).expect("a JavaScript form");
    let flat = |fragment: &CheckingFragment| {
        fragment
            .pieces
            .iter()
            .map(|piece| match piece {
                CheckingPiece::Synth(text) => text.clone(),
                CheckingPiece::Verbatim(range) => format!("<{}>", range.start),
                other => panic!("unexpected piece {other:?}"),
            })
            .collect::<String>()
    };
    assert_eq!(
        flat(&rendered[0]),
        concat!(
            "const ctx = context({ a }, /** @type {Instance} */ (/** @type {never} */ (/** @type {unknown} */ (null))));\n",
            "(/** @satisfies {Target} */ (<40>));\n",
        )
    );
    assert_eq!(
        flat(&rendered[1]),
        concat!(
            "/** @type {Value} */ const value = <50>;\n",
            "/** @type {typeof helper<Arg>} */ (helper)(/** @type {const} */ (value));\n",
        )
    );
    assert!(
        rendered[0]
            .pieces
            .contains(&CheckingPiece::Verbatim(authored)),
        "the authored piece is kept as it is"
    );
}

#[test]
fn a_generated_type_that_holds_an_authored_piece_has_no_javascript_form() {
    let mut fragment = CheckingFragment::synth("const value = null! as typeof ");
    fragment
        .pieces
        .push(CheckingPiece::Verbatim(SourceRange { start: 3, end: 4 }));
    fragment.push_synth(";\n");
    assert!(javascript_statements(&[fragment]).is_err());
    // Syntax with no JavaScript form is refused rather than emitted.
    assert!(javascript_declarations("interface Shape { a: 1 }\n").is_err());
    assert!(javascript_declarations("enum Kind { A }\n").is_err());
}
