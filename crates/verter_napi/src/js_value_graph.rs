//! Materialising a JS value into `serde_json::Value` with every own
//! enumerable string key preserved, whatever that key carries.
//!
//! ## Why the generic conversion is not enough
//!
//! napi-rs reads an object's properties with `Object::get`, which reports
//! a property whose value is `undefined` as absent. A key a caller states
//! as `undefined` therefore never reaches the schema, and a closed schema
//! cannot refuse a key it never sees: `{ framework: "vue", …, runes:
//! undefined }` would decode clean instead of refusing the cross-framework
//! key.
//!
//! Here the key list is the sole authority on which properties exist and
//! the value is whatever the object holds, `undefined` included. An own
//! enumerable key survives materialisation regardless of its value, and
//! `undefined` is represented as JSON `null`.
//!
//! Two consequences, both intended:
//!
//! - a known optional slot stated as `undefined` still decodes as absent,
//!   because `Option<T>` reads `null` as `None`;
//! - an unknown or cross-framework key stated as `undefined` is present,
//!   so `deny_unknown_fields` refuses it and names it.
//!
//! A REQUIRED slot stated as `undefined` is consequently refused as a
//! `null` value rather than as a missing field. That is the same refusal
//! the browser binding reports for the same payload, which materialises
//! every own key before its schema runs; the two bindings agreeing on the
//! `undefined` class is the point. The claim is scoped to that class: JS
//! values the request schema cannot carry — a `Map`, a typed array, a
//! bigint outside the JSON number range — still reach the two schemas by
//! different routes, and nothing here converges them.
//!
//! ## Own keys only
//!
//! [`JsValueGraph::own_enumerable_keys`] answers OWN enumerable string
//! keys and ignores the prototype chain. That is deliberate and it is a
//! narrowing: the replaced generic conversion read an object's properties
//! through `napi_get_property_names`, which Node defines as walking the
//! prototype chain, so an inherited enumerable key used to reach the
//! schema and be refused. `Object.create({ runes: true })` carrying an
//! otherwise valid Vue body was refused before and is accepted now.
//!
//! The narrowing is the ratified convergence with the browser binding,
//! whose `Object.entries` is own-keys-only, and with what a caller
//! actually wrote. A payload is its own properties.
//!
//! ## Refusals this layer owns
//!
//! Two, both structural rather than about vocabulary, because a JS graph
//! can describe things JSON has no term for:
//!
//! - a value nested past [`MAX_NESTING_DEPTH`], which is also how a graph
//!   that refers back to itself is refused — `const a = {}; a.self = a`
//!   has no JSON representation at all, and without a bound the traversal
//!   would run until the stack guard page killed the process;
//! - an array declaring more than [`MAX_ARRAY_ELEMENTS`] elements. A
//!   declared length is free in V8 — `new Array(2 ** 32 - 1)` allocates
//!   nothing — so an unbounded materialiser would turn a cheap argument
//!   into a tens-of-gigabytes reservation and abort the process on the
//!   allocation failure. The count is checked before anything is
//!   reserved.
//!
//! Object keys carry no equivalent bound: the key list Node hands back is
//! a real array of the keys that exist, so its size is already paid for by
//! the caller's own object and cannot be inflated by a declaration.
//!
//! ## What this layer does not own
//!
//! No key vocabulary lives here. This layer decides only what a JS value
//! IS; which keys are legal, and which values they may carry, stays the
//! schema's alone.

use std::ptr;

use napi::bindgen_prelude::FromNapiValue;
use napi::{sys, Error, Result, Status, ValueType};
use serde_json::{Map, Value};

/// How deep a request value may nest.
///
/// The request schema nests a handful of levels — request, options, an
/// option object, an array, its elements — so this is far above anything
/// a caller writes on purpose and far below the depth at which recursion
/// threatens the stack.
pub const MAX_NESTING_DEPTH: u32 = 64;

/// How many elements one request array may declare.
///
/// Every array the schema carries is a short list of names or products.
pub const MAX_ARRAY_ELEMENTS: u32 = 1 << 16;

/// What a JS value is, as far as materialisation needs to know.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsValueClass {
    /// `undefined`, whether as a stated property value, an array element,
    /// or the whole payload.
    Undefined,
    /// An array, materialised element by element in index order.
    Array,
    /// Any other object, materialised key by key.
    Object,
    /// A value with no own-property structure of its own — `null`, a
    /// boolean, a number, a string, a bigint — or one that cannot be
    /// represented at all, which [`JsValueGraph::leaf`] refuses.
    Leaf,
}

