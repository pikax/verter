//! Unit coverage for the single Svelte compile-options resolver: fold precedence
//! (inline > compile-option), defaults, the fail-closed unsupported-option surface
//! (each of the four, including a `false` value and both origins), and the runes
//! delegation invariant (the resolver never touches / rejects valid runes).

use super::*;
use crate::svelte::parser::parse_svelte;
use crate::svelte::parser::{SvelteElementKind, SvelteNode, SvelteSpecialKind};
use crate::svelte::runtime::{
    compile_client, ClientCompileError, CompileOptionOrigin, CoreOfficialValidationRule,
    OfficialRejection, SvelteRuntimeOptions, UnsupportedSvelteCompileOption,
    UnsupportedSvelteRuntimeSurface,
};
use oxc_allocator::Allocator;

/// Resolve `source` under `opts`.
fn resolve(
    source: &str,
    opts: &SvelteRuntimeOptions,
) -> Result<ResolvedSvelteCompileOptions, UnsupportedSvelteRuntimeSurface> {
    let parsed = parse_svelte(source);
    resolve_svelte_compile_options(source, &parsed, opts)
}

/// The open span of the first root `<svelte:options>` element in `source`.
fn options_open_span(source: &str) -> verter_span::Span {
    let parsed = parse_svelte(source);
    for node in &parsed.template {
        if let SvelteNode::Element(el) = node {
            if matches!(
                el.kind,
                SvelteElementKind::Special(SvelteSpecialKind::Options)
            ) {
                return el.open_span;
            }
        }
    }
    panic!("no root <svelte:options> element in: {source}");
}

fn base_opts() -> SvelteRuntimeOptions {
    SvelteRuntimeOptions {
        filename: Some("App.svelte".to_string()),
        ..Default::default()
    }
}

// ── Defaults ─────────────────────────────────────────────────────────────────

#[test]
fn resolver_defaults_with_no_options_element_or_compile_options() {
    let r = resolve("<button>x</button>\n", &base_opts()).expect("defaults resolve");
    assert_eq!(r.fragments, SvelteFragments::Html);
    assert!(!r.preserve_whitespace);
    assert!(!r.preserve_comments);
    assert!(r.disclose_version, "discloseVersion defaults to true");
}

// ── Namespace: html-only, svg/mathml fail closed ─────────────────────────────
//
// This backend emits html-namespace roots ONLY; a `namespace: 'svg' | 'mathml'`
// selection (compile option OR inline) fails closed with the precise
// `NamespaceUnsupported` refusal (svg/mathml root element emission is a separate
// deferred surface). The precedence rail (INLINE WINS) is still exercised through
// the ONE supported namespace: an inline `namespace="html"` overrides a compile
// option, and the resolver never rejects a supported html selection.

#[test]
fn compile_option_svg_namespace_fails_closed() {
    let opts = SvelteRuntimeOptions {
        namespace: Some(SvelteNamespace::Svg),
        ..base_opts()
    };
    let err = resolve("<button>x</button>\n", &opts).expect_err("compile-option svg fails closed");
    assert!(
        matches!(
            err,
            UnsupportedSvelteRuntimeSurface::NamespaceUnsupported {
                namespace: SvelteNamespace::Svg,
                origin: CompileOptionOrigin::CompileProfile,
                span: None,
            }
        ),
        "namespace=svg → CompileProfile/Svg refusal, got {err:?}"
    );
    assert_eq!(
        err.diagnostic_code(),
        "svelte-runtime-unsupported-namespace"
    );
}

#[test]
fn compile_option_mathml_namespace_fails_closed() {
    let opts = SvelteRuntimeOptions {
        namespace: Some(SvelteNamespace::Mathml),
        ..base_opts()
    };
    let err =
        resolve("<button>x</button>\n", &opts).expect_err("compile-option mathml fails closed");
    assert!(
        matches!(
            err,
            UnsupportedSvelteRuntimeSurface::NamespaceUnsupported {
                namespace: SvelteNamespace::Mathml,
                origin: CompileOptionOrigin::CompileProfile,
                span: None,
            }
        ),
        "namespace=mathml → CompileProfile/Mathml refusal, got {err:?}"
    );
    assert_eq!(
        err.diagnostic_code(),
        "svelte-runtime-unsupported-namespace"
    );
}

#[test]
fn inline_svg_namespace_fails_closed_with_inline_origin_and_span() {
    let source = "<svelte:options namespace=\"svg\" />\n<button>x</button>\n";
    let err = resolve(source, &base_opts()).expect_err("inline svg fails closed");
    assert!(
        matches!(
            err,
            UnsupportedSvelteRuntimeSurface::NamespaceUnsupported {
                namespace: SvelteNamespace::Svg,
                origin: CompileOptionOrigin::Inline,
                span: Some(_),
            }
        ),
        "inline namespace=\"svg\" → Inline/Svg with span, got {err:?}"
    );
    assert_eq!(err.span(), options_open_span(source));
    assert_eq!(
        err.diagnostic_code(),
        "svelte-runtime-unsupported-namespace"
    );
}

#[test]
fn inline_mathml_namespace_fails_closed_with_inline_origin() {
    let source = "<svelte:options namespace=\"mathml\" />\n<button>x</button>\n";
    let err = resolve(source, &base_opts()).expect_err("inline mathml fails closed");
    assert!(
        matches!(
            err,
            UnsupportedSvelteRuntimeSurface::NamespaceUnsupported {
                namespace: SvelteNamespace::Mathml,
                origin: CompileOptionOrigin::Inline,
                span: Some(_),
            }
        ),
        "inline namespace=\"mathml\" → Inline/Mathml, got {err:?}"
    );
}

#[test]
fn inline_html_namespace_overrides_a_compile_option_svg() {
    // INLINE WINS: an inline `namespace="html"` overrides a compile-option `svg`, so the
    // resolver succeeds (html) rather than refusing on the masked svg.
    let opts = SvelteRuntimeOptions {
        namespace: Some(SvelteNamespace::Svg),
        ..base_opts()
    };
    assert!(
        resolve(
            "<svelte:options namespace=\"html\" />\n<button>x</button>\n",
            &opts,
        )
        .is_ok(),
        "an inline html namespace must override (and mask) a compile-option svg"
    );
    // NEGATIVE: WITHOUT the inline override the compile-option svg refuses.
    assert!(
        resolve("<button>x</button>\n", &opts).is_err(),
        "a compile-option svg with no inline override must refuse"
    );
}

#[test]
fn inline_html_namespace_resolves() {
    // The explicit supported form: `namespace="html"` is the default and always resolves.
    assert!(
        resolve(
            "<svelte:options namespace=\"html\" />\n<button>x</button>\n",
            &base_opts(),
        )
        .is_ok(),
        "inline namespace=\"html\" is supported"
    );
}

