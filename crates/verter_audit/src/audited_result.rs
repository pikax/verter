#![deny(missing_docs)]
//! [`AuditedResult`] — an audit-bearing execution envelope.
//!
//! [`AuditedResult<T, E>`] pairs the outcome of an audited request —
//! either a success value `T` or a typed error `E` — with the
//! [`RequestAuditRecord`] gathered while producing it. It is the
//! return carrier for audited host entry-points that hand back both
//! the result and the observability envelope in a single value.
//!
//! ## Why this lives in `verter_audit`
//!
//! The envelope is NOT a protobuf wire DTO: it is generic over the
//! domain payload `T` and the error `E`, a shape protobuf cannot
//! express. It also embeds the [`RequestAuditRecord`], which is an
//! audit-substrate type. Placing the carrier in `verter_protocol`
//! would either invert the crate dependency (pulling the audit record
//! into the protobuf-authoritative crate) or force a hand-written
//! TypeScript mirror that drifts from the generated audit surface.
//! `verter_audit` owns `ts-rs` and `packages/types/audit.generated.ts`,
//! so the carrier rides the existing `ts-rs` export path: consumers
//! (for example `packages/typeinfo`) import the generated
//! `AuditedResult` type rather than re-declaring it.
//!
//! ## Shape
//!
//! A `#[serde(tag = "kind")]` discriminated enum, so the JSON / TS form
//! carries a `kind: "Ok" | "Err"` discriminant alongside the value and
//! the audit record. This mirrors the existing audit-substrate enum
//! convention and keeps the success / error split explicit on the wire
//! rather than hiding it inside a nested `Result`.

use serde::{Deserialize, Serialize};

use crate::record::RequestAuditRecord;

/// The outcome of an audited request together with the
/// [`RequestAuditRecord`] captured while producing it.
///
/// `T` is the success payload, `E` the typed error. Both arms carry
/// the audit record so the observability envelope survives regardless
/// of whether the request succeeded — a caller that only wants the
/// audit can read [`AuditedResult::audit`] without inspecting the
/// outcome.
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export_to = "audit.generated.ts")]
#[serde(tag = "kind")]
pub enum AuditedResult<T, E> {
    /// The request succeeded, producing `value`. `audit` is the
    /// envelope gathered while producing it.
    Ok {
        /// The success payload.
        value: T,
        /// The audit envelope captured for the request.
        audit: RequestAuditRecord,
    },
    /// The request failed with the typed `error`. `audit` is the
    /// envelope gathered while producing it — error paths are audited
    /// identically to success paths.
    Err {
        /// The typed error describing the failure.
        error: E,
        /// The audit envelope captured for the request.
        audit: RequestAuditRecord,
    },
}

impl<T, E> AuditedResult<T, E> {
    /// Construct a success outcome carrying `value` and its `audit`
    /// envelope.
    #[must_use]
    pub fn ok(value: T, audit: RequestAuditRecord) -> Self {
        Self::Ok { value, audit }
    }

    /// Construct an error outcome carrying `error` and its `audit`
    /// envelope.
    #[must_use]
    pub fn err(error: E, audit: RequestAuditRecord) -> Self {
        Self::Err { error, audit }
    }

    /// Borrow the [`RequestAuditRecord`] regardless of outcome.
    #[must_use]
    pub fn audit(&self) -> &RequestAuditRecord {
        match self {
            Self::Ok { audit, .. } | Self::Err { audit, .. } => audit,
        }
    }

    /// Borrow the outcome as a [`Result`], dropping the audit record.
    /// Use [`AuditedResult::audit`] to read the envelope alongside.
    #[must_use]
    pub fn as_result(&self) -> Result<&T, &E> {
        match self {
            Self::Ok { value, .. } => Ok(value),
            Self::Err { error, .. } => Err(error),
        }
    }

    /// Consume the carrier, returning the outcome and the audit record
    /// as a pair. The two halves can then be routed independently —
    /// the `Result` to the caller, the record to the records store.
    #[must_use]
    pub fn into_parts(self) -> (Result<T, E>, RequestAuditRecord) {
        match self {
            Self::Ok { value, audit } => (Ok(value), audit),
            Self::Err { error, audit } => (Err(error), audit),
        }
    }

    /// Consume the carrier, returning only the outcome and discarding
    /// the audit record.
    #[must_use]
    pub fn into_result(self) -> Result<T, E> {
        match self {
            Self::Ok { value, .. } => Ok(value),
            Self::Err { error, .. } => Err(error),
        }
    }

    /// Map the success payload, preserving the error arm and the audit
    /// record unchanged.
    #[must_use]
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> AuditedResult<U, E> {
        match self {
            Self::Ok { value, audit } => AuditedResult::Ok {
                value: f(value),
                audit,
            },
            Self::Err { error, audit } => AuditedResult::Err { error, audit },
        }
    }

