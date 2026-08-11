//! Deterministic rendering of a [`snapshot`](super::snapshot).
//!
//! COMPILED ONLY under the `attribution` feature.
//!
//! Both renderers walk [`WorkSite::ALL`] in declaration order and format
//! integers only, so two runs that observed the same work produce
//! byte-identical output. That is what lets a baseline capture be
//! diffed, and what lets the determinism digests be compared without a
//! bespoke comparison tool.

use std::fmt::Write as _;

use super::schema::{WorkDomain, WorkSite};
use super::table::{snapshot, SiteSample};

/// A domain's roll-up across its sites.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DomainTotal {
    /// The rolled-up domain.
    pub domain: WorkDomain,
    /// Sites in the domain that recorded something.
    pub sites: u32,
    /// Total hits.
    pub calls: u64,
    /// Total inclusive nanoseconds.
    pub nanos: u64,
    /// Total allocations charged to the domain's sites.
    pub alloc_count: u64,
    /// Total allocated bytes charged to the domain's sites.
    pub alloc_bytes: u64,
    /// Total net bytes (allocated minus released) charged to the domain.
    pub net_bytes: u64,
}

/// Roll a snapshot up by domain, in [`WorkDomain`] declaration order.
///
/// Domains with no observations are omitted.
pub fn domain_totals(rows: &[SiteSample]) -> Vec<DomainTotal> {
    let mut out: Vec<DomainTotal> = Vec::new();
    for site in WorkSite::ALL {
        let domain = site.domain();
        let Some(row) = rows.iter().find(|row| row.site == *site) else {
            continue;
        };
        match out.iter_mut().find(|total| total.domain == domain) {
            Some(total) => {
                total.sites += 1;
                total.calls += row.calls;
                total.nanos += row.nanos;
                total.alloc_count += row.alloc_count;
                total.alloc_bytes += row.alloc_bytes;
                total.net_bytes += row.net_bytes();
            }
            None => out.push(DomainTotal {
                domain,
                sites: 1,
                calls: row.calls,
                nanos: row.nanos,
                alloc_count: row.alloc_count,
                alloc_bytes: row.alloc_bytes,
                net_bytes: row.net_bytes(),
            }),
        }
    }
    out
}

/// Render the current snapshot as a tab-separated dataset with a header
/// row.
///
/// One row per site that recorded something; columns are
/// `site`, `domain`, `unit`, `calls`, `amount`, `ns`, `digest`,
/// `alloc_count`, `alloc_bytes`, `dealloc_bytes`.
pub fn render_tsv() -> String {
    let rows = snapshot();
    let mut out = String::with_capacity(128 * (rows.len() + 1));
    out.push_str(
        "site\tdomain\tunit\tcalls\tamount\tns\tdigest\talloc_count\talloc_bytes\tdealloc_bytes\n",
    );
    for row in &rows {
        let _ = writeln!(
            out,
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            row.id(),
            row.domain().id(),
            row.unit().id(),
            row.calls,
            row.amount,
            row.nanos,
            row.digest,
            row.alloc_count,
            row.alloc_bytes,
            row.dealloc_bytes,
        );
    }
    out
}

/// Render the current snapshot as a JSON object with `sites` and
/// `domains` arrays.
///
/// Hand-written rather than `serde`-derived so the field order, and
/// therefore the bytes, are fixed by this function and not by a derive.
pub fn render_json() -> String {
    let rows = snapshot();
    let totals = domain_totals(&rows);
    let mut out = String::with_capacity(256 * (rows.len() + totals.len() + 2));
    out.push_str("{\n  \"sites\": [\n");
    for (index, row) in rows.iter().enumerate() {
        let comma = if index + 1 == rows.len() { "" } else { "," };
        let _ = writeln!(
            out,
            "    {{\"site\": \"{}\", \"domain\": \"{}\", \"unit\": \"{}\", \"calls\": {}, \"amount\": {}, \"ns\": {}, \"digest\": {}, \"alloc_count\": {}, \"alloc_bytes\": {}, \"dealloc_bytes\": {}}}{comma}",
            row.id(),
            row.domain().id(),
            row.unit().id(),
            row.calls,
            row.amount,
            row.nanos,
            row.digest,
            row.alloc_count,
            row.alloc_bytes,
            row.dealloc_bytes,
        );
    }
    out.push_str("  ],\n  \"domains\": [\n");
    for (index, total) in totals.iter().enumerate() {
        let comma = if index + 1 == totals.len() { "" } else { "," };
        let _ = writeln!(
            out,
            "    {{\"domain\": \"{}\", \"sites\": {}, \"calls\": {}, \"ns\": {}, \"alloc_count\": {}, \"alloc_bytes\": {}, \"net_bytes\": {}}}{comma}",
            total.domain.id(),
            total.sites,
            total.calls,
            total.nanos,
            total.alloc_count,
            total.alloc_bytes,
            total.net_bytes,
        );
    }
    out.push_str("  ]\n}\n");
    out
}