// ── Namespace at REAL emission (`compile_client`), not only `resolve` ─────────
//
// The `resolve`-level tests above prove the FOLD; these prove the same html-only +
// fail-close behavior at the compile_client (real emission) boundary — the layer a
// consumer actually calls.

#[test]
fn compile_client_inline_html_masks_a_compile_option_svg_namespace() {
    // F1 (discriminating pair): at REAL emission, an inline `<svelte:options
    // namespace="html">` masks a compile-option `namespace: 'svg'` — `compile_client`
    // SUCCEEDS and emits the html `$.from_html` root (the inline value wins the fold
    // BEFORE the fail-close gate).
    let opts = SvelteRuntimeOptions {
        namespace: Some(SvelteNamespace::Svg),
        ..base_opts()
    };
    let js = emit_with(
        "<svelte:options namespace=\"html\" />\n<div>x</div>\n",
        &opts,
    );
    assert!(
        js.contains("$.from_html(`<div>x</div>`)"),
        "inline html masks the compile-option svg → real html emission:\n{js}"
    );
    // NEGATIVE: the masked svg never produces a namespaced factory.
    assert!(
        !js.contains("$.from_svg(") && !js.contains("$.from_mathml("),
        "a masked svg namespace must not emit a namespaced factory:\n{js}"
    );
    // The PAIR: WITHOUT the inline mask, the SAME compile-option svg fails closed at
    // `compile_client` (real emission, not just `resolve`).
    let err = compile_err("<div>x</div>\n", &opts);
    assert!(
        matches!(
            err,
            ClientCompileError::Unsupported(
                UnsupportedSvelteRuntimeSurface::NamespaceUnsupported {
                    namespace: SvelteNamespace::Svg,
                    origin: CompileOptionOrigin::CompileProfile,
                    ..
                }
            )
        ),
        "a compile-option svg with no inline mask fails closed at compile_client, got {err:?}"
    );
}

#[test]
fn compile_client_namespace_option_svg_and_mathml_fail_closed_both_origins() {
    // F2: the namespace-OPTION svg/mathml fail-close is REAL at `compile_client` (not
    // only `resolve`) — both origins (compile-profile + inline), both namespaces.
    let svg_profile = SvelteRuntimeOptions {
        namespace: Some(SvelteNamespace::Svg),
        ..base_opts()
    };
    let mathml_profile = SvelteRuntimeOptions {
        namespace: Some(SvelteNamespace::Mathml),
        ..base_opts()
    };
    // Compile-profile origin (svg + mathml).
    for opts in [&svg_profile, &mathml_profile] {
        match compile_err("<div>x</div>\n", opts) {
            ClientCompileError::Unsupported(surface) => assert_eq!(
                surface.diagnostic_code(),
                "svelte-runtime-unsupported-namespace",
                "a compile-profile namespace option fails closed at compile_client"
            ),
            other => panic!("expected NamespaceUnsupported, got {other:?}"),
        }
    }
    // Inline origin (svg + mathml).
    for token in ["svg", "mathml"] {
        let src = format!("<svelte:options namespace=\"{token}\" />\n<div>x</div>\n");
        match compile_err(&src, &base_opts()) {
            ClientCompileError::Unsupported(surface) => assert_eq!(
                surface.diagnostic_code(),
                "svelte-runtime-unsupported-namespace",
                "an inline namespace={token} fails closed at compile_client"
            ),
            other => panic!("expected NamespaceUnsupported for {token}, got {other:?}"),
        }
    }
}

#[test]
fn dynamic_attribute_math_root_element_fails_closed() {
    // F2: a `<math>` root element carrying a DYNAMIC attribute fails closed at
    // `compile_client` (mirroring the differential corpus's dynamic-svg row) — svg /
    // mathml element emission is a deferred surface in every attribute shape, refused
    // by the non-html-namespace element gate before the attr walk.
    let err = compile_err(
        "<script>let { w } = $props();</script>\n<math display={w}></math>\n",
        &base_opts(),
    );
    assert!(
        matches!(err, ClientCompileError::Unsupported(_)),
        "a dynamic-attribute <math> root must fail closed, got {err:?}"
    );
}

#[test]
fn inline_preserve_whitespace_overrides_the_compile_option() {
    // Compile-option says false; inline `preserveWhitespace` (shorthand → true) wins.
    let opts = SvelteRuntimeOptions {
        preserve_whitespace: Some(false),
        ..base_opts()
    };
    let r = resolve(
        "<svelte:options preserveWhitespace />\n<button>x</button>\n",
        &opts,
    )
    .expect("inline preserveWhitespace resolves");
    assert!(
        r.preserve_whitespace,
        "inline shorthand → true wins over false"
    );
}

#[test]
fn inline_preserve_whitespace_false_overrides_compile_option_true() {
    let opts = SvelteRuntimeOptions {
        preserve_whitespace: Some(true),
        ..base_opts()
    };
    let r = resolve(
        "<svelte:options preserveWhitespace={false} />\n<button>x</button>\n",
        &opts,
    )
    .expect("inline preserveWhitespace={false} resolves");
    assert!(
        !r.preserve_whitespace,
        "inline false wins over the compile-option true"
    );
}

#[test]
fn compile_option_fragments_comments_and_disclose_thread() {
    // `fragments` / `preserveComments` / `discloseVersion` thread onto the resolved
    // object. (The `name` component-function name is derived by lowering, not carried
    // on the resolved options — its emission is asserted by
    // `name_option_sets_the_component_function_name` below.)
    let opts = SvelteRuntimeOptions {
        fragments: Some(SvelteFragments::Tree),
        preserve_comments: Some(true),
        disclose_version: Some(false),
        name: Some("MyName".to_string()),
        ..base_opts()
    };
    let r = resolve("<button>x</button>\n", &opts).expect("compile options resolve");
    assert_eq!(r.fragments, SvelteFragments::Tree);
    assert!(r.preserve_comments);
    assert!(!r.disclose_version);
}

// ── Fail-closed: compile-option origin ───────────────────────────────────────

#[test]
fn compile_option_accessors_fails_closed_even_when_false() {
    for value in [true, false] {
        let opts = SvelteRuntimeOptions {
            accessors: Some(value),
            ..base_opts()
        };
        let err =
            resolve("<button>x</button>\n", &opts).expect_err("accessors present fails closed");
        assert!(
            matches!(
                err,
                UnsupportedSvelteRuntimeSurface::CompileOptionUnsupported {
                    option: UnsupportedSvelteCompileOption::Accessors,
                    origin: CompileOptionOrigin::CompileProfile,
                    span: None,
                }
            ),
            "accessors={value} → CompileProfile/Accessors, got {err:?}"
        );
        assert_eq!(
            err.diagnostic_code(),
            "svelte-runtime-unsupported-accessors"
        );
    }
}