/// A JS object graph, read one value at a time.
///
/// [`JsValueGraph::own_enumerable_keys`] and [`JsValueGraph::property`]
/// are separate on purpose: the key list decides which properties exist,
/// so a lookup that answers `undefined` is a stated `undefined` rather
/// than an absence. Folding the two together is exactly the conflation
/// that lets an `undefined`-valued key escape the schema.
///
/// [`JsValueGraph::element_count`] and [`JsValueGraph::element`] are
/// separate for the opposite reason: an array's declared length is not
/// evidence that its elements exist, so the count is something to check
/// before anything is reserved rather than a promise to fulfil.
pub trait JsValueGraph {
    /// One value in the graph.
    type Value;

    /// Which materialisation rule `value` falls under.
    fn classify(&self, value: &Self::Value) -> Result<JsValueClass>;

    /// Every own enumerable string-keyed property of `object`, in JS
    /// enumeration order. Inherited and symbol-keyed properties are not
    /// part of the payload a caller wrote.
    fn own_enumerable_keys(&self, object: &Self::Value) -> Result<Vec<String>>;

    /// Whatever `object[key]` holds, `undefined` included.
    fn property(&self, object: &Self::Value, key: &str) -> Result<Self::Value>;

    /// How many elements `array` DECLARES. A declared element need not
    /// exist; reading one that does not answers `undefined`.
    fn element_count(&self, array: &Self::Value) -> Result<u32>;

    /// Whatever `array[index]` holds, `undefined` included.
    fn element(&self, array: &Self::Value, index: u32) -> Result<Self::Value>;

    /// A [`JsValueClass::Leaf`] value as JSON, or a refusal saying why the
    /// value has no JSON representation.
    fn leaf(&self, value: &Self::Value) -> Result<Value>;
}

/// Materialises `value` and everything reachable from it.
///
/// One traversal of the supplied graph: each value is classified once,
/// each own key is read once, each element is read once. A graph that
/// refers back to itself has no traversal that terminates and no JSON
/// representation either, so it is refused by [`MAX_NESTING_DEPTH`]
/// rather than followed.
pub fn materialize_js_value<G: JsValueGraph>(graph: &G, value: &G::Value) -> Result<Value> {
    materialize_nested(graph, value, 0)
}

/// `materialize_js_value` at a known nesting depth.
fn materialize_nested<G: JsValueGraph>(graph: &G, value: &G::Value, depth: u32) -> Result<Value> {
    if depth > MAX_NESTING_DEPTH {
        return Err(Error::new(
            Status::InvalidArg,
            format!(
                "A request value nests deeper than {MAX_NESTING_DEPTH} levels, \
                 or refers back to itself; neither has a JSON representation"
            ),
        ));
    }

    Ok(match graph.classify(value)? {
        JsValueClass::Undefined => Value::Null,
        JsValueClass::Leaf => graph.leaf(value)?,
        JsValueClass::Array => {
            // Checked before anything is reserved: the length is what a
            // caller declares, not what the array holds.
            let declared = graph.element_count(value)?;
            if declared > MAX_ARRAY_ELEMENTS {
                return Err(Error::new(
                    Status::InvalidArg,
                    format!(
                        "A request array declares {declared} elements, above the \
                         {MAX_ARRAY_ELEMENTS} a request may carry"
                    ),
                ));
            }
            let mut materialized = Vec::with_capacity(declared as usize);
            for index in 0..declared {
                let element = graph.element(value, index)?;
                materialized.push(materialize_nested(graph, &element, depth + 1)?);
            }
            Value::Array(materialized)
        }
        JsValueClass::Object => {
            let mut materialized = Map::new();
            for key in graph.own_enumerable_keys(value)? {
                let property = graph.property(value, &key)?;
                let property = materialize_nested(graph, &property, depth + 1)?;
                materialized.insert(key, property);
            }
            Value::Object(materialized)
        }
    })
}

/// The live JS object graph behind one Node environment.
pub(crate) struct NapiValueGraph {
    env: sys::napi_env,
}

