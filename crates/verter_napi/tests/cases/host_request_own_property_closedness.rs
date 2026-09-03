//! A key a caller STATES is the caller's key, whatever value it carries.
//!
//! The native request crosses the boundary through
//! [`verter_napi::materialize_js_value`], and this suite drives that
//! materialiser over a JS object graph — the one representation that can
//! express a property whose value is `undefined`, which the decoded
//! `serde_json::Value` cannot. Fixtures phrased as JSON alone cannot see
//! this class at all: they can only omit a key or give it a value.
//!
//! The rule under test, in both directions: an unknown or cross-framework
//! key stated as `undefined` is still refused by name, and a known
//! optional slot stated as `undefined` still decodes exactly as the
//! payload that omits it. Without the second half a materialiser that
//! refused every `undefined` would pass the first half.
//!
//! A modelled graph is only as good as what its author thought to model,
//! so it is not the boundary's acceptance evidence: `real_js_host_request_boundary`
//! drives the same rules through a Node addon over live V8 objects. What
//! a model buys is reach — a declared length or a self-reference can be
//! posed here without a running JS engine, and the traversal rules can be
//! exercised value by value.

use napi::{Result, Status};
use serde_json::{json, Number, Value};

use verter_napi::{
    decode_host_compile_request, materialize_js_value, materialize_js_value_with_budget,
    napi_host_compile_request_to_ffi, JsValueClass, JsValueGraph, JsValueMaterializationBudget,
    NapiHostCompileRequest, MAX_ARRAY_ELEMENTS, MAX_NESTING_DEPTH,
};

// ── a JS object graph, `undefined` included ──────────────────────────────

/// One JS value as a caller writes it.
///
/// `Undefined` is the whole reason this type exists: it is a stated
/// property value that `serde_json::Value` has no way to spell, so a
/// fixture built as JSON cannot pose the question this suite asks.
#[derive(Clone, Debug, PartialEq)]
enum Js {
    Undefined,
    Null,
    Bool(bool),
    Num(Number),
    Str(String),
    Array(Vec<Js>),
    /// Own enumerable properties, in insertion order.
    Object(Vec<(String, Js)>),
    /// An array that DECLARES a length without holding any element —
    /// `new Array(2 ** 32 - 1)`, whose length costs nothing and whose
    /// every read answers `undefined`.
    DeclaredArray(u32),
    /// An object whose one own property, `self`, is the object itself —
    /// `const a = {}; a.self = a`. A tree cannot hold a real cycle, so
    /// every read answers the same node, which is what a cyclic graph
    /// presents to a materialiser.
    SelfReferential,
}

/// The graph a JS payload with no `undefined` anywhere describes.
fn js(value: &Value) -> Js {
    match value {
        Value::Null => Js::Null,
        Value::Bool(flag) => Js::Bool(*flag),
        Value::Number(number) => Js::Num(number.clone()),
        Value::String(text) => Js::Str(text.clone()),
        Value::Array(items) => Js::Array(items.iter().map(js).collect()),
        Value::Object(entries) => {
            Js::Object(entries.iter().map(|(k, v)| (k.clone(), js(v))).collect())
        }
    }
}

/// The value at `path`, where a numeric step indexes an array.
fn at<'a>(root: &'a mut Js, path: &[&str]) -> &'a mut Js {
    let mut here = root;
    for step in path {
        here = match here {
            Js::Object(entries) => {
                &mut entries
                    .iter_mut()
                    .find(|(key, _)| key == step)
                    .unwrap_or_else(|| panic!("fixture has no `{step}`"))
                    .1
            }
            Js::Array(items) => {
                let index: usize = step.parse().expect("an array step is an index");
                items.get_mut(index).expect("fixture has that element")
            }
            other => panic!("`{step}` has no container to live in: {other:?}"),
        };
    }
    here
}

/// Assigns `object[key] = value`, replacing the slot if it already exists
/// and appending it otherwise — what a JS assignment does.
fn set(object: &mut Js, key: &str, value: Js) {
    let Js::Object(entries) = object else {
        panic!("only an object carries properties, got {object:?}");
    };
    match entries.iter_mut().find(|(existing, _)| existing == key) {
        Some(slot) => slot.1 = value,
        None => entries.push((key.to_string(), value)),
    }
}