#[test]
fn compile_option_immutable_fails_closed_even_when_false() {
    for value in [true, false] {
        let opts = SvelteRuntimeOptions {
            immutable: Some(value),
            ..base_opts()
        };
        let err = resolve("<button>x</button>\n", &opts).expect_err("immutable fails closed");
        assert!(
            matches!(
                err,
                UnsupportedSvelteRuntimeSurface::CompileOptionUnsupported {
                    option: UnsupportedSvelteCompileOption::Immutable,
                    origin: CompileOptionOrigin::CompileProfile,
                    span: None,
                }
            ),
            "immutable={value} → CompileProfile/Immutable, got {err:?}"
        );
        assert_eq!(
            err.diagnostic_code(),
            "svelte-runtime-unsupported-immutable"
        );
    }
}

#[test]
fn compile_option_hmr_fails_closed_even_when_false() {
    for value in [true, false] {
        let opts = SvelteRuntimeOptions {
            hmr: Some(value),
            ..base_opts()
        };
        let err = resolve("<button>x</button>\n", &opts).expect_err("hmr fails closed");
        assert!(
            matches!(
                err,
                UnsupportedSvelteRuntimeSurface::CompileOptionUnsupported {
                    option: UnsupportedSvelteCompileOption::Hmr,
                    origin: CompileOptionOrigin::CompileProfile,
                    span: None,
                }
            ),
            "hmr={value} → CompileProfile/Hmr, got {err:?}"
        );
        assert_eq!(err.diagnostic_code(), "svelte-runtime-unsupported-hmr");
    }
}

#[test]
fn compile_option_compatibility_component_api_4_fails_closed() {
    let opts = SvelteRuntimeOptions {
        compatibility_component_api: Some(4),
        ..base_opts()
    };
    let err = resolve("<button>x</button>\n", &opts).expect_err("componentApi 4 fails closed");
    assert!(
        matches!(
            err,
            UnsupportedSvelteRuntimeSurface::CompileOptionUnsupported {
                option: UnsupportedSvelteCompileOption::CompatibilityComponentApi,
                origin: CompileOptionOrigin::CompileProfile,
                span: None,
            }
        ),
        "componentApi 4 → CompileProfile/CompatibilityComponentApi, got {err:?}"
    );
    assert_eq!(
        err.diagnostic_code(),
        "svelte-runtime-unsupported-compatibility-component-api"
    );
}

#[test]
fn compile_option_compatibility_component_api_5_and_absent_are_supported() {
    // Only `5` and absent (`None`) resolve; EVERY non-`5` explicit value fails
    // closed — `4` as the refused compat feature and any other value (`0`, `6`, …)
    // as an official `options_invalid_value` error (the sibling
    // `…_non_supported_values_fail_closed` test asserts that half).
    for value in [None, Some(5)] {
        let opts = SvelteRuntimeOptions {
            compatibility_component_api: value,
            ..base_opts()
        };
        assert!(
            resolve("<button>x</button>\n", &opts).is_ok(),
            "componentApi {value:?} must be supported"
        );
    }
}

#[test]
fn compile_option_compatibility_component_api_non_supported_values_fail_closed() {
    // Fail-closed, no fail-open: svelte@5.56.3 accepts `componentApi` ∈ {4,5}
    // ONLY — `4` is the refused compat FEATURE, `5` is the current supported API, and
    // any OTHER value (`0`, `6`, …) is an official `options_invalid_value` error.
    // Verter must fail closed on every non-`5` explicit value (never resolve a module),
    // reusing the `CompatibilityComponentApi` refusal to keep the enum closed.
    for value in [0u32, 6, 99] {
        let opts = SvelteRuntimeOptions {
            compatibility_component_api: Some(value),
            ..base_opts()
        };
        let err = resolve("<button>x</button>\n", &opts)
            .expect_err("a non-{4,5} componentApi must fail closed");
        assert!(
            matches!(
                err,
                UnsupportedSvelteRuntimeSurface::CompileOptionUnsupported {
                    option: UnsupportedSvelteCompileOption::CompatibilityComponentApi,
                    origin: CompileOptionOrigin::CompileProfile,
                    span: None,
                }
            ),
            "componentApi {value} → CompileProfile/CompatibilityComponentApi fail-closed, got {err:?}"
        );
        assert_eq!(
            err.diagnostic_code(),
            "svelte-runtime-unsupported-compatibility-component-api"
        );
    }
}

// ── Fail-closed: inline origin ───────────────────────────────────────────────

#[test]
fn inline_immutable_fails_closed_with_inline_origin_and_span() {
    let source = "<svelte:options immutable />\n<button>x</button>\n";
    let err = resolve(source, &base_opts()).expect_err("inline immutable fails closed");
    assert!(
        matches!(
            err,
            UnsupportedSvelteRuntimeSurface::CompileOptionUnsupported {
                option: UnsupportedSvelteCompileOption::Immutable,
                origin: CompileOptionOrigin::Inline,
                span: Some(_),
            }
        ),
        "inline immutable → Inline/Immutable with span, got {err:?}"
    );
    // The span is the `<svelte:options>` open tag.
    assert_eq!(err.span(), options_open_span(source));
}

#[test]
fn inline_tag_hits_the_defensive_reject_arm_not_a_silent_ok() {
    // `<svelte:options tag>` is a parser-FIRST hard error
    // (`svelte_options_deprecated_tag`) rejected upstream, so `compile_client` never
    // reaches the resolver with a `tag` attribute. The resolver still carries a
    // DEFENSIVE arm: called DIRECTLY on a parsed `<svelte:options tag="x">` AST
    // (bypassing the parser gate), it must NOT silently fold the tag away and return
    // `Ok` — it produces the SAME official rejection the parser-first gate mints.
    let source = "<svelte:options tag=\"x\" />\n<button>x</button>\n";
    let err = resolve(source, &base_opts())
        .expect_err("a direct-call `tag` must hit the defensive reject arm, not silent Ok");
    assert!(
        matches!(
            err,
            UnsupportedSvelteRuntimeSurface::OfficialReject {
                rejection: OfficialRejection {
                    rule: CoreOfficialValidationRule::OptionsInvalid,
                    official_code: "svelte_options_deprecated_tag",
                },
                ..
            }
        ),
        "the defensive `tag` arm mirrors the official svelte_options_deprecated_tag reject, got {err:?}"
    );
    // The refusal carries the `<svelte:options>` open-tag span.
    assert_eq!(err.span(), options_open_span(source));
}

#[test]
fn inline_accessors_fails_closed_with_inline_origin() {
    let source = "<svelte:options accessors={false} />\n<button>x</button>\n";
    let err = resolve(source, &base_opts()).expect_err("inline accessors fails closed");
    assert!(
        matches!(
            err,
            UnsupportedSvelteRuntimeSurface::CompileOptionUnsupported {
                option: UnsupportedSvelteCompileOption::Accessors,
                origin: CompileOptionOrigin::Inline,
                span: Some(_),
            }
        ),
        "inline accessors={{false}} → Inline/Accessors, got {err:?}"
    );
}

// ── Runes delegation (S6): the resolver never touches / rejects runes ─────────

