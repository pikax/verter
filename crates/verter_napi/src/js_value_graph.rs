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
//! Structural rather than about vocabulary, because a JS graph can
//! describe things JSON has no term for:
//!
//! - a value nested past [`MAX_NESTING_DEPTH`], which is also how a graph
//!   that refers back to itself is refused — `const a = {}; a.self = a`
//!   has no JSON representation at all, and without a bound the traversal
//!   would run until the stack guard page killed the process;
//! - an array declaring more than [`MAX_ARRAY_ELEMENTS`] elements, and an
//!   object exposing more than that many own enumerable keys. A declared
//!   array length is free in V8 — `new Array(2 ** 32 - 1)` allocates
//!   nothing — so the count is reserved against the decoded-value budget
//!   before anything is reserved. Object keys are counted the same way,
//!   and each key's UTF-8 length is charged before the Rust string is
//!   copied;
//! - a dense binary object (`Buffer`, typed array, `DataView`). Those
//!   values expose every byte index as an enumerable own key, so
//!   enumerating them would allocate millions of numeric key strings
//!   before either budget ran. They are refused at classification,
//!   before the key list is read;
//! - a graph that exceeds [`MAX_DECODED_VALUES_PER_REQUEST`] values or
//!   [`MAX_RETAINED_BYTES_PER_REQUEST`] retained bytes. A tiny shared JS
//!   DAG expands into a distinct native copy on every visit, so the
//!   traversal itself is what the budget bounds.
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

/// How many elements one request array may declare, and how many own
/// enumerable keys one request object may expose.
///
/// Every array the schema carries is a short list of names or products.
/// Object key lists are real, but a dense exotic object can still expose
/// millions of index keys; the count is checked before those names are
/// converted into Rust strings.
pub const MAX_ARRAY_ELEMENTS: u32 = 1 << 16;

/// Decoded JSON nodes one request graph may materialise.
///
/// Sized to admit one legal max-length array plus the handful of objects
/// the schema wraps it in, and to refuse a graph that expands far past
/// that by revisiting a shared JS value.
pub const MAX_DECODED_VALUES_PER_REQUEST: usize = (MAX_ARRAY_ELEMENTS as usize).saturating_mul(2);

/// Native bytes one request graph may retain.
pub const MAX_RETAINED_BYTES_PER_REQUEST: usize = 8 * 1024 * 1024;

/// Shared accounting for payloads materialised during one call.
///
/// A batch may reuse the same JS value at many positions. Each traversal
/// creates distinct native strings, vectors, and JSON values, so every
/// traversal consumes the budget even when V8 stores the input only once.
///
/// Decoded-value accounting is per request graph: a batch resets it
/// between entries so a few thousand small requests are admitted by the
/// retained-byte and entry-count bounds. Retained bytes accumulate across
/// the whole call and abort it when exhausted.
#[doc(hidden)]
#[derive(Debug)]
pub struct JsValueMaterializationBudget {
    decoded_values: usize,
    retained_bytes: usize,
    max_decoded_values: usize,
    max_retained_bytes: usize,
    values_exhausted: bool,
    bytes_exhausted: bool,
}

impl JsValueMaterializationBudget {
    #[doc(hidden)]
    pub fn new(max_decoded_values: usize, max_retained_bytes: usize) -> Self {
        Self {
            decoded_values: 0,
            retained_bytes: 0,
            max_decoded_values,
            max_retained_bytes,
            values_exhausted: false,
            bytes_exhausted: false,
        }
    }

    /// Per-request decoded-value / retained-byte caps used by the singular
    /// route and by each batch entry's request graph.
    pub fn per_request() -> Self {
        Self::new(
            MAX_DECODED_VALUES_PER_REQUEST,
            MAX_RETAINED_BYTES_PER_REQUEST,
        )
    }

    fn ensure_decoded_values(&mut self, additional: usize) -> Result<()> {
        let total = self.decoded_values.checked_add(additional);
        if total.is_none_or(|total| total > self.max_decoded_values) {
            self.values_exhausted = true;
            return Err(Error::new(
                Status::InvalidArg,
                format!(
                    "A compile request materializes more than {} decoded values",
                    self.max_decoded_values
                ),
            ));
        }
        Ok(())
    }

    /// Increment the decoded-value counter before the caller allocates a
    /// matching capacity. Nested sparse arrays therefore cannot reserve
    /// many full buffers while the counter is still near zero.
    fn reserve_decoded_values(&mut self, additional: usize) -> Result<()> {
        self.ensure_decoded_values(additional)?;
        self.decoded_values += additional;
        Ok(())
    }

    fn retain_value(&mut self) -> Result<()> {
        self.reserve_decoded_values(1)
    }