impl NapiValueGraph {
    /// # Safety
    ///
    /// `env` must be a live Node environment for as long as the graph is
    /// used, and every value read through the graph must belong to it —
    /// which is what napi-rs's own argument extraction supplies.
    pub(crate) unsafe fn new(env: sys::napi_env) -> Self {
        Self { env }
    }
}

impl JsValueGraph for NapiValueGraph {
    type Value = sys::napi_value;

    fn classify(&self, value: &Self::Value) -> Result<JsValueClass> {
        match napi::type_of!(self.env, *value)? {
            ValueType::Undefined => Ok(JsValueClass::Undefined),
            ValueType::Object => {
                let mut is_array = false;
                // SAFETY: the environment/value pair is live per the
                // constructor's contract.
                napi::check_status!(
                    unsafe { sys::napi_is_array(self.env, *value, &mut is_array) },
                    "Failed to detect whether a request value is an array"
                )?;
                Ok(if is_array {
                    JsValueClass::Array
                } else {
                    JsValueClass::Object
                })
            }
            _ => Ok(JsValueClass::Leaf),
        }
    }

    fn own_enumerable_keys(&self, object: &Self::Value) -> Result<Vec<String>> {
        let mut names = ptr::null_mut();
        // SAFETY: as `classify`. `own_only` excludes the prototype chain
        // and `skip_symbols` excludes symbol keys, so what comes back is
        // the payload the caller wrote.
        //
        // `own_only` is a deliberate NARROWING, not a restatement of the
        // replaced conversion: napi-rs read properties through
        // `napi_get_property_names`, which Node defines as walking the
        // prototype chain, so an inherited enumerable key used to reach
        // the schema and be refused. `Object.create({ runes: true })`
        // with an otherwise valid Vue body is accepted here and was
        // refused before. Own keys are what the browser binding's
        // `Object.entries` enumerates and what a caller wrote.
        napi::check_status!(
            unsafe {
                sys::napi_get_all_property_names(
                    self.env,
                    *object,
                    sys::KeyCollectionMode::own_only,
                    sys::KeyFilter::enumerable | sys::KeyFilter::skip_symbols,
                    sys::KeyConversion::numbers_to_strings,
                    &mut names,
                )
            },
            "Failed to read the own enumerable keys of a request object"
        )?;
        // SAFETY: `names` is the string array the call above produced.
        unsafe { Vec::<String>::from_napi_value(self.env, names) }
    }

    fn property(&self, object: &Self::Value, key: &str) -> Result<Self::Value> {
        let mut property_key = ptr::null_mut();
        // SAFETY: as `classify`; the key pointer and length describe a
        // live UTF-8 slice for the duration of the call.
        napi::check_status!(
            unsafe {
                sys::napi_create_string_utf8(
                    self.env,
                    key.as_ptr().cast(),
                    key.len() as isize,
                    &mut property_key,
                )
            },
            "Failed to create the request property key `{key}`"
        )?;

        let mut property = ptr::null_mut();
        // SAFETY: as `classify`.
        napi::check_status!(
            unsafe { sys::napi_get_property(self.env, *object, property_key, &mut property) },
            "Failed to read the request property `{key}`"
        )?;
        Ok(property)
    }

    fn element_count(&self, array: &Self::Value) -> Result<u32> {
        let mut length = 0;
        // SAFETY: as `classify`; `classify` reported this value an array.
        napi::check_status!(
            unsafe { sys::napi_get_array_length(self.env, *array, &mut length) },
            "Failed to read the length of a request array"
        )?;
        Ok(length)
    }

    fn element(&self, array: &Self::Value, index: u32) -> Result<Self::Value> {
        let mut element = ptr::null_mut();
        // SAFETY: as `classify`; reading an index an array does not hold
        // answers `undefined`, which materialises as `null`.
        napi::check_status!(
            unsafe { sys::napi_get_element(self.env, *array, index, &mut element) },
            "Failed to read element {index} of a request array"
        )?;
        Ok(element)
    }

    fn leaf(&self, value: &Self::Value) -> Result<Value> {
        // SAFETY: as `classify`. Only values `classify` called a leaf
        // reach here, so napi-rs's own object walk — the one that drops an
        // `undefined`-valued key, and the one that walks the prototype
        // chain — is never entered; what is reused is its scalar
        // conversion, including the bigint arms and the refusals that name
        // a function, symbol or external as unrepresentable.
        unsafe { Value::from_napi_value(self.env, *value) }
    }
}