    /// Map the error payload, preserving the success arm and the audit
    /// record unchanged.
    #[must_use]
    pub fn map_err<F>(self, f: impl FnOnce(E) -> F) -> AuditedResult<T, F> {
        match self {
            Self::Ok { value, audit } => AuditedResult::Ok { value, audit },
            Self::Err { error, audit } => AuditedResult::Err {
                error: f(error),
                audit,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::RequestMemoryAudit;
    use crate::record::{RequestKind, RequestKindPayload};
    use crate::store::RequestStoreAudit;
    use crate::timing::RequestTimingAudit;

    fn sample_record(request_id: u64) -> RequestAuditRecord {
        RequestAuditRecord {
            request_id,
            canonical_id: "/sample.vue".to_string(),
            kind: RequestKind::TypeInfoGraph,
            parent_request_id: None,
            from_cache: false,
            timings: RequestTimingAudit::default(),
            memory: RequestMemoryAudit::default(),
            store: RequestStoreAudit::default(),
            footprint: None,
            scheduler: None,
            files: Vec::new(),
            waits: None,
            kind_payload: RequestKindPayload::None,
            trace_id: String::new(),
        }
    }

    #[test]
    fn ok_constructor_carries_value_and_audit() {
        let result: AuditedResult<u32, String> = AuditedResult::ok(7, sample_record(11));
        assert_eq!(result.audit().request_id, 11);
        assert_eq!(result.as_result(), Ok(&7));
    }

    #[test]
    fn err_constructor_carries_error_and_audit() {
        let result: AuditedResult<u32, String> =
            AuditedResult::err("boom".to_string(), sample_record(12));
        assert_eq!(result.audit().request_id, 12);
        assert_eq!(result.as_result(), Err(&"boom".to_string()));
    }

    #[test]
    fn into_parts_splits_outcome_from_record() {
        let result: AuditedResult<u32, String> = AuditedResult::ok(42, sample_record(13));
        let (outcome, audit) = result.into_parts();
        assert_eq!(outcome, Ok(42));
        assert_eq!(audit.request_id, 13);
    }

    #[test]
    fn into_result_discards_record() {
        let ok: AuditedResult<u32, String> = AuditedResult::ok(1, sample_record(14));
        assert_eq!(ok.into_result(), Ok(1));
        let err: AuditedResult<u32, String> = AuditedResult::err("e".into(), sample_record(15));
        assert_eq!(err.into_result(), Err("e".to_string()));
    }

    #[test]
    fn map_transforms_success_preserves_error_arm_and_audit() {
        let ok: AuditedResult<u32, String> = AuditedResult::ok(3, sample_record(16));
        let mapped = ok.map(|v| v * 10);
        assert_eq!(mapped.audit().request_id, 16);
        assert_eq!(mapped.into_result(), Ok(30));

        let err: AuditedResult<u32, String> = AuditedResult::err("keep".into(), sample_record(17));
        let mapped_err = err.map(|v| v * 10);
        assert_eq!(mapped_err.into_result(), Err("keep".to_string()));
    }

    #[test]
    fn map_err_transforms_error_preserves_success_arm_and_audit() {
        let err: AuditedResult<u32, String> = AuditedResult::err("123".into(), sample_record(18));
        let mapped = err.map_err(|e| e.len());
        assert_eq!(mapped.audit().request_id, 18);
        assert_eq!(mapped.into_result(), Err(3));

        let ok: AuditedResult<u32, String> = AuditedResult::ok(9, sample_record(19));
        let mapped_ok = ok.map_err(|e: String| e.len());
        assert_eq!(mapped_ok.into_result(), Ok(9));
    }

    #[test]
    fn serde_round_trips_through_json_with_kind_tag() {
        let ok: AuditedResult<u32, String> = AuditedResult::ok(5, sample_record(20));
        let json = serde_json::to_string(&ok).expect("serialize ok");
        assert!(
            json.contains("\"kind\":\"Ok\""),
            "expected serde tag `kind: Ok`, got: {json}"
        );
        let back: AuditedResult<u32, String> = serde_json::from_str(&json).expect("deserialize ok");
        assert_eq!(back.as_result(), Ok(&5));
        assert_eq!(back.audit().request_id, 20);

        let err: AuditedResult<u32, String> =
            AuditedResult::err("nope".to_string(), sample_record(21));
        let json_err = serde_json::to_string(&err).expect("serialize err");
        assert!(
            json_err.contains("\"kind\":\"Err\""),
            "expected serde tag `kind: Err`, got: {json_err}"
        );
        let back_err: AuditedResult<u32, String> =
            serde_json::from_str(&json_err).expect("deserialize err");
        assert_eq!(back_err.as_result(), Err(&"nope".to_string()));
    }

    #[test]
    fn ts_export_emits_generic_definition() {
        // Discriminating ts-rs check: the generic enum must export a
        // generic TypeScript definition (`AuditedResult<T, E>`) carrying
        // both arms and the audit record field. If ts-rs could not
        // express the generic the carrier would fail to compile or this
        // assertion would not find the parametrised name.
        use ts_rs::TS;
        let cfg = ts_rs::Config::default();
        let decl = <AuditedResult<ts_rs::Dummy, ts_rs::Dummy> as TS>::decl(&cfg);
        assert!(
            decl.contains("AuditedResult<T, E>"),
            "expected a generic `AuditedResult<T, E>` TS declaration, got: {decl}"
        );
        assert!(
            decl.contains("\"Ok\"") && decl.contains("\"Err\""),
            "expected both Ok and Err arms in the TS declaration, got: {decl}"
        );
        assert!(
            decl.contains("audit: RequestAuditRecord"),
            "expected the audit record field in the TS declaration, got: {decl}"
        );
    }
}