    pub(crate) fn retain_bytes(&mut self, additional: usize) -> Result<()> {
        let total = self.retained_bytes.checked_add(additional);
        if total.is_none_or(|total| total > self.max_retained_bytes) {
            self.bytes_exhausted = true;
            return Err(Error::new(
                Status::InvalidArg,
                format!(
                    "A compile request retains more than {} bytes of decoded payload",
                    self.max_retained_bytes
                ),
            ));
        }
        self.retained_bytes = total.expect("the checked total was validated above");
        Ok(())
    }

    pub(crate) fn reset_decoded_values(&mut self) {
        self.decoded_values = 0;
        self.values_exhausted = false;
    }

    pub(crate) fn bytes_exhausted(&self) -> bool {
        self.bytes_exhausted
    }
}

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

/// Own enumerable string keys of one object, counted before any key is
/// converted into a retained Rust string.
pub trait JsObjectKeys {
    /// How many own enumerable string keys the object exposes.
    fn count(&self) -> Result<u32>;

    /// UTF-8 bytes the key at `index` will retain, measured before
    /// [`Self::at`] copies them.
    fn retained_bytes(&self, index: u32) -> Result<usize>;

    /// The key at `index` in JS enumeration order.
    fn at(&self, index: u32) -> Result<String>;
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
    /// One object's own enumerable string keys, counted before conversion.
    type Keys: JsObjectKeys;

    /// Which materialisation rule `value` falls under.
    fn classify(&self, value: &Self::Value) -> Result<JsValueClass>;

    /// Own enumerable string-keyed properties of `object`, in JS
    /// enumeration order. Inherited and symbol-keyed properties are not
    /// part of the payload a caller wrote. The returned handle is counted
    /// and charged before any key string is retained.
    fn own_enumerable_keys(&self, object: &Self::Value) -> Result<Self::Keys>;

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

    /// Native bytes the materialised leaf will retain. This is queried
    /// before [`Self::leaf`] allocates them.
    fn leaf_retained_bytes(&self, value: &Self::Value) -> Result<usize>;
}

/// Materialises `value` and everything reachable from it.
///
/// One traversal of the supplied graph: each value is classified once,
/// each own key is read once, each element is read once. A graph that
/// refers back to itself has no traversal that terminates and no JSON
/// representation either, so it is refused by [`MAX_NESTING_DEPTH`]
/// rather than followed.
///
/// Bounded by [`MAX_DECODED_VALUES_PER_REQUEST`] and
/// [`MAX_RETAINED_BYTES_PER_REQUEST`].
pub fn materialize_js_value<G: JsValueGraph>(graph: &G, value: &G::Value) -> Result<Value> {
    let mut budget = JsValueMaterializationBudget::per_request();
    materialize_js_value_with_budget(graph, value, &mut budget)
}

/// Materialises one value while charging a budget shared by its call.
#[doc(hidden)]
pub fn materialize_js_value_with_budget<G: JsValueGraph>(
    graph: &G,
    value: &G::Value,
    budget: &mut JsValueMaterializationBudget,
) -> Result<Value> {
    materialize_nested(graph, value, 0, budget, true)
}

/// `materialize_js_value` at a known nesting depth.
fn materialize_nested<G: JsValueGraph>(
    graph: &G,
    value: &G::Value,
    depth: u32,
    budget: &mut JsValueMaterializationBudget,
    charge_self: bool,
) -> Result<Value> {
    if depth > MAX_NESTING_DEPTH {
        return Err(Error::new(
            Status::InvalidArg,
            format!(
                "A request value nests deeper than {MAX_NESTING_DEPTH} levels, \
                 or refers back to itself; neither has a JSON representation"
            ),
        ));
    }

    if charge_self {
        budget.retain_value()?;
    }

    Ok(match graph.classify(value)? {
        JsValueClass::Undefined => Value::Null,
        JsValueClass::Leaf => {
            budget.retain_bytes(graph.leaf_retained_bytes(value)?)?;
            graph.leaf(value)?
        }
        JsValueClass::Array => {
            // Checked and reserved before anything is allocated: the length
            // is what a caller declares, not what the array holds.
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
            budget.reserve_decoded_values(declared as usize)?;
            let mut materialized = Vec::with_capacity(declared as usize);
            for index in 0..declared {
                let element = graph.element(value, index)?;
                materialized.push(materialize_nested(
                    graph,
                    &element,
                    depth + 1,
                    budget,
                    false,
                )?);
            }
            Value::Array(materialized)
        }
        JsValueClass::Object => {
            let keys = graph.own_enumerable_keys(value)?;
            let declared = keys.count()?;
            if declared > MAX_ARRAY_ELEMENTS {
                return Err(Error::new(
                    Status::InvalidArg,
                    format!(
                        "A request object exposes {declared} keys, above the \
                         {MAX_ARRAY_ELEMENTS} a request may carry"
                    ),
                ));
            }
            budget.reserve_decoded_values(declared as usize)?;
            let mut materialized = Map::new();
            for index in 0..declared {
                budget.retain_bytes(keys.retained_bytes(index)?)?;
                let key = keys.at(index)?;
                let property = graph.property(value, &key)?;
                let property = materialize_nested(graph, &property, depth + 1, budget, false)?;
                materialized.insert(key, property);
            }
            Value::Object(materialized)
        }
    })
}