/// States `key` as `undefined` on the object at `path`.
fn state_undefined(root: &mut Js, path: &[&str], key: &str) {
    set(at(root, path), key, Js::Undefined);
}

/// Removes `key` from the object at `path` — the payload a caller writes
/// when they simply do not mention the slot.
fn omit(root: &mut Js, path: &[&str], key: &str) {
    let object = at(root, path);
    let Js::Object(entries) = object else {
        panic!("only an object carries properties, got {object:?}");
    };
    entries.retain(|(existing, _)| existing != key);
}

/// Reads the modelled graph the way the native boundary reads a live one.
struct Graph;

impl JsValueGraph for Graph {
    type Value = Js;

    fn classify(&self, value: &Js) -> Result<JsValueClass> {
        Ok(match value {
            Js::Undefined => JsValueClass::Undefined,
            Js::Array(_) | Js::DeclaredArray(_) => JsValueClass::Array,
            Js::Object(_) | Js::SelfReferential => JsValueClass::Object,
            Js::Null | Js::Bool(_) | Js::Num(_) | Js::Str(_) => JsValueClass::Leaf,
        })
    }

    fn own_enumerable_keys(&self, object: &Js) -> Result<Vec<String>> {
        match object {
            Js::Object(entries) => Ok(entries.iter().map(|(key, _)| key.clone()).collect()),
            Js::SelfReferential => Ok(vec!["self".to_string()]),
            other => panic!("only an object is enumerated, got {other:?}"),
        }
    }

    fn property(&self, object: &Js, key: &str) -> Result<Js> {
        match object {
            // Reading a property an object does not have answers
            // `undefined`, exactly as JS property access does — which is
            // why the key list, not the lookup, decides what the payload
            // contains.
            Js::Object(entries) => Ok(entries
                .iter()
                .find(|(existing, _)| existing == key)
                .map(|(_, value)| value.clone())
                .unwrap_or(Js::Undefined)),
            Js::SelfReferential => Ok(Js::SelfReferential),
            other => panic!("only an object carries properties, got {other:?}"),
        }
    }

    fn element_count(&self, array: &Js) -> Result<u32> {
        match array {
            Js::Array(items) => Ok(items.len() as u32),
            Js::DeclaredArray(length) => Ok(*length),
            other => panic!("only an array has a length, got {other:?}"),
        }
    }

    fn element(&self, array: &Js, index: u32) -> Result<Js> {
        match array {
            // As with a property: an element an array does not hold
            // answers `undefined`.
            Js::Array(items) => Ok(items.get(index as usize).cloned().unwrap_or(Js::Undefined)),
            Js::DeclaredArray(_) => Ok(Js::Undefined),
            other => panic!("only an array has elements, got {other:?}"),
        }
    }

    fn leaf(&self, value: &Js) -> Result<Value> {
        Ok(match value {
            Js::Null => Value::Null,
            Js::Bool(flag) => Value::Bool(*flag),
            Js::Num(number) => Value::Number(number.clone()),
            Js::Str(text) => Value::String(text.clone()),
            other => panic!("not a leaf: {other:?}"),
        })
    }

    fn leaf_retained_bytes(&self, value: &Js) -> Result<usize> {
        Ok(match value {
            Js::Str(text) => text.len(),
            _ => 0,
        })
    }
}

// ── fixtures ─────────────────────────────────────────────────────────────

fn identity_json() -> Value {
    json!({
        "filename": "Comp.vue",
        "componentId": "c-1",
        "isProduction": false,
        "forceJs": false,
    })
}

fn vue_options_json() -> Value {
    json!({
        "backend": "inferred",
        "ssr": false,
        "isCustomElement": [],
        "babelParserPlugins": [],
    })
}

fn analysis_product_json() -> Value {
    json!({ "kind": "analysis", "wantScriptBindings": true, "wantTemplateData": true })
}

fn vue_request_json() -> Value {
    json!({
        "framework": "vue",
        "identity": identity_json(),
        "products": [analysis_product_json()],
        "options": vue_options_json(),
    })
}