#[test]
fn inline_runes_is_not_touched_by_the_resolver() {
    // Both boolean forms + the shorthand resolve to Ok — runes flows through the
    // existing mode-inference plumbing, never the resolver.
    for src in [
        "<svelte:options runes />\n<button>x</button>\n",
        "<svelte:options runes={true} />\n<button>x</button>\n",
        "<svelte:options runes={false} />\n<button>x</button>\n",
    ] {
        assert!(
            resolve(src, &base_opts()).is_ok(),
            "valid inline runes must NOT be rejected by the resolver: {src}"
        );
    }
}

#[test]
fn compile_option_runes_does_not_affect_resolution() {
    // The `runes` compile option is delegated (unread by the resolver); it never
    // fails closed and never appears on the resolved object.
    for runes in [None, Some(true), Some(false)] {
        let opts = SvelteRuntimeOptions {
            runes,
            ..base_opts()
        };
        assert!(
            resolve("<button>x</button>\n", &opts).is_ok(),
            "runes={runes:?} must resolve"
        );
    }
}

#[test]
fn runes_emission_is_equivalent_whether_inline_or_compile_option() {
    // S6 topology-equivalence: a runes-mode component emits the SAME module whether
    // runes is selected inline (`<svelte:options runes>`) or as a compile option —
    // the resolver preserves runes precedence in both directions, never re-infers.
    let inline = "<svelte:options runes />\n<script>let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n";
    let body = "<script>let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n";

    let alloc = Allocator::default();
    let inline_parsed = parse_svelte(inline);
    let inline_opts = SvelteRuntimeOptions {
        filename: Some("App.svelte".to_string()),
        ..Default::default()
    };
    let inline_module = compile_client(inline, &inline_parsed, &inline_opts, &alloc, false, false)
        .expect("inline runes compiles");

    let alloc2 = Allocator::default();
    let body_parsed = parse_svelte(body);
    let compile_opts = SvelteRuntimeOptions {
        filename: Some("App.svelte".to_string()),
        runes: Some(true),
        ..Default::default()
    };
    let compile_module = compile_client(body, &body_parsed, &compile_opts, &alloc2, false, false)
        .expect("compile-option runes compiles");

    assert_eq!(
        inline_module.code, compile_module.code,
        "runes emission must be identical inline vs compile-option"
    );
    // Both are the runes-mode output (state rune emits `$.state`).
    assert!(inline_module.code.contains("$.state(0)"));
}

// ── S4: per-option client codegen consumers ──────────────────────────────────

/// Emit `source` under `opts`.
fn emit_with(source: &str, opts: &SvelteRuntimeOptions) -> String {
    let alloc = Allocator::default();
    let parsed = parse_svelte(source);
    compile_client(source, &parsed, opts, &alloc, false, false)
        .expect("compiles")
        .code
}

/// The ROOT region's clone-factory family for `source` — the plan-level namespace the
/// root factory selects (the classifier gates full SVG/MathML element EMISSION as an
/// element-breadth concern, so the namespace factory is asserted at the static-template
/// plan the diff-oracle corpus keys on, not the emitted module).
fn root_factory(source: &str) -> crate::svelte::runtime::html::TemplateFactory {
    let alloc = Allocator::default();
    let parsed = parse_svelte(source);
    let ir =
        crate::svelte::runtime::lower_parsed_svelte_to_ir(source, &parsed, &base_opts(), &alloc)
            .expect("lowers");
    crate::svelte::runtime::plan_static_templates(&ir, None)
        .templates
        .into_iter()
        .next()
        .expect("a root factory")
}

/// Compile `source` and return the client compile error (asserting it fails closed).
fn compile_err(source: &str, opts: &SvelteRuntimeOptions) -> ClientCompileError {
    let alloc = Allocator::default();
    let parsed = parse_svelte(source);
    compile_client(source, &parsed, opts, &alloc, false, false).expect_err("must fail closed")
}

#[test]
fn svg_root_element_fails_closed() {
    // An `<svg>` root element fails closed: svg element emission (the `$.from_svg`
    // root-helper layer + namespaced element cloning) is a separate deferred surface,
    // so the element allowlist gate refuses rather than cloning it as html.
    let err = compile_err("<svg><circle /></svg>\n", &base_opts());
    assert!(
        matches!(err, ClientCompileError::Unsupported(_)),
        "an <svg> root must fail closed, got {err:?}"
    );
    // NEGATIVE: it never compiles a module cloning the svg as html.
    let alloc = Allocator::default();
    let parsed = parse_svelte("<svg><circle /></svg>\n");
    assert!(
        compile_client(
            "<svg><circle /></svg>\n",
            &parsed,
            &base_opts(),
            &alloc,
            false,
            false
        )
        .is_err(),
        "an <svg> root must never emit a client module"
    );
}

#[test]
fn mathml_root_element_fails_closed() {
    let err = compile_err("<math><mrow></mrow></math>\n", &base_opts());
    assert!(
        matches!(err, ClientCompileError::Unsupported(_)),
        "a <math> root must fail closed, got {err:?}"
    );
}

#[test]
fn plain_html_root_stays_from_html() {
    // A plain element root selects `$.from_html`, never a namespaced factory (svg /
    // mathml are refused, so there is no namespaced factory family at all).
    let js = emit_with("<div>x</div>\n", &base_opts());
    assert!(
        js.contains("$.from_html(`<div>"),
        "div root → $.from_html:\n{js}"
    );
    assert!(
        !js.contains("$.from_svg(") && !js.contains("$.from_mathml("),
        "a plain root must NOT emit a namespaced factory:\n{js}"
    );
    // And the plan-level factory is the html clone family.
    match root_factory("<div>x</div>\n") {
        crate::svelte::runtime::html::TemplateFactory::FromHtml { fragments, .. } => {
            assert_eq!(fragments, SvelteFragments::Html);
        }
        other => panic!("div root must be a clone factory, got {other:?}"),
    }
}

#[test]
fn disclose_version_false_drops_the_side_effect_import() {
    let default_js = emit_with("<div>x</div>\n", &base_opts());
    assert!(
        default_js.contains("import 'svelte/internal/disclose-version';"),
        "discloseVersion defaults to true (the import is present):\n{default_js}"
    );
    let opts = SvelteRuntimeOptions {
        disclose_version: Some(false),
        ..base_opts()
    };
    let js = emit_with("<div>x</div>\n", &opts);
    assert!(
        !js.contains("svelte/internal/disclose-version"),
        "discloseVersion=false drops the side-effect import:\n{js}"
    );
    // NEGATIVE: the rest of the module still emits (the runtime namespace import stays).
    assert!(js.contains("import * as $ from 'svelte/internal/client';"));
}