/// UTF-8 byte length of a JavaScript string, without allocating a Rust
/// copy. The value must already have been classified as a string.
pub(crate) fn napi_utf8_string_len(env: sys::napi_env, value: sys::napi_value) -> Result<usize> {
    let mut length = 0;
    // SAFETY: the caller supplies a live env/value pair; a null output
    // buffer asks Node for the UTF-8 byte length without allocating.
    napi::check_status!(
        unsafe { sys::napi_get_value_string_utf8(env, value, ptr::null_mut(), 0, &mut length) },
        "Failed to measure a request string"
    )?;
    Ok(length)
}

fn napi_is_flag(
    env: sys::napi_env,
    value: sys::napi_value,
    probe: unsafe fn(sys::napi_env, sys::napi_value, *mut bool) -> sys::napi_status,
    what: &str,
) -> Result<bool> {
    let mut flag = false;
    napi::check_status!(
        // SAFETY: the environment/value pair is live per the graph
        // constructor's contract; `flag` is a stack bool the probe writes.
        unsafe { probe(env, value, &mut flag) },
        "Failed to detect whether a request value is {what}"
    )?;
    Ok(flag)
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

/// Own enumerable names of one live JS object, still as a JS string array
/// so the count can be refused before any name is copied into Rust.
pub(crate) struct NapiObjectKeys {
    env: sys::napi_env,
    names: sys::napi_value,
}

impl NapiObjectKeys {
    fn name_at(&self, index: u32) -> Result<sys::napi_value> {
        let mut name = ptr::null_mut();
        napi::check_status!(
            unsafe { sys::napi_get_element(self.env, self.names, index, &mut name) },
            "Failed to read own enumerable key {index} of a request object"
        )?;
        Ok(name)
    }
}

impl JsObjectKeys for NapiObjectKeys {
    fn count(&self) -> Result<u32> {
        let mut length = 0;
        napi::check_status!(
            unsafe { sys::napi_get_array_length(self.env, self.names, &mut length) },
            "Failed to read the own enumerable key count of a request object"
        )?;
        Ok(length)
    }

    fn retained_bytes(&self, index: u32) -> Result<usize> {
        let name = self.name_at(index)?;
        napi_utf8_string_len(self.env, name)
    }

    fn at(&self, index: u32) -> Result<String> {
        let name = self.name_at(index)?;
        // SAFETY: `name` is a string element of the names array produced
        // by `napi_get_all_property_names` with `numbers_to_strings`.
        unsafe { String::from_napi_value(self.env, name) }
    }
}

impl JsValueGraph for NapiValueGraph {
    type Value = sys::napi_value;
    type Keys = NapiObjectKeys;

    fn classify(&self, value: &Self::Value) -> Result<JsValueClass> {
        match napi::type_of!(self.env, *value)? {
            ValueType::Undefined => Ok(JsValueClass::Undefined),
            ValueType::Object => {
                if napi_is_flag(self.env, *value, sys::napi_is_array, "an array")? {
                    return Ok(JsValueClass::Array);
                }
                // Buffer, typed arrays and DataView expose every byte index
                // as an enumerable own key. Refuse them before the key
                // list is materialised.
                if napi_is_flag(self.env, *value, sys::napi_is_buffer, "a Buffer")?
                    || napi_is_flag(self.env, *value, sys::napi_is_typedarray, "a typed array")?
                    || napi_is_flag(self.env, *value, sys::napi_is_dataview, "a DataView")?
                {
                    return Err(Error::new(
                        Status::InvalidArg,
                        "A request value is a binary or typed-array object, \
                         which has no JSON representation",
                    ));
                }
                Ok(JsValueClass::Object)
            }
            _ => Ok(JsValueClass::Leaf),
        }
    }

    fn own_enumerable_keys(&self, object: &Self::Value) -> Result<Self::Keys> {
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
        Ok(NapiObjectKeys {
            env: self.env,
            names,
        })
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

    fn leaf_retained_bytes(&self, value: &Self::Value) -> Result<usize> {
        if napi::type_of!(self.env, *value)? != ValueType::String {
            return Ok(0);
        }
        napi_utf8_string_len(self.env, *value)
    }
}