fn svelte_request_json() -> Value {
    json!({
        "framework": "svelte",
        "identity": identity_json(),
        "products": [analysis_product_json()],
        "options": { "dev": true },
    })
}

fn vue_request() -> Js {
    js(&vue_request_json())
}

fn svelte_request() -> Js {
    js(&svelte_request_json())
}

fn materialize(value: &Js) -> Value {
    materialize_js_value(&Graph, value).expect("the fixture graph materialises")
}

fn decode(value: &Js) -> NapiHostCompileRequest {
    decode_host_compile_request(materialize(value)).expect("the fixture decodes")
}

/// The refusal message for a graph materialisation declines.
fn materialisation_refusal(value: &Js) -> String {
    let error = materialize_js_value(&Graph, value).expect_err("the graph is refused");
    assert_eq!(
        error.status,
        Status::InvalidArg,
        "a graph with no JSON representation must be refused as an invalid argument"
    );
    error.reason.clone()
}

/// The refusal message for a payload the boundary declines.
fn refusal(value: &Js) -> String {
    let error =
        decode_host_compile_request(materialize(value)).expect_err("the fixture is refused");
    assert_eq!(
        error.status,
        Status::InvalidArg,
        "a malformed request must be refused as an invalid argument"
    );
    error.reason.clone()
}

// ── the materialisation rule ─────────────────────────────────────────────

#[test]
fn a_property_stated_as_undefined_survives_materialisation_as_null() {
    let mut request = vue_request();
    state_undefined(&mut request, &["options"], "runes");

    let materialized = materialize(&request);
    let options = materialized["options"]
        .as_object()
        .expect("the options section materialises as an object");
    // `get`, not indexing: indexing answers `null` for a key that is not
    // there, which is the very outcome this asserts against.
    assert_eq!(
        options.get("runes"),
        Some(&Value::Null),
        "a key the caller stated must reach the schema whatever it carries; \
         dropping it is what lets a closed schema accept a foreign key"
    );
}

#[test]
fn a_payload_with_no_undefined_materialises_to_the_json_the_caller_wrote() {
    // Every JS type the request schema uses, plus the nesting it uses
    // them in: objects, arrays, strings, numbers, booleans and `null`.
    let wire = json!({
        "text": "value",
        "flag": true,
        "off": false,
        "empty": Value::Null,
        "whole": 7,
        "fractional": 2.5,
        "list": ["a", 1, false, Value::Null, { "nested": [] }],
        "object": { "inner": { "deep": "leaf" } },
    });
    assert_eq!(
        materialize(&js(&wire)),
        wire,
        "materialisation must preserve value shape, not merely key presence"
    );

    assert_eq!(
        decode(&vue_request()),
        decode_host_compile_request(vue_request_json()).expect("the JSON wire decodes"),
        "a payload with no `undefined` must decode exactly as its JSON wire does"
    );
}

// ── refusals: an unknown key stated as `undefined` ───────────────────────

#[test]
fn an_unknown_top_level_key_stated_as_undefined_is_refused() {
    let mut request = vue_request();
    state_undefined(&mut request, &[], "bogus");

    let message = refusal(&request);
    assert!(
        message.contains("unknown field `bogus`"),
        "expected an unknown-field refusal naming `bogus`, got: {message}"
    );
}

#[test]
fn an_unknown_key_stated_as_undefined_is_refused_at_every_nesting_level() {
    // The identity, the framework options, a requested product, and an
    // option object nested inside the options: each is its own closed
    // type, so a level that stopped enumerating own keys would pass a
    // top-level-only check.
    let rows: Vec<(&[&str], &str)> = vec![
        (&["identity"], "sourceMap"),
        (&["options"], "hoistStatick"),
        (&["products", "0"], "bogus"),
    ];
    for (path, key) in rows {
        let mut request = vue_request();
        state_undefined(&mut request, path, key);
        let message = refusal(&request);
        assert!(
            message.contains(&format!("unknown field `{key}`")),
            "expected an unknown-field refusal naming `{key}` at {path:?}, got: {message}"
        );
    }

    let mut request = vue_request();
    set(
        at(&mut request, &["options"]),
        "cssModules",
        js(&json!({ "scopeBehaviour": "local" })),
    );
    state_undefined(&mut request, &["options", "cssModules"], "bogus");
    let message = refusal(&request);
    assert!(
        message.contains("unknown field `bogus`"),
        "expected a nested-option refusal naming `bogus`, got: {message}"
    );
}

