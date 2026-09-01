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
//! The JS object is materialised by
//! [`crate::materialize_js_value`], which puts every own enumerable key of
//! the payload in front of the schema whatever it carries — a key stated
//! as `undefined` included. The schema's closedness therefore holds for
//! the whole payload a caller wrote, not only for the keys they gave a
//! defined value. Own keys are the whole payload: an inherited enumerable
//! key is not part of what a caller wrote and does not reach the schema.
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
//! The same two declarations are the authority for the request's PUBLISHED
//! TypeScript shape: [`crate::host_compile_request_ts`] renders
//! `packages/native/host-compile-request.generated.ts` from them and the
//! DTOs their arms carry, so the declaration a caller type-checks against
//! is the declaration that decodes them.
//!
//! No rule the canonical compile request owns is repeated here. Product-set
//! legality, backend/product compatibility and option admission are decided
//! when the converted request reaches
//! [`verter_ffi::convert::ffi_host_compile_request_to_compile_request`].

use napi::bindgen_prelude::{FromNapiValue, TypeName, ValidateNapiValue};
use napi::{Error, Result, Status, ValueType};
use serde_json::Value;

use crate::host_compile_request_ts::tagged_js_union;
use crate::js_value_graph::{materialize_js_value, NapiValueGraph};

use verter_ffi::types::{
    FfiAnalysisProductRequest, FfiHostCompileIdentity, FfiHostCompileRequest, FfiIdeProductRequest,
    FfiRequestedProduct, FfiRuntimeProductRequest, FfiSvelteCompileOptions,
    FfiSvelteHostCompileRequest, FfiVueCompileOptions, FfiVueHostCompileRequest,
};

tagged_js_union! {
    /// One requested compiler product. The product set is the demand
    /// document: there is no target preset that expands into a bundle of
    /// products, and request order is preserved.
    ///
    /// An arm that carries no options carries nothing but its tag:
    /// `{ "kind": "publicApi", "inline": true }` is a refusal, not a
    /// silently ignored option.
    pub enum NapiRequestedProduct tagged "kind" as "HostRequestedProduct" {
        RuntimeClient(FfiRuntimeProductRequest) => "runtimeClient" as "HostRuntimeClientProduct",
        RuntimeServer(FfiRuntimeProductRequest) => "runtimeServer" as "HostRuntimeServerProduct",
        IdeCompanion(FfiIdeProductRequest) => "ideCompanion" as "HostIdeCompanionProduct",
        /// Carries no options: its output is shaped by host-resolved
        /// profile identities the caller never supplies.
        PublicApi {} => "publicApi" as "HostPublicApiProduct",
        /// Carries no options, for the same reason as `publicApi`.
        Declarations {} => "declarations" as "HostDeclarationsProduct",
        Analysis(FfiAnalysisProductRequest) => "analysis" as "HostAnalysisProduct",
    }
}

tagged_js_union! {
    /// A host compile request tagged by `framework`, so framework-owned
    /// options are structurally unreachable from the other framework's arm
    /// and a foreign key inside either arm is refused at decode.
    pub enum NapiHostCompileRequest tagged "framework" as "HostCompileRequest" {
        Vue {
            identity: FfiHostCompileIdentity,
            products: Vec<NapiRequestedProduct>,
            options: FfiVueCompileOptions,
        } => "vue" as "HostVueCompileRequest",
        Svelte {
            identity: FfiHostCompileIdentity,
            products: Vec<NapiRequestedProduct>,
            options: FfiSvelteCompileOptions,
        } => "svelte" as "HostSvelteCompileRequest",
    }
}

/// Decodes a JS-supplied request value.
///
/// Every refusal — unknown field, unknown tag, missing field, wrong type —
/// is serde's own, reported verbatim so the caller is told which field and
/// which rule refused it. Nothing is substituted on a missing optional
/// slot: it stays absent, and what an absent slot means is the canonical
/// request's decision.
///
/// A property stated as `undefined` arrives here as `null`: a known
/// optional slot still reads as absent, and an unknown or cross-framework
/// key is still present for the schema to refuse by name.
///
/// Which refusals name their slot is serde's own division and is not
/// uniform. An unknown field, an unknown tag and a missing field are
/// named in the message. A slot given the wrong KIND of value is not:
/// `{ ssr: 5 }` and `{ ssr: undefined }` both report `invalid type: …,
/// expected a boolean` with no field name, because the outermost
/// `#[serde(tag = "framework")]` buffers the payload into serde's private
/// content representation before the variant is deserialised, and no
/// deserializer-side path tracker can see through that buffer. Closing
/// that gap means changing how the framework tag is dispatched, which is
/// a change to this decode's shape rather than to its message.
pub fn decode_host_compile_request(value: Value) -> Result<NapiHostCompileRequest> {
    serde_json::from_value(value).map_err(|error| Error::new(Status::InvalidArg, error.to_string()))
}

impl FromNapiValue for NapiHostCompileRequest {
    unsafe fn from_napi_value(
        env: napi::sys::napi_env,
        napi_val: napi::sys::napi_value,
    ) -> Result<Self> {
        // SAFETY: the caller supplies a live env/value pair from napi-rs's
        // own argument extraction, which is the contract `NapiValueGraph`
        // requires of the environment it reads.
        let graph = unsafe { NapiValueGraph::new(env) };
        let value = materialize_js_value(&graph, &napi_val)?;
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