#[test]
fn preserve_whitespace_keeps_insignificant_whitespace_in_the_skeleton() {
    let source = "<div> a </div>\n";
    let default_js = emit_with(source, &base_opts());
    assert!(
        default_js.contains("<div>a</div>"),
        "the default trims leading/trailing whitespace:\n{default_js}"
    );
    let opts = SvelteRuntimeOptions {
        preserve_whitespace: Some(true),
        ..base_opts()
    };
    let js = emit_with(source, &opts);
    assert!(
        js.contains("<div> a </div>"),
        "preserveWhitespace keeps the surrounding whitespace:\n{js}"
    );
    // NEGATIVE: the preserved skeleton is NOT the trimmed one.
    assert!(!js.contains("<div>a</div>"));
}

#[test]
fn preserve_whitespace_region_walk_matches_the_skeleton_offsets() {
    // Regression: the region template skeleton and the emit-side walk MUST derive
    // their whitespace/comment cleaning from the SAME `region_ctx`. A prior split fed
    // the skeleton `preserveWhitespace: true` (retaining the leading `\n\n`) while the
    // walk re-cleaned with a hardcoded `preserve_ws = false`, so `$.sibling` offsets
    // counted a whitespace-STRIPPED fragment against a whitespace-PRESERVING skeleton —
    // the reactive `{x}` binding then landed on the inter-element space text node
    // instead of the `<span>`.
    let source = "<svelte:options preserveWhitespace />\n<script>let { x } = $props();</script>\n<div>a</div> <span>{x}</span>\n";
    let js = emit_with(source, &base_opts());

    // The skeleton preserves the leading whitespace (the `<svelte:options>` /
    // `<script>` lines strip to `\n\n`), so the fragment's top-level nodes are
    // [text `\n\n`, `<div>`, text ` `, `<span>`] — the `<span>` sits at sibling offset 3.
    assert!(
        js.contains("var root = $.from_html(`\n\n<div>a</div> <span> </span>\n`, 1);"),
        "the skeleton retains the leading/trailing whitespace under preserveWhitespace:\n{js}"
    );
    // POSITIVE: the walk reaches the `<span>` at the whitespace-PRESERVING offset 3.
    assert!(
        js.contains("var span = $.sibling($.first_child(fragment), 3);"),
        "the walk offset matches the whitespace-preserving skeleton (offset 3):\n{js}"
    );
    // NEGATIVE: it is NOT the whitespace-STRIPPED offset 2 (which would land the
    // reactive binding on the inter-element space text node).
    assert!(
        !js.contains("$.sibling($.first_child(fragment), 2)"),
        "the walk must not re-clean with a desynced preserve_ws=false (offset 2):\n{js}"
    );
}

#[test]
fn name_option_sets_the_component_function_name() {
    let opts = SvelteRuntimeOptions {
        name: Some("MyWidget".to_string()),
        filename: Some("ignored.svelte".to_string()),
        ..Default::default()
    };
    let js = emit_with("<div>x</div>\n", &opts);
    assert!(
        js.contains("export default function MyWidget($$anchor)"),
        "the `name` option overrides the filename-derived function name:\n{js}"
    );
    // NEGATIVE: the filename-derived name does not appear.
    assert!(!js.contains("function Ignored("));
}

#[test]
fn name_reserved_word_deconflicts_with_a_counter_suffix() {
    // A `name` that sanitizes to a JS reserved word is an invalid function
    // name, so `module.scope.generate` suffixes `_1` (svelte@5.56.3: `var` → `var_1`,
    // `class` → `class_1`, `await` → `await_1`).
    for (name, expected) in [("var", "var_1"), ("class", "class_1"), ("await", "await_1")] {
        let opts = SvelteRuntimeOptions {
            name: Some(name.to_string()),
            ..base_opts()
        };
        let js = emit_with("<div>x</div>\n", &opts);
        assert!(
            js.contains(&format!("export default function {expected}($$anchor)")),
            "reserved `{name}` deconflicts to `{expected}`:\n{js}"
        );
        // NEGATIVE: the bare reserved word is never emitted as the function name.
        assert!(
            !js.contains(&format!("function {name}($$anchor)")),
            "the bare reserved word `{name}` must never be the function name:\n{js}"
        );
    }
    // CONTROL: a non-reserved name is used verbatim (no suffix).
    let opts = SvelteRuntimeOptions {
        name: Some("widget".to_string()),
        ..base_opts()
    };
    let js = emit_with("<div>x</div>\n", &opts);
    assert!(
        js.contains("export default function widget($$anchor)")
            && !js.contains("function widget_1("),
        "a non-reserved name is used verbatim:\n{js}"
    );
}

#[test]
fn name_collision_with_a_declared_binding_deconflicts_with_a_counter_suffix() {
    // A `name` colliding with a declared user binding (a top-level `let`, an
    // import, an `{#each}` local — svelte's `root.conflicts`) suffixes `_1`
    // (svelte@5.56.3: `name: 'foo'` + `let foo` → `foo_1`).
    let opts = SvelteRuntimeOptions {
        name: Some("foo".to_string()),
        ..base_opts()
    };
    let js = emit_with(
        "<script>let { foo } = $props();</script>\n<div>{foo}</div>\n",
        &opts,
    );
    assert!(
        js.contains("export default function foo_1("),
        "a name colliding with a declared `foo` binding deconflicts to `foo_1`:\n{js}"
    );
    // NEGATIVE: the raw `foo` is never the function name (it is the user binding).
    assert!(
        !js.contains("export default function foo("),
        "the component function must not shadow the user binding name:\n{js}"
    );
    // A collision with an `{#each}` local (a NESTED scope binding) deconflicts too.
    let opts_each = SvelteRuntimeOptions {
        name: Some("item".to_string()),
        ..base_opts()
    };
    let each_js = emit_with(
        "<script>let { xs } = $props();</script>\n{#each xs as item}{item}{/each}\n",
        &opts_each,
    );
    assert!(
        each_js.contains("export default function item_1("),
        "a name colliding with an {{#each}} local deconflicts to `item_1`:\n{each_js}"
    );
    // CONTROL: a name with NO declared collision is used verbatim.
    let opts_free = SvelteRuntimeOptions {
        name: Some("foo".to_string()),
        ..base_opts()
    };
    let free_js = emit_with("<div>x</div>\n", &opts_free);
    assert!(
        free_js.contains("export default function foo(") && !free_js.contains("function foo_1("),
        "a name with no declared collision is used verbatim:\n{free_js}"
    );
}