#[test]
fn a_cross_framework_option_stated_as_undefined_is_refused_in_both_arms() {
    let mut vue = vue_request();
    state_undefined(&mut vue, &["options"], "runes");
    let message = refusal(&vue);
    assert!(
        message.contains("unknown field `runes`"),
        "expected a cross-framework refusal naming `runes`, got: {message}"
    );

    let mut svelte = svelte_request();
    state_undefined(&mut svelte, &["options"], "backend");
    let message = refusal(&svelte);
    assert!(
        message.contains("unknown field `backend`"),
        "expected a cross-framework refusal naming `backend`, got: {message}"
    );
}

#[test]
fn a_required_slot_stated_as_undefined_is_refused_rather_than_defaulted() {
    // A stated `undefined` reads as a supplied `null`, so a required slot
    // is refused on the value rather than as a missing field. Both are
    // refusals, and this is the refusal the browser binding reports for
    // the same payload — the two bindings converge on the `undefined`
    // class, which is the claim, and not on every JS value a caller could
    // hand either of them.
    let mut request = vue_request();
    state_undefined(&mut request, &["options"], "ssr");
    let message = refusal(&request);
    assert!(
        message.contains("invalid type: null") && message.contains("expected a boolean"),
        "expected a null-value refusal for a required boolean slot, got: {message}"
    );
}

#[test]
fn a_wrong_kind_of_value_is_refused_the_same_way_whatever_produced_it() {
    // Which refusals carry a field name is serde's division, and it is
    // not uniform: an unknown field, an unknown tag and a missing field
    // are named; a slot given the wrong KIND of value is not, because the
    // outermost framework tag buffers the payload before the variant is
    // deserialised and no deserializer-side path tracker survives that.
    //
    // A stated `undefined` therefore lands in the pre-existing unnamed
    // class rather than opening a new one, and this pins that: a slot
    // stated as `undefined` and a slot given a number are refused with
    // the same shape of message.
    let mut stated_undefined = vue_request();
    state_undefined(&mut stated_undefined, &["options"], "ssr");

    let mut wrong_type = vue_request();
    set(at(&mut wrong_type, &["options"]), "ssr", Js::Num(5.into()));

    for message in [refusal(&stated_undefined), refusal(&wrong_type)] {
        assert!(
            message.starts_with("invalid type: ") && message.contains("expected a boolean"),
            "expected a wrong-kind refusal for a required boolean slot, got: {message}"
        );
    }

    // Every other refusal class does name its field, so the schema stays
    // diagnosable outside that one class.
    let mut unknown = vue_request();
    state_undefined(&mut unknown, &["options"], "runes");
    assert!(refusal(&unknown).contains("`runes`"));

    let mut missing = vue_request();
    omit(&mut missing, &["options"], "ssr");
    assert!(refusal(&missing).contains("missing field `ssr`"));

    let mut unknown_tag = vue_request();
    set(&mut unknown_tag, "framework", Js::Str("react".to_string()));
    assert!(refusal(&unknown_tag).contains("`react`"));
}

// ── refusals: graphs with no JSON representation ─────────────────────────

#[test]
fn an_array_declaring_more_elements_than_a_request_may_carry_is_refused_before_it_is_reserved() {
    // A declared length costs nothing in V8, so an unbounded materialiser
    // turns a cheap argument into a tens-of-gigabytes reservation and
    // aborts the process on the allocation failure — no JS exception, no
    // stack, and it happens before any schema runs.
    let mut request = vue_request();
    set(
        at(&mut request, &["options"]),
        "isCustomElement",
        Js::DeclaredArray(u32::MAX),
    );

    let message = materialisation_refusal(&request);
    assert!(
        message.contains(&u32::MAX.to_string())
            && message.contains(&MAX_ARRAY_ELEMENTS.to_string()),
        "expected a refusal naming the declared length and the limit, got: {message}"
    );

    // The bound is on size, not on declared arrays: one below the ceiling
    // still materialises, element for element.
    assert_eq!(
        materialize(&Js::DeclaredArray(3)),
        json!([Value::Null, Value::Null, Value::Null]),
        "an array within the bound must materialise its declared elements"
    );
}

