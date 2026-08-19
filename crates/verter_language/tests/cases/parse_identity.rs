use verter_identity::encoding::CanonicalEncode;
use verter_identity::identity::{
    CompatibilityDomainId, CompatibilityEpoch, ContentId, SyntaxProfileId,
};
use verter_language::parse_identity::{
    parse_key_for, syntax_profile_id_for, ParseKeyDescriptor, ParseOptions, SyntaxProfileDescriptor,
};
use verter_language::{FileLanguage, LanguageId, ScriptSourceType};

const SYNTAX_PROFILE_GOLDEN_BYTES_HEX: &str = concat!(
    "210000007665727465722e6c616e67756167652e73796e7461785f70726f66696c652e7631",
    "06000000",
    "01000300000000000000767565",
    "02000300000000000000767565",
    "0300040000000000000001000000",
    "040002000000000000005b5b",
    "050002000000000000005d5d",
    "06001c00000000000000",
    "0200000000000000",
    "0200000000000000612d",
    "0200000000000000622d",
);
const SYNTAX_PROFILE_GOLDEN_DIGEST: &str =
    "45ee5ff2283da3cbd9f2a0e705d88542e3fa9a7d3c0ad83a12f671fecdc1381f";

const PARSE_KEY_GOLDEN_BYTES_HEX: &str = concat!(
    "1c0000007665727465722e6c616e67756167652e70617273655f6b65792e7631",
    "05000000",
    "010020000000000000006c28040c73e69a5ad8d37faa33e7808618d463eacfc0d2ac76ec6e96854aa63e",
    "02000300000000000000767565",
    "030017000000000000007665727465722e6672616d65776f726b2e73796e746178",
    "0400040000000000000007000000",
    "0500200000000000000045ee5ff2283da3cbd9f2a0e705d88542e3fa9a7d3c0ad83a12f671fecdc1381f",
);
const PARSE_KEY_GOLDEN_DIGEST: &str =
    "8937d3dd05d7e4ef6304631df8bd5165fe4abb572889421a9ff7a18f056b954d";

fn decode_hex(value: &str) -> Vec<u8> {
    assert_eq!(value.len() % 2, 0, "golden hex must contain byte pairs");
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let pair = std::str::from_utf8(pair).expect("golden hex is ASCII");
            u8::from_str_radix(pair, 16).expect("golden hex contains only hexadecimal digits")
        })
        .collect()
}

fn vue_options() -> ParseOptions {
    ParseOptions {
        delimiters: ("[[".to_string(), "]]".to_string()),
        custom_elements: vec!["b-".to_string(), "a-".to_string(), "a-".to_string()],
        svelte_loose: false,
    }
}

fn syntax_profile(options: &ParseOptions) -> SyntaxProfileId {
    syntax_profile_id_for(&FileLanguage::vue(), options).expect("Vue is a carrier frontend")
}

#[test]
fn syntax_profile_canonical_bytes_and_digest_are_pinned() {
    let descriptor = SyntaxProfileDescriptor::new(&FileLanguage::vue(), &vue_options())
        .expect("Vue is a carrier frontend");

    assert_eq!(
        descriptor.canonical_bytes(),
        decode_hex(SYNTAX_PROFILE_GOLDEN_BYTES_HEX)
    );
    assert_eq!(
        descriptor.canonical_digest().to_hex(),
        SYNTAX_PROFILE_GOLDEN_DIGEST
    );
}

#[test]
fn parse_key_canonical_bytes_and_digest_are_pinned() {
    const SOURCE: &str = "<template>[[ value ]]</template>";
    let syntax_profile = syntax_profile(&vue_options());
    let descriptor = ParseKeyDescriptor::new(
        ContentId::from_content_bytes(SOURCE.as_bytes()),
        LanguageId::new("vue"),
        CompatibilityDomainId("verter.framework.syntax"),
        CompatibilityEpoch(7),
        syntax_profile,
    );

    assert_eq!(
        descriptor.canonical_bytes(),
        decode_hex(PARSE_KEY_GOLDEN_BYTES_HEX)
    );
    assert_eq!(
        descriptor.canonical_digest().to_hex(),
        PARSE_KEY_GOLDEN_DIGEST
    );
}

