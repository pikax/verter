//! The native host compile request: a tag-discriminated JS shape decoded
//! at the Node boundary and converted into the FFI request schema.
//!
//! ## Why this is not an `#[napi(object)]` struct
//!
//! `#[napi(object)]`'s derived conversion reads only the property names it
//! declares and never enumerates the JS object's own keys, so an
//! unrecognised or cross-framework key is silently dropped before any
//! refusal can run. The legacy compile-profile route works around that
//! with a hand-written key list that has to be kept in step by hand. This
//! request instead crosses the boundary as a `serde_json::Value` and is
//! decoded by the same `deny_unknown_fields` schema the FFI layer already
//! owns, so unknown-key, cross-framework, closed-vocabulary and
//! missing-field refusals are the schema's own and cannot drift from it.
//!
//! One JS-side gap survives that, and it is the same gap the decode
//! function documents below: an own property whose value is `undefined` is
//! dropped while the JS object is read into a `Value`, so it never reaches
//! serde at all. `{ framework: "vue", …, runes: undefined }` therefore
//! decodes clean rather than refusing on the cross-framework key. What is
//! refused is every key a caller states with a value; a key stated as
//! `undefined` is indistinguishable from one never written.
//!
//! ## What this layer owns, and what it deliberately does not
//!
//! It owns the JS-facing DISCRIMINATION: a `framework` tag on the request
//! and a `kind` tag on each requested product, which is what a TypeScript
//! discriminated union needs in order to narrow exhaustively. The FFI
//! schema tags the same two unions externally (`{ "vue": … }`), which
//! narrows poorly in TypeScript.
//!
//! It does NOT re-declare the framework option vocabularies. Those belong
//! to the protocol crate, and a second copy here could disagree with the
//! first; the identity, option and per-product payload types are used
//! verbatim, so their fields cannot be dropped, reordered or defaulted on
//! the way through — there is no per-field copy to get wrong.
//!
//! No rule the canonical compile request owns is repeated here. Product-set
//! legality, backend/product compatibility and option admission are decided
//! when the converted request reaches
//! [`verter_ffi::convert::ffi_host_compile_request_to_compile_request`].

use napi::bindgen_prelude::{FromNapiValue, TypeName, ValidateNapiValue};
use napi::{Error, Result, Status, ValueType};
use serde::Deserialize;
use serde_json::Value;

use verter_ffi::types::{
    FfiAnalysisProductRequest, FfiHostCompileIdentity, FfiHostCompileRequest, FfiIdeProductRequest,
    FfiRequestedProduct, FfiRuntimeProductRequest, FfiSvelteCompileOptions,
    FfiSvelteHostCompileRequest, FfiVueCompileOptions, FfiVueHostCompileRequest,
};

/// One requested compiler product, tagged by `kind`.
///
/// The variants that carry no options are empty struct variants rather
/// than unit variants so that `deny_unknown_fields` still applies to them:
/// `{ "kind": "publicApi", "inline": true }` is a refusal, not a silently
/// ignored option.
#[derive(Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum NapiRequestedProduct {
    RuntimeClient(FfiRuntimeProductRequest),
    RuntimeServer(FfiRuntimeProductRequest),
    IdeCompanion(FfiIdeProductRequest),
    PublicApi {},
    Declarations {},
    Analysis(FfiAnalysisProductRequest),
}

/// A host compile request tagged by `framework`, so framework-owned
/// options are structurally unreachable from the other framework's arm and
/// a foreign key inside either arm is refused at decode.
#[derive(Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(tag = "framework", rename_all = "camelCase", deny_unknown_fields)]
pub enum NapiHostCompileRequest {
    Vue {
        identity: FfiHostCompileIdentity,
        products: Vec<NapiRequestedProduct>,
        options: FfiVueCompileOptions,
    },
    Svelte {
        identity: FfiHostCompileIdentity,
        products: Vec<NapiRequestedProduct>,
        options: FfiSvelteCompileOptions,
    },
}