#[test]
fn name_colliding_with_a_referenced_identifier_deconflicts() {
    // `Scope.generate` deconflicts against scope REFERENCES (used identifiers),
    // not only declarations. A `name: "String"` in a component that REFERENCES the
    // global `String` must emit `String_1` (svelte@5.56.3: the generated component
    // name must not shadow a referenced binding), never bare `String`.
    let opts = SvelteRuntimeOptions {
        name: Some("String".to_string()),
        ..base_opts()
    };
    let js = emit_with(
        "<script>let { v } = $props();</script>\n<div title={String(v)}></div>\n",
        &opts,
    );
    assert!(
        js.contains("export default function String_1("),
        "a name colliding with a referenced `String` deconflicts to `String_1`:\n{js}"
    );
    // NEGATIVE: the raw `String` is never the function name (it would shadow the ref).
    assert!(
        !js.contains("export default function String("),
        "the component function must not shadow the referenced identifier:\n{js}"
    );
    // CONTROL: with NO reference to `String`, the name is used verbatim.
    let free = emit_with("<div>x</div>\n", &opts);
    assert!(
        free.contains("export default function String(") && !free.contains("function String_1("),
        "a name with no referenced collision is used verbatim:\n{free}"
    );
}

#[test]
fn name_colliding_with_an_instance_script_referenced_identifier_deconflicts() {
    // `Scope.generate` (svelte's `module.scope.generate`) deconflicts the
    // component name against the MODULE scope's FULL reference set — which propagates
    // every free identifier referenced in the INSTANCE SCRIPT (and module script), not
    // just template expressions. A `name: "String"` whose INSTANCE SCRIPT references
    // the global `String` must emit `String_1` (svelte@5.56.3), never bare `String`
    // (which would shadow the global the script's `String(x)` call resolves to →
    // broken runtime code).
    let opts = SvelteRuntimeOptions {
        name: Some("String".to_string()),
        ..base_opts()
    };
    let js = emit_with(
        "<script>export let x;\n$: y = String(x);</script>\n<div>{y}</div>\n",
        &opts,
    );
    assert!(
        js.contains("export default function String_1("),
        "a name colliding with an instance-script-referenced `String` deconflicts to `String_1`:\n{js}"
    );
    // NEGATIVE: the raw `String` is never the function name (it would shadow the global
    // the instance script calls).
    assert!(
        !js.contains("export default function String("),
        "the component function must not shadow the instance-script global reference:\n{js}"
    );
    // CONTROL: with NO script reference to `String`, the name is used verbatim (the
    // deconfliction is driven by the actual reference, not the name string alone).
    let free = emit_with(
        "<script>export let x;\n$: y = x + 1;</script>\n<div>{y}</div>\n",
        &opts,
    );
    assert!(
        free.contains("export default function String(") && !free.contains("function String_1("),
        "a name with no script reference to it is used verbatim:\n{free}"
    );
}

#[test]
fn name_colliding_with_a_store_base_declaration_deconflicts() {
    // The component name deconflicts against the SOURCE-FORM declared name, not the
    // synthesized `$`-subscription accessor. A `const Foo = writable(0)` store declares
    // the base `Foo`; the auto-subscription binding is the synthesized `$Foo`. A
    // `name: "Foo"` therefore collides with the declared `const Foo` and must emit
    // `Foo_1` (svelte@5.56.3) — NEVER a bare `function Foo` that DUPLICATES the source
    // `const Foo` (broken JS: two `Foo` declarations in one scope).
    let opts = SvelteRuntimeOptions {
        name: Some("Foo".to_string()),
        ..base_opts()
    };
    let js = emit_with(
        "<script>import { writable } from 'svelte/store';\nconst Foo = writable(0);</script>\n<div>{$Foo}</div>\n",
        &opts,
    );
    assert!(
        js.contains("export default function Foo_1("),
        "a name colliding with a store BASE declaration deconflicts to `Foo_1`:\n{js}"
    );
    // NEGATIVE: the bare `function Foo` is never emitted — it would duplicate the
    // source `const Foo` and break the module.
    assert!(
        !js.contains("export default function Foo("),
        "the component function must not duplicate the source `const Foo` declaration:\n{js}"
    );
}

#[test]
fn name_colliding_with_a_module_script_import_deconflicts() {
    // A `<script module>` import local is a source-form declaration in the module
    // scope. A `name: "Foo"` colliding with `import Foo from './x.js'` must emit
    // `Foo_1` (svelte@5.56.3), never a bare `function Foo` shadowing the import.
    let opts = SvelteRuntimeOptions {
        name: Some("Foo".to_string()),
        ..base_opts()
    };
    let js = emit_with(
        "<script module>import Foo from './x.js';</script>\n<div>{Foo}</div>\n",
        &opts,
    );
    assert!(
        js.contains("export default function Foo_1("),
        "a name colliding with a module-script import deconflicts to `Foo_1`:\n{js}"
    );
    assert!(
        !js.contains("export default function Foo("),
        "the component function must not shadow the module-script import:\n{js}"
    );
}

#[test]
fn name_colliding_with_a_slot_let_local_deconflicts() {
    // A slot `let:row` binding is a source-form template declaration. A `name: "row"`
    // colliding with it must emit `row_1` (svelte@5.56.3), never a bare `function row`.
    let opts = SvelteRuntimeOptions {
        name: Some("row".to_string()),
        ..base_opts()
    };
    let js = emit_with(
        "<script>import Child from './Child.svelte';</script>\n<Child let:row>{row}</Child>\n",
        &opts,
    );
    assert!(
        js.contains("export default function row_1("),
        "a name colliding with a slot `let:` local deconflicts to `row_1`:\n{js}"
    );
    assert!(
        !js.contains("export default function row("),
        "the component function must not shadow the slot `let:` local:\n{js}"
    );
}

#[test]
fn name_does_not_reserve_an_unreferenced_synthesized_store_accessor() {
    // The authored-vs-synthesized distinction: every top-level declared base (here the
    // component import `Foo`) gets an INERT synthesized `$Foo` subscription-accessor
    // binding registered in the scope graph. That synthesized `$Foo` only becomes a
    // real binding in svelte@5.56.3 when the source actually references `$Foo`. When
    // `$Foo` is NEVER referenced, svelte creates no `$Foo` binding, so a `name: "$Foo"`
    // is used verbatim (`function $Foo`). The deconfliction must derive from the SOURCE
    // (the base `Foo` is declared; `$Foo` is never referenced), NOT falsely reserve the
    // inert synthesized accessor.
    let opts = SvelteRuntimeOptions {
        name: Some("$Foo".to_string()),
        ..base_opts()
    };
    let js = emit_with(
        "<script>import Foo from './Foo.svelte';</script>\n<Foo />\n",
        &opts,
    );
    assert!(
        js.contains("export default function $Foo("),
        "an unreferenced synthesized `$Foo` accessor must not reserve the name `$Foo`:\n{js}"
    );
    // NEGATIVE: it is NOT deconflicted to `$Foo_1` — an inert synthesized
    // scope-graph binding never over-reserves the `$Foo` name.
    assert!(
        !js.contains("export default function $Foo_1("),
        "the name `$Foo` must not deconflict against an inert synthesized accessor:\n{js}"
    );
}