#[test]
fn vue_custom_element_order_and_duplicates_are_irrelevant() {
    let ordered = ParseOptions {
        delimiters: ("{{".to_string(), "}}".to_string()),
        custom_elements: vec!["a-".to_string(), "b-".to_string()],
        svelte_loose: false,
    };
    let permuted_with_duplicate = ParseOptions {
        delimiters: ("{{".to_string(), "}}".to_string()),
        custom_elements: vec!["b-".to_string(), "a-".to_string(), "a-".to_string()],
        svelte_loose: false,
    };

    assert_eq!(
        syntax_profile(&ordered),
        syntax_profile(&permuted_with_duplicate)
    );
}

#[test]
fn vue_parse_affecting_options_change_the_syntax_profile() {
    let baseline = syntax_profile(&ParseOptions::vue_standard());
    let different_delimiters = syntax_profile(&ParseOptions {
        delimiters: ("[[".to_string(), "]]".to_string()),
        custom_elements: Vec::new(),
        svelte_loose: false,
    });
    let custom_element = syntax_profile(&ParseOptions {
        custom_elements: vec!["x-".to_string()],
        svelte_loose: false,
        ..ParseOptions::vue_standard()
    });

    assert_ne!(baseline, different_delimiters);
    assert_ne!(baseline, custom_element);
    assert_ne!(different_delimiters, custom_element);
}

#[test]
fn svelte_profile_has_no_vue_only_option_dimensions() {
    let baseline = syntax_profile_id_for(&FileLanguage::svelte(), &ParseOptions::default())
        .expect("Svelte is a carrier frontend");
    let vue_only_options = syntax_profile_id_for(
        &FileLanguage::svelte(),
        &ParseOptions {
            delimiters: ("[[".to_string(), "]]".to_string()),
            custom_elements: vec!["x-".to_string()],
            svelte_loose: false,
        },
    )
    .expect("Svelte is a carrier frontend");

    assert_eq!(baseline, vue_only_options);
}

#[test]
fn identical_inputs_share_parse_keys_and_parse_affecting_changes_do_not() {
    const SOURCE: &str = "<template>{{ value }}</template>";
    let domain = CompatibilityDomainId("verter.framework.syntax");
    let epoch = CompatibilityEpoch(3);
    let baseline_profile = syntax_profile(&ParseOptions::vue_standard());
    let baseline = parse_key_for(
        SOURCE,
        &FileLanguage::vue(),
        domain,
        epoch,
        &baseline_profile,
    )
    .expect("Vue is a carrier frontend");
    let repeated = parse_key_for(
        SOURCE,
        &FileLanguage::vue(),
        domain,
        epoch,
        &baseline_profile,
    )
    .expect("Vue is a carrier frontend");
    let changed_profile = syntax_profile(&ParseOptions {
        delimiters: ("[[".to_string(), "]]".to_string()),
        custom_elements: Vec::new(),
        svelte_loose: false,
    });
    let changed_options = parse_key_for(
        SOURCE,
        &FileLanguage::vue(),
        domain,
        epoch,
        &changed_profile,
    )
    .expect("Vue is a carrier frontend");
    let changed_content = parse_key_for(
        "<template>{{ other }}</template>",
        &FileLanguage::vue(),
        domain,
        epoch,
        &baseline_profile,
    )
    .expect("Vue is a carrier frontend");

    assert_eq!(baseline, repeated);
    assert_ne!(baseline, changed_options);
    assert_ne!(baseline, changed_content);
}

#[test]
fn script_profiles_encode_the_real_dialect_and_exact_source_bytes() {
    let source = "export const value = 1";
    let ts = FileLanguage::script(ScriptSourceType::Ts);
    let tsx = FileLanguage::script(ScriptSourceType::Tsx);
    let (_, first) = verter_language::default_parse_identity_for(source, &ts).unwrap();
    let (_, repeated) = verter_language::default_parse_identity_for(source, &ts).unwrap();
    let (_, changed_dialect) = verter_language::default_parse_identity_for(source, &tsx).unwrap();
    let (_, changed_source) =
        verter_language::default_parse_identity_for("export const value = 2", &ts).unwrap();

    assert_eq!(first, repeated);
    assert_ne!(first, changed_dialect);
    assert_ne!(first, changed_source);
}