/// Decodes a JS-supplied request value.
///
/// Every refusal — unknown field, unknown tag, missing field, wrong type —
/// is serde's own, reported verbatim so the caller is told which field and
/// which rule refused it. Nothing is substituted on a missing optional
/// slot: it stays absent, and what an absent slot means is the canonical
/// request's decision.
///
/// A JS property whose value is `undefined` is not carried into the
/// decoded value at all, so it reads as absent rather than as `null`. That
/// holds for an unknown key too: an `undefined`-valued key is dropped
/// before serde can refuse it, which is the one refusal the closed schema
/// cannot reach from here.
pub fn decode_host_compile_request(value: Value) -> Result<NapiHostCompileRequest> {
    serde_json::from_value(value).map_err(|error| Error::new(Status::InvalidArg, error.to_string()))
}

impl FromNapiValue for NapiHostCompileRequest {
    unsafe fn from_napi_value(
        env: napi::sys::napi_env,
        napi_val: napi::sys::napi_value,
    ) -> Result<Self> {
        // SAFETY: the caller supplies a live env/value pair from napi-rs's
        // own argument extraction, which is exactly the contract
        // `Value::from_napi_value` expects.
        let value = unsafe { Value::from_napi_value(env, napi_val)? };
        decode_host_compile_request(value)
    }
}

impl TypeName for NapiHostCompileRequest {
    fn type_name() -> &'static str {
        "HostCompileRequest"
    }

    fn value_type() -> ValueType {
        ValueType::Object
    }
}

/// The type imposes no constraint beyond what `from_napi_value` already
/// checks, so the default (accept, then decode) validation is correct.
impl ValidateNapiValue for NapiHostCompileRequest {}

/// Converts the tagged native request into the FFI request schema.
///
/// Exhaustive in both NATIVE unions with no rest pattern and no wildcard
/// arm, so a framework or product added to the JS-facing schema is a
/// compile error here rather than a request that quietly converts to the
/// wrong arm. The FFI side is railed where it is destructured, not here: a
/// variant added to `FfiRequestedProduct` or `FfiHostCompileRequest` alone
/// leaves this function compiling unchanged and is caught by the equally
/// exhaustive match in
/// [`verter_ffi::convert::ffi_host_compile_request_to_compile_request`].
pub fn napi_host_compile_request_to_ffi(request: NapiHostCompileRequest) -> FfiHostCompileRequest {
    match request {
        NapiHostCompileRequest::Vue {
            identity,
            products,
            options,
        } => FfiHostCompileRequest::Vue(FfiVueHostCompileRequest {
            identity,
            products: requested_products_to_ffi(products),
            options,
        }),
        NapiHostCompileRequest::Svelte {
            identity,
            products,
            options,
        } => FfiHostCompileRequest::Svelte(FfiSvelteHostCompileRequest {
            identity,
            products: requested_products_to_ffi(products),
            options,
        }),
    }
}

/// Requested products keep their request order: the product set is the
/// demand document, and reordering it would change what the caller asked
/// for.
fn requested_products_to_ffi(products: Vec<NapiRequestedProduct>) -> Vec<FfiRequestedProduct> {
    products
        .into_iter()
        .map(|product| match product {
            NapiRequestedProduct::RuntimeClient(runtime) => {
                FfiRequestedProduct::RuntimeClient(runtime)
            }
            NapiRequestedProduct::RuntimeServer(runtime) => {
                FfiRequestedProduct::RuntimeServer(runtime)
            }
            NapiRequestedProduct::IdeCompanion(ide) => FfiRequestedProduct::IdeCompanion(ide),
            NapiRequestedProduct::PublicApi {} => FfiRequestedProduct::PublicApi,
            NapiRequestedProduct::Declarations {} => FfiRequestedProduct::Declarations,
            NapiRequestedProduct::Analysis(analysis) => FfiRequestedProduct::Analysis(analysis),
        })
        .collect()
}