#[test]
fn name_astral_char_sanitizes_per_utf16_code_unit() {
    // `Scope.generate` sanitizes per UTF-16 CODE UNIT (the JS-string regex), not
    // per Unicode scalar. An astral `name: "💩"` (ONE scalar, TWO UTF-16 units) becomes
    // `__` (two `_`), matching svelte@5.56.3 — NOT `_` (a single per-scalar replace).
    let opts = SvelteRuntimeOptions {
        name: Some("💩".to_string()),
        ..base_opts()
    };
    let js = emit_with("<div>x</div>\n", &opts);
    assert!(
        js.contains("export default function __($$anchor)"),
        "an astral name sanitizes to TWO underscores (per UTF-16 unit):\n{js}"
    );
    // NEGATIVE: it is NOT the single per-scalar underscore.
    assert!(
        !js.contains("export default function _($$anchor)"),
        "the astral name must not collapse to a single underscore:\n{js}"
    );
}

#[test]
fn preserve_comments_retains_the_comment_and_shifts_sibling_offsets() {
    // An interior `<!-- c -->` between a dropped position and a reactive `<span>`.
    let source = "<script>let { x } = $props();</script>\n<div><!-- c --><span>{x}</span></div>\n";

    // DEFAULT (preserveComments: false): the comment is dropped from the skeleton, so the
    // span is the DIRECT first child of the div.
    let default_js = emit_with(source, &base_opts());
    assert!(
        !default_js.contains("<!--"),
        "the default drops the comment from the skeleton:\n{default_js}"
    );
    assert!(
        default_js.contains("var span = $.child(div);"),
        "with the comment dropped the span is the direct first child:\n{default_js}"
    );

    // preserveComments: true — the comment occupies its OWN DOM node, retained in the
    // skeleton (`<!-- c -->`) AND bound as its own walk anchor, matching svelte@5.56.3:
    // `var node = $.child(div); var span = $.sibling(node);` (every retained comment
    // inside a walked region gets a `var node` binding, NOT an inline offset-skip).
    let opts = SvelteRuntimeOptions {
        preserve_comments: Some(true),
        ..base_opts()
    };
    let js = emit_with(source, &opts);
    assert!(
        js.contains("<div><!-- c -->"),
        "the retained comment is serialized into the skeleton:\n{js}"
    );
    // POSITIVE: the retained comment is bound as its own walk anchor (svelte's form).
    assert!(
        js.contains("var node = $.child(div);"),
        "the retained comment is bound as its own walk anchor var:\n{js}"
    );
    // POSITIVE: the span reaches via `$.sibling(node)` (offset 1 from the comment anchor).
    assert!(
        js.contains("var span = $.sibling(node);"),
        "the span reaches via $.sibling(node) (offset 1 from the comment anchor):\n{js}"
    );
    // NEGATIVE: NOT the inline offset-skip form (Verter's prior divergence from svelte).
    assert!(
        !js.contains("$.sibling($.child(div))"),
        "the retained comment must not be skipped via an inline offset walk:\n{js}"
    );
    // NEGATIVE: the span is not the direct first child (the comment shifts it).
    assert!(
        !js.contains("var span = $.child(div);"),
        "the span must not be the direct first child when the comment is present:\n{js}"
    );
}

#[test]
fn preserve_comments_serializes_empty_and_data_comment_forms() {
    // A data comment → `<!--data-->`; an empty `<!---->` → the bare `<!>` marker
    // (svelte `stringify`). Both retained under preserveComments.
    let opts = SvelteRuntimeOptions {
        preserve_comments: Some(true),
        ..base_opts()
    };
    let data = emit_with("<div><!--keep me--><span>x</span></div>\n", &opts);
    assert!(
        data.contains("<!--keep me-->"),
        "a data comment serializes as <!--data-->:\n{data}"
    );
    let empty = emit_with("<div><!----><span>x</span></div>\n", &opts);
    assert!(
        empty.contains("<div><!><span>x</span></div>"),
        "an empty comment serializes as the bare <!> marker:\n{empty}"
    );
}

#[test]
fn sole_retained_comment_emits_the_comment_fragment_factory() {
    // When the SOLE template node is a retained comment (preserveComments),
    // svelte's Fragment special-case (`nodes.length === 1 && nodes[0].type ===
    // 'comment'`) emits `$.comment()` as the fragment factory, NOT a
    // `$.from_html(`<!-- … -->`)` clone (distinct helper topology).
    let opts = SvelteRuntimeOptions {
        preserve_comments: Some(true),
        ..base_opts()
    };
    let js = emit_with("<!-- hello -->\n", &opts);
    assert!(
        js.contains("var fragment = $.comment();"),
        "a sole retained comment uses the $.comment() fragment factory:\n{js}"
    );
    // NEGATIVE: it is NOT a from_html clone of the comment markup.
    assert!(
        !js.contains("$.from_html("),
        "a sole retained comment must not clone a from_html template:\n{js}"
    );
    // CONTROL: the SAME comment is a from_html clone when NOT sole (a sibling element
    // makes the fragment a multi-node template, so the `$.comment()` special case does
    // not apply).
    let sibling = emit_with("<!-- hello --><div>x</div>\n", &opts);
    assert!(
        sibling.contains("$.from_html(") && !sibling.contains("$.comment()"),
        "a comment with a sibling is a from_html template, not $.comment():\n{sibling}"
    );
}

/// Emit `source` under `fragments: 'tree'` with the given namespace/scoping fixture.
fn emit_tree(source: &str) -> String {
    let opts = SvelteRuntimeOptions {
        fragments: Some(SvelteFragments::Tree),
        ..base_opts()
    };
    emit_with(source, &opts)
}

/// The ROOT region's `TemplateFactory` under `fragments: 'tree'` (plan-level).
///
/// The SVG / MathML element EMISSION gate is an element-breadth concern (the client
/// surface classifier refuses full SVG/MathML element emission), so — exactly like
/// [`root_factory`] for the namespace-factory family — the objectify output is asserted
/// at the static-template plan the tree serializer produces, not the emitted module.
fn tree_root_factory(source: &str) -> crate::svelte::runtime::html::TemplateFactory {
    let alloc = Allocator::default();
    let parsed = parse_svelte(source);
    let opts = SvelteRuntimeOptions {
        fragments: Some(SvelteFragments::Tree),
        ..base_opts()
    };
    let resolved = resolve_svelte_compile_options(source, &parsed, &opts).expect("resolves");
    let mut ir = crate::svelte::runtime::lower_parsed_svelte_to_ir(source, &parsed, &opts, &alloc)
        .expect("lowers");
    ir.root_options = crate::svelte::runtime::ir::RootCompileOptions {
        fragments: resolved.fragments,
        preserve_whitespace: resolved.preserve_whitespace,
        preserve_comments: resolved.preserve_comments,
        disclose_version: resolved.disclose_version,
    };
    crate::svelte::runtime::plan_static_templates(&ir, None)
        .templates
        .into_iter()
        .next()
        .expect("a root factory")
}

