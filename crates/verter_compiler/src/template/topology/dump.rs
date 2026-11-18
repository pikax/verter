use std::fmt::Write;

use super::VueTemplateTopology;

/// Deterministic text rendering for review and tests: one line per node,
/// indented by depth, with kind, span, region and chain role, then one
/// line per region.
pub fn dump_topology(topology: &VueTemplateTopology<'_>) -> String {
    let mut out = String::new();
    let mut depths: Vec<usize> = Vec::with_capacity(topology.len());
    for id in topology.nodes() {
        let depth = topology
            .parent(id)
            .map_or(0, |parent| depths[parent.index()] + 1);
        depths.push(depth);
        let span = topology.span(id);
        let _ = write!(
            out,
            "{:indent$}{:?} [{}..{}) r{}",
            "",
            topology.kind(id),
            span.start,
            span.end,
            topology.region_of(id).index(),
            indent = depth * 2,
        );
        if let Some((chain, index)) = topology.chain_of(id) {
            let _ = write!(out, " if:{}.{}", chain.index(), index);
        }
        out.push('\n');
    }
    for region in topology.regions() {
        let data = topology.region(region);
        let _ = write!(out, "r{} {:?} parent=", region.index(), data.kind);
        match topology.region_parent(region) {
            Some(parent) => {
                let _ = write!(out, "r{}", parent.index());
            }
            None => out.push('-'),
        }
        out.push_str(" owner=");
        match topology.region_owner(region) {
            Some(owner) => {
                let _ = write!(out, "{}", owner.index());
            }
            None => out.push('-'),
        }
        let _ = writeln!(out, " {:?}", data.completeness);
    }
    out
}
