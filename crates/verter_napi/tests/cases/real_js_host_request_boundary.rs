//! The host compile request driven from real JS values.
//!
//! The rule under repair lives in `FromNapiValue`, and its input is a live
//! V8 object graph. Three of the distinctions that decide the outcome
//! cannot be modelled in Rust at all: a property STATED as `undefined`
//! versus one that is absent, a key that lives on the prototype rather
//! than on the object, and an array whose length is DECLARED rather than
//! held. A `serde_json::Value` can express none of them, and a Rust model
//! of a JS graph only expresses what its author thought to model.
//!
//! So this suite drives the real boundary: a test-only Node addon whose
//! single entry takes a `NapiHostCompileRequest`, and a JS program that
//! hands it objects a caller could actually write.
//!
//! Discrimination contract: restoring the generic materialisation in
//! `NapiHostCompileRequest::from_napi_value` — reading the payload with
//! `<serde_json::Value as FromNapiValue>::from_napi_value` — turns this
//! suite red. That conversion drops an `undefined`-valued key before the
//! schema sees it, so every cross-framework and unknown-key case here is
//! silently accepted; and it reads properties through
//! `napi_get_property_names`, which walks the prototype chain, so the
//! inherited-key case is refused instead of accepted.
//!
//! # Where the addon comes from
//!
//! This suite builds nothing. The addon is its own `cdylib` package that
//! `verter_napi` dev-depends on, so cargo links it while it builds this
//! binary and leaves it beside it in the same `deps` directory;
//! `.config/nextest.toml` carries that one file into a nextest archive at
//! the same relative path, so an archived run finds it beside the binary
//! too. Locating it is therefore reading a sibling path, and a missing
//! addon is a loud failure rather than a rebuild.
//!
//! Building it here instead would mean spawning a `cargo build` that
//! links the native crate's entire dependency closure. Against a cold
//! target directory — which is what an archived run extracted into a
//! fresh tree has — that costs minutes, and minutes inside a workspace
//! run is indistinguishable from a hang: the run terminates the test and
//! cancels every case it had not yet scheduled. Build cost belongs in the
//! build, not in a test body.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Every case the JS program is expected to report on.
///
/// Named here as well as there so a program that dies early, or that
/// quietly stops running a case, fails instead of passing on the cases it
/// did reach.
const EXPECTED_CASES: &[&str] = &[
    "valid-request-accepted",
    "unknown-top-level-key-stated-as-undefined-refused",
    "unknown-option-key-stated-as-undefined-refused",
    "unknown-product-key-stated-as-undefined-refused",
    "vue-cross-framework-option-stated-as-undefined-refused",
    "svelte-cross-framework-option-stated-as-undefined-refused",
    "required-option-stated-as-undefined-refused",
    "optional-slot-stated-as-undefined-compiles-as-omitted",
    "prototype-inherited-key-ignored",
    "array-declaring-more-elements-than-a-request-may-carry-refused",
    "self-referential-graph-refused",
];

/// The fixture addon's shared library, as its platform names it.
#[cfg(target_os = "windows")]
const FIXTURE_LIBRARY: &str = "verter_napi_request_fixture.dll";
#[cfg(target_os = "macos")]
const FIXTURE_LIBRARY: &str = "libverter_napi_request_fixture.dylib";
#[cfg(all(unix, not(target_os = "macos")))]
const FIXTURE_LIBRARY: &str = "libverter_napi_request_fixture.so";

/// Answers the fixture addon built beside this test binary.
///
/// An absent addon fails here, loudly and by name. It is never rebuilt:
/// this suite is the only evidence that discriminates the repair, and a
/// suite that can quietly produce its own input can also quietly produce
/// the wrong one.
fn fixture_addon_library() -> PathBuf {
    let exe = std::env::current_exe().expect("a running test has a path");
    let library = exe
        .parent()
        .expect("a test binary sits in a directory")
        .join(FIXTURE_LIBRARY);
    assert!(
        library.exists(),
        "the fixture addon must sit beside this test binary, at {}. \
         `verter_napi` dev-depends on it so cargo builds it with this \
         binary; a run from a nextest archive additionally needs the \
         `archive.include` entry naming it in `.config/nextest.toml`.",
        library.display()
    );
    library
}

/// The JS program driving the boundary.
///
/// Every case reports one `PASS <name>` or `FAIL <name>: <detail>` line as
/// it finishes, so a case that kills the process is visible as a missing
/// line rather than as a silent pass.
const DRIVER: &str = r#"
const addon = require(process.argv[2]);
const decode = addon.decodeHostCompileRequest;

function report(name, failure) {
  console.log(failure === null ? `PASS ${name}` : `FAIL ${name}: ${failure}`);
}

/** The case body answers null when it holds, or why it did not. */
function check(name, body) {
  let failure;
  try {
    failure = body();
  } catch (error) {
    failure = `threw unexpectedly: ${error && error.message}`;
  }
  report(name, failure === undefined ? null : failure);
}

/** Answers the refusal message, or null when the payload was accepted. */
function refusalOf(request) {
  try {
    decode(request);
    return null;
  } catch (error) {
    return String((error && error.message) || error);
  }
}