/// The ROOT region's `$.from_tree` objectified ARRAY LITERAL for `source` (plan-level).
fn tree_literal(source: &str) -> String {
    match tree_root_factory(source) {
        crate::svelte::runtime::html::TemplateFactory::FromHtml {
            tree: Some(tree), ..
        } => tree,
        other => panic!("expected a tree factory carrying an objectified literal, got {other:?}"),
    }
}

#[test]
fn fragments_tree_emits_the_from_tree_objectified_factory() {
    // The `$.from_tree` objectifier mirrors the HTML-string skeleton: the element name,
    // the baked attrs object, and the spread children. A reactive interpolation child is
    // the official ` ` placeholder text node (`['span', null, ' ']`). Byte-parity with
    // svelte@5.56.3's `objectify` / `as_tree`.
    let js = emit_tree(
        "<script>let { x } = $props();</script>\n<div class=\"card\">hello <span>{x}</span></div>\n",
    );
    assert!(
        js.contains(
            "var root = $.from_tree([['div', { class: 'card' }, 'hello ', ['span', null, ' ']]]);"
        ),
        "the tree factory objectifies the region roots:\n{js}"
    );
    // NEGATIVE: it is NOT the html-string clone family.
    assert!(
        !js.contains("$.from_html(") && !js.contains("$.from_svg("),
        "tree mode never emits a string-clone factory:\n{js}"
    );
    // The DOM walk is byte-identical to the html-clone walk (only the factory differs).
    assert!(
        js.contains("var span = $.sibling($.child(div));"),
        "the tree walk reaches the reactive span identically:\n{js}"
    );
}

#[test]
fn fragments_tree_multiroot_carries_the_fragment_flag() {
    // A MULTI-ROOT tree template carries `TEMPLATE_FRAGMENT` (1); a no-attr element with
    // children objectifies as `['name', null, …]`.
    let js = emit_tree("<script>let { y } = $props();</script>\n<div>a</div><p>b {y}</p>\n");
    assert!(
        js.contains("var root = $.from_tree([['div', null, 'a'], ['p', null, ' ']], 1);"),
        "multi-root tree carries the fragment flag + null attrs slot:\n{js}"
    );
}

#[test]
fn fragments_tree_svg_root_fails_closed() {
    // svg / mathml element emission is deferred in EVERY fragments mode: an `<svg>` root
    // fails closed under `fragments: 'tree'` exactly as under the default html fragments
    // (the element allowlist gate, not the factory family, is what refuses it). There is
    // no `TEMPLATE_USE_SVG` tree flag to carry.
    let opts = SvelteRuntimeOptions {
        fragments: Some(SvelteFragments::Tree),
        ..base_opts()
    };
    assert!(
        matches!(
            compile_err("<svg><circle cx=\"1\"/></svg>\n", &opts),
            ClientCompileError::Unsupported(_)
        ),
        "an <svg> root fails closed in tree mode too"
    );
}

#[test]
fn fragments_tree_mathml_root_fails_closed() {
    let opts = SvelteRuntimeOptions {
        fragments: Some(SvelteFragments::Tree),
        ..base_opts()
    };
    assert!(
        matches!(
            compile_err("<math><mi>x</mi></math>\n", &opts),
            ClientCompileError::Unsupported(_)
        ),
        "a <math> root fails closed in tree mode too"
    );
}

#[test]
fn fragments_tree_childless_and_valueless_attrs() {
    // A childless no-attr element is a bare `['name']`; a childless with-attrs element is
    // `['name', { … }]`; a valueless boolean attribute collects the empty string.
    let js = emit_tree("<input type=\"text\" disabled/>\n");
    assert!(
        js.contains("var root = $.from_tree([['input', { type: 'text', disabled: '' }]]);"),
        "childless with-attrs objectifies name + attrs, valueless → '':\n{js}"
    );
    // A childless no-attr element is a bare `['name']`. Uses a SUPPORTED element
    // (`<span>`) so the objectified plan structure matches what real compilation emits
    // (the finite element allowlist refuses an unsupported tag, so its plan is never
    // produced).
    assert_eq!(
        tree_literal("<span></span>\n"),
        "[['span']]",
        "a childless no-attr element is a bare ['name']"
    );
}

#[test]
fn fragments_tree_bakes_the_scope_class_into_the_attrs_object() {
    // The css scope class bakes into the attrs OBJECT exactly as it bakes into the
    // html-string class (the SAME `collect_static_attrs` selection).
    let js = emit_tree("<div class=\"a b\">t</div>\n<style>.a{color:red}</style>\n");
    assert!(
        js.contains("['div', { class: 'a b svelte-") && js.contains("' }, 't']"),
        "the scope class appends into the tree attrs object:\n{js}"
    );
}

#[test]
fn fragments_tree_block_anchor_is_a_sparse_hole() {
    // A control-flow `<!>` anchor is a sparse-array HOLE (`objectify` returns `null`);
    // a sole controlled `{@html}` / `{#each}` child leaves the element childless.
    let js = emit_tree("<script>let { x } = $props();</script>\n<div>a{#if x}b{/if}c</div>\n");
    assert!(
        js.contains("var root = $.from_tree([['div', null, 'a',, 'c']]);"),
        "the block anchor is an elided (hole) array element:\n{js}"
    );
}

#[test]
fn fragments_tree_rendered_anchor_first_root_unshifts_the_effect_start_hole() {
    // When the FIRST region root lowers to a rendered `<!>` comment anchor — a
    // block (`{#if}`) / component / `{@render}` / `{@html}` first-root — `as_tree`
    // UNSHIFTS an extra `null` hole for `effect.start` (mirroring
    // `Template.as_tree`'s `nodes[0].type === 'comment'` unshift): svelte@5.56.3 emits
    // TWO leading holes (`[,, ['p', …]]`), not one. A first-root that lowers to a
    // rendered comment anchor (ANY block / component / `{@render}` / `{@html}`, not
    // only an authored `IrNode::Comment`) contributes its OWN leading hole, so the
    // anchor's hole is never collapsed into the effect-start slot (`[, ['p', …]]`).
    let js = emit_tree("<script>let { x } = $props();</script>\n{#if x}a{/if}<p>b</p>\n");
    assert!(
        js.contains("var root = $.from_tree([,, ['p', null, 'b']], 1);"),
        "a block first-root unshifts the effect-start hole (double leading hole):\n{js}"
    );
    // NEGATIVE: it is NOT the single-hole form (the effect-start hole missing).
    assert!(
        !js.contains("$.from_tree([, ['p'"),
        "the effect-start hole must not collapse into a single leading hole:\n{js}"
    );
    // CONTROL: an ELEMENT first-root does NOT unshift (svelte `nodes[0].type ===
    // 'element'`), so the two element roots stay flush with no leading hole.
    let ctrl = emit_tree("<p>b</p><span>c</span>\n");
    assert!(
        ctrl.contains("var root = $.from_tree([['p', null, 'b'], ['span', null, 'c']], 1);"),
        "an element first-root carries no effect-start hole:\n{ctrl}"
    );
}