#[test]
fn a_graph_that_refers_back_to_itself_is_refused_rather_than_followed() {
    // `const a = {}; a.self = a` is a payload a caller produces by
    // accident, and it has no JSON representation at all. Following it
    // runs until the stack guard page kills the process.
    let mut request = vue_request();
    set(
        at(&mut request, &["options"]),
        "cssModules",
        Js::SelfReferential,
    );

    let message = materialisation_refusal(&request);
    assert!(
        message.contains("refers back to itself"),
        "expected a refusal naming the self-reference, got: {message}"
    );

    // The bound is on runaway nesting, not on nesting: a graph nested
    // well past anything the schema uses, but finite and within the
    // budget, still materialises.
    let mut nested = Js::Str("leaf".to_string());
    for _ in 0..MAX_NESTING_DEPTH {
        nested = Js::Object(vec![("inner".to_string(), nested)]);
    }
    materialize_js_value(&Graph, &nested).expect("a graph within the depth budget materialises");
}

// @ai-generated - A batch-wide budget must count repeated shared payloads, not only each graph.
#[test]
fn one_materialization_budget_bounds_values_and_bytes_across_payloads() {
    let mut value_budget = JsValueMaterializationBudget::new(3, usize::MAX);
    materialize_js_value_with_budget(
        &Graph,
        &Js::Array(vec![Js::Null, Js::Null]),
        &mut value_budget,
    )
    .expect("the first payload fits the shared value budget");
    let error =
        materialize_js_value_with_budget(&Graph, &Js::Array(vec![Js::Null]), &mut value_budget)
            .expect_err("the second payload exceeds the shared value budget");
    assert!(
        error.reason.contains("3 decoded values"),
        "{}",
        error.reason
    );

    let mut byte_budget = JsValueMaterializationBudget::new(usize::MAX, 5);
    materialize_js_value_with_budget(&Graph, &Js::Str("abc".to_string()), &mut byte_budget)
        .expect("the first payload fits the shared byte budget");
    let error =
        materialize_js_value_with_budget(&Graph, &Js::Str("def".to_string()), &mut byte_budget)
            .expect_err("the second payload exceeds the shared byte budget");
    assert!(error.reason.contains("5 bytes"), "{}", error.reason);
}

// ── the control: a known optional slot stated as `undefined` ─────────────

#[test]
fn a_known_optional_slot_stated_as_undefined_decodes_as_the_omitting_payload_does() {
    // One optional slot per closed type the request reaches: the
    // identity, the framework options, and a requested product. Without
    // this leg a materialiser that refused every `undefined`, or that
    // turned an optional slot into a supplied value, would still pass
    // every refusal above.
    let base = js(&json!({
        "framework": "vue",
        "identity": identity_json(),
        "products": [{ "kind": "runtimeClient", "inline": true, "runtimeSourceMap": true }],
        "options": vue_options_json(),
    }));

    let rows: Vec<(&[&str], &str)> = vec![
        (&["identity"], "filename"),
        // Absent from the base payload, so this row also proves a slot
        // stated as `undefined` and a slot never written are one request.
        (&["options"], "hmr"),
        (&["products", "0"], "inline"),
    ];

    for (path, key) in rows {
        let mut stated = base.clone();
        state_undefined(&mut stated, path, key);

        let mut omitted = base.clone();
        omit(&mut omitted, path, key);

        assert_eq!(
            decode(&stated),
            decode(&omitted),
            "`{key}` stated as `undefined` must decode exactly as the payload \
             that omits it"
        );
        assert_eq!(
            napi_host_compile_request_to_ffi(decode(&stated)),
            napi_host_compile_request_to_ffi(decode(&omitted)),
            "`{key}` stated as `undefined` must compile exactly as the payload \
             that omits it"
        );
    }
}