function refuses(request, expected) {
  const message = refusalOf(request);
  if (message === null) return `accepted a payload that must be refused`;
  if (!message.includes(expected)) return `refused with ${message}, expected it to name ${expected}`;
  return undefined;
}

const identity = () => ({
  filename: "Comp.vue",
  componentId: "c-1",
  isProduction: false,
  forceJs: false,
});
const vueOptions = () => ({
  backend: "inferred",
  ssr: false,
  isCustomElement: [],
  babelParserPlugins: [],
});
const products = () => [
  { kind: "analysis", wantScriptBindings: true, wantTemplateData: true },
];
const vue = () => ({ framework: "vue", identity: identity(), products: products(), options: vueOptions() });
const svelte = () => ({ framework: "svelte", identity: identity(), products: products(), options: { dev: true } });

check("valid-request-accepted", () => {
  const rendered = decode(vue());
  if (typeof rendered !== "string" || !rendered.includes("Vue")) {
    return `a valid Vue request rendered as ${rendered}`;
  }
  return undefined;
});

check("unknown-top-level-key-stated-as-undefined-refused", () => {
  const request = vue();
  request.bogus = undefined;
  return refuses(request, "unknown field `bogus`");
});

check("unknown-option-key-stated-as-undefined-refused", () => {
  const request = vue();
  request.options.hoistStatick = undefined;
  return refuses(request, "unknown field `hoistStatick`");
});

check("unknown-product-key-stated-as-undefined-refused", () => {
  const request = vue();
  request.products[0].bogus = undefined;
  return refuses(request, "unknown field `bogus`");
});

check("vue-cross-framework-option-stated-as-undefined-refused", () => {
  const request = vue();
  request.options.runes = undefined;
  return refuses(request, "unknown field `runes`");
});

check("svelte-cross-framework-option-stated-as-undefined-refused", () => {
  const request = svelte();
  request.options.backend = undefined;
  return refuses(request, "unknown field `backend`");
});

check("required-option-stated-as-undefined-refused", () => {
  const request = vue();
  request.options.ssr = undefined;
  return refuses(request, "invalid type: null, expected a boolean");
});

check("optional-slot-stated-as-undefined-compiles-as-omitted", () => {
  const stated = vue();
  stated.options.hmr = undefined;
  const omitted = vue();
  // A slot the base payload never mentions, so this also proves a slot
  // stated as `undefined` and a slot never written are one request.
  if (Object.prototype.hasOwnProperty.call(omitted.options, "hmr")) {
    return "the base payload was expected not to mention `hmr`";
  }
  const withStated = decode(stated);
  const withOmitted = decode(omitted);
  if (withStated !== withOmitted) {
    return `stated ${withStated} but omitted ${withOmitted}`;
  }
  return undefined;
});

check("prototype-inherited-key-ignored", () => {
  // A payload is its own properties. `runes` is a Svelte option and would
  // be refused as an own key of a Vue request; inherited, it is not part
  // of what the caller wrote.
  const request = vue();
  request.options = Object.assign(Object.create({ runes: true }), vueOptions());
  if (request.options.runes !== true) {
    return "the fixture failed to put `runes` on the prototype";
  }
  if (Object.prototype.hasOwnProperty.call(request.options, "runes")) {
    return "the fixture put `runes` on the object rather than the prototype";
  }
  const inherited = decode(request);
  const plain = decode(vue());
  if (inherited !== plain) {
    return `an inherited key changed the request: ${inherited} vs ${plain}`;
  }
  return undefined;
});

check("array-declaring-more-elements-than-a-request-may-carry-refused", () => {
  // A declared length costs nothing in V8; materialising it unbounded
  // would reserve tens of gigabytes and abort the process.
  const request = vue();
  request.options.isCustomElement = new Array(4294967295);
  return refuses(request, "declares 4294967295 elements");
});

check("self-referential-graph-refused", () => {
  const cyclic = {};
  cyclic.self = cyclic;
  const request = vue();
  request.options.cssModules = cyclic;
  return refuses(request, "refers back to itself");
});
"#;

#[test]
fn the_host_request_boundary_holds_for_real_js_values() {
    let library = fixture_addon_library();

    let scratch = Path::new(env!("CARGO_TARGET_TMPDIR")).join("napi_request_fixture");
    std::fs::create_dir_all(&scratch).expect("the scratch directory is creatable");

    // Node loads an addon by extension, not by content.
    let addon = scratch.join("verter_napi_request_fixture.node");
    std::fs::copy(&library, &addon).expect("the built addon is copyable");
    let driver = scratch.join("drive.cjs");
    std::fs::write(&driver, DRIVER).expect("the driver is writable");

    let run = Command::new("node")
        .arg(&driver)
        .arg(&addon)
        .output()
        .expect("node is on PATH");

    let stdout = String::from_utf8_lossy(&run.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&run.stderr).into_owned();
    let transcript = format!("stdout:\n{stdout}\nstderr:\n{stderr}");

    assert!(
        run.status.success(),
        "the JS driver must run to completion; {transcript}"
    );

    for case in EXPECTED_CASES {
        assert!(
            stdout.contains(&format!("PASS {case}")),
            "`{case}` did not pass at the real JS boundary; {transcript}"
        );
    }
    assert!(
        !stdout.contains("FAIL "),
        "every case must pass; {transcript}"
    );
}
