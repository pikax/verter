//! Probe SYNTHESIS — the FIXED, VERSIONED probe the generator appends to the
//! query's resolution environment so tsgo's `textDocument/hover` prints the
//! resolved type (`docs/arch/u0-oracle-harness-design.md` §Q2 "Probe-driven
//! generation", §4 `probe_form_is_deterministic_and_versioned` /
//! `parameterized_probe_rhs_synthesis`).
//!
//! PURE + tsgo-free: builds source text only. The generator (feature-gated)
//! drives tsgo over the synthesized source; this module never contacts tsgo, so
//! it lives on the consumption-reachable side and is exercised offline.
//!
//! The probe is `type __oracle_probe__<query_ordinal> = <RHS>;`, placed in the
//! query's OWN resolution environment: a same-file append for `ResolveExpr` /
//! `ShallowSurfaceExpr` (`support.rs:132,160`), a scratch file = `eval_source`
//! prelude + trailing probe for `EvaluateExpr` (`support.rs:208`,
//! `evaluate_type_expression.rs:~314`). The naming + RHS rules are versioned by
//! `PROBE_SYNTHESIS_VERSION` (in `snapshot_id`), so the probe locator is
//! derivable from version + query without tsgo.

/// The fixed probe-symbol prefix. The full name is `__oracle_probe__<ordinal>`.
pub(crate) const PROBE_PREFIX: &str = "__oracle_probe__";

/// The fixed distributive-identity HELPER prefix. The full helper name is
/// `__oracle_probe_dist__<ordinal>` — ordinal-keyed like the probe itself, so
/// two queries synthesized into the same environment never collide.
pub(crate) const PROBE_DIST_PREFIX: &str = "__oracle_probe_dist__";

/// The deterministic probe symbol name for a query ordinal.
pub(crate) fn probe_name(ordinal: u16) -> String {
    format!("{PROBE_PREFIX}{ordinal}")
}

/// The deterministic distributive-identity helper name for a query ordinal.
pub(crate) fn dist_helper_name(ordinal: u16) -> String {
    format!("{PROBE_DIST_PREFIX}{ordinal}")
}

/// The synthesized distributive-identity scaffold: the helper declaration plus
/// the wrapped probe RHS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Scaffold {
    /// `type __oracle_probe_dist__<N><T> = T extends never ? never : T;`
    pub(crate) helper_decl: String,
    /// `__oracle_probe_dist__<N><symbol>`
    pub(crate) rhs: String,
}

/// The distributive-identity probe scaffold (the `ProbeRhsKind::
/// DistributiveIdentity` capture strategy) — a PURE function of
/// `(ordinal, symbol)`, versioned by `PROBE_SYNTHESIS_VERSION`.
///
/// `T extends never ? never : T` is a universal type-level identity:
/// distribution maps each union member `M` through `M extends never ? never :
/// M = M` (no inhabited type extends `never`); the empty union distributes to
/// `never` (still the identity); `any`/`unknown` are identities under it too.
/// Hovering the wrapped probe makes tsgo print the EXPANDED member union of
/// its own `keyof` enumeration — the conditional only severs the union
/// display-origin pointer that makes a plain hover re-print `keyof <operand>`.
/// The helper is INLINED (not lib `Exclude<T, never>`) so the probe semantics
/// are pinned by `PROBE_SYNTHESIS_VERSION` alone, with no lib coupling.
pub(crate) fn distributive_identity_scaffold(ordinal: u16, symbol: &str) -> Scaffold {
    let helper = dist_helper_name(ordinal);
    Scaffold {
        helper_decl: format!("type {helper}<T> = T extends never ? never : T;"),
        rhs: format!("{helper}<{symbol}>"),
    }
}

/// Why a probe RHS cannot be synthesized for the currently-admissible set —
/// the construct stays `Ignored` until its named spike lands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProbeRhsError {
    /// A `ResolveExpr` with NON-EMPTY `type_args` needs the deterministic,
    /// versioned `TypeExpr` → TS-source type-argument printer (a
    /// `probe_synthesis_version` bump, §4 `parameterized_probe_rhs_synthesis`).
    /// Until that printer is spiked + versioned, parameterized rows stay
    /// `Ignored`; empty-`type_args` rows use the bare-`symbol` RHS now.
    ParameterizedTypeArgsDeferred,
}

/// The RHS for a `ResolveExpr` / `ShallowSurfaceExpr` probe.
///
/// Empty `type_args` → the bare `symbol` RHS (admissible NOW). NON-EMPTY
/// `type_args` → `ParameterizedTypeArgsDeferred` (the printer is not yet spiked,
/// §4). This is the ONLY currently-admissible RHS form for these helpers.
pub(crate) fn resolve_expr_probe_rhs(
    symbol: &str,
    type_args: &[String],
) -> Result<String, ProbeRhsError> {
    if type_args.is_empty() {
        Ok(symbol.to_string())
    } else {
        Err(ProbeRhsError::ParameterizedTypeArgsDeferred)
    }
}

/// The full probe header line `type <probe_name> = <rhs>;`. This is the
/// `raw_capture.probe_header` the offline `probe_header_names_target` audit
/// re-checks against the captured hover.
pub(crate) fn probe_header(ordinal: u16, rhs: &str) -> String {
    format!("type {} = {};", probe_name(ordinal), rhs)
}

/// Where the synthesized probe was placed, for the hover capture: the full
/// synthesized SOURCE plus the byte offset of the probe NAME (the hover
/// position — hovering the alias name elicits its resolved RHS).
pub(crate) struct SynthesizedProbe {
    pub(crate) source: String,
    pub(crate) probe_name_offset: usize,
}

/// Append the probe to the query's own source — the placement for `ResolveExpr`
/// / `ShallowSurfaceExpr` (same-file append) AND, with `base = eval_source`
/// prelude, the `EvaluateExpr` scratch-file model (the prelude + trailing probe;
/// the caller passes the prelude as `base`). Returns the synthesized source and
/// the probe-NAME offset for the hover position.
///
/// The synthesized text is `base` (with a guaranteed trailing newline) followed
/// by the probe header on its own line, so the probe never merges into a
/// trailing token of `base`.
pub(crate) fn append_probe(base: &str, ordinal: u16, rhs: &str) -> SynthesizedProbe {
    append_probe_with_scaffold(base, ordinal, rhs, None)
}

/// [`append_probe`] with an optional scaffold helper declaration emitted on its
/// own line IMMEDIATELY BEFORE the probe line. `probe_name_offset` semantics are
/// unchanged: it points at the probe alias NAME (the hover position), never at
/// the helper. A `None` scaffold is the bare path verbatim.
pub(crate) fn append_probe_with_scaffold(
    base: &str,
    ordinal: u16,
    rhs: &str,
    scaffold_decl: Option<&str>,
) -> SynthesizedProbe {
    let scaffold_len = scaffold_decl.map(str::len).unwrap_or(0);
    let mut source = String::with_capacity(base.len() + rhs.len() + scaffold_len + 40);
    source.push_str(base);
    if !source.is_empty() && !source.ends_with('\n') {
        source.push('\n');
    }
    if let Some(decl) = scaffold_decl {
        source.push_str(decl);
        source.push('\n');
    }
    const TYPE_KW: &str = "type ";
    source.push_str(TYPE_KW);
    let probe_name_offset = source.len();
    source.push_str(&probe_name(ordinal));
    source.push_str(" = ");
    source.push_str(rhs);
    source.push_str(";\n");
    SynthesizedProbe {
        source,
        probe_name_offset,
    }
}

#[cfg(test)]
mod tests;
