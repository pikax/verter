from __future__ import annotations

from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parent
PIN_COMMIT = "5a8ca4a391ea6d748f2891f1e39de6aaeec7987e"
PIN_TREE = "7ab8e3317fb8c8a15b7d63c6114a76f79f79f46d"

COMMON = {
    "phase": "expansion",
    "semantic_role": "delivery",
    "class": "successor",
    "dispatchable": True,
    "activation_gate": "ORC0",
    "rescope_loc": 1500,
    "rescope_files": 12,
    "rescope_unrelated_packages": 3,
    "source_refs": ["live:docs/arch/refactor/rev11/sources/legacy-arch-reconciliation.md"],
}


def n(id: str, name: str, predecessors: list[str], *, kind: str = "implementation",
      conflicts: list[str], size: str = "M", resource: str = "rust-mixed",
      gate: str = "targeted-domain", review: str = "architecture-3",
      optional: bool = False, release: str = "none",
      conditional: list[str] | None = None, external: list[str] | None = None,
      loc: int | None = None, files: int | None = None, packages: int | None = None) -> dict[str, Any]:
    if loc is None:
        loc = 0 if resource == "docs-light" else (300 if size == "S" else 800)
    if files is None:
        files = 0 if resource == "docs-light" else (3 if size == "S" else 8)
    if packages is None:
        packages = 0 if resource == "docs-light" else (1 if size == "S" else 2)
    effort = "high" if size in {"M", "L"} or kind in {"constitution", "contract", "cutover", "terminal", "convergence"} else "medium"
    return {
        "id": id, "name": name, "predecessors": predecessors,
        "conditional_predecessors": conditional or [], "kind": kind,
        "conflict_domains": conflicts, "resource_class": resource,
        "gate_profile": gate, "review_profile": review, "size": size,
        "optional": optional, "release_gating": release,
        "external_requirements": external or [],
        "implementation_effort_min": effort,
        "implementation_effort_default": effort,
        "review_effort_min": effort,
        "review_effort_default": effort,
        "verification_effort_min": effort,
        "verification_effort_default": effort,
        "confirmation_effort_min": effort,
        "confirmation_effort_default": effort,
        "max_production_loc": loc,
        "max_production_files": files,
        "max_related_packages": packages,
    }

TRAINS: dict[str, dict[str, Any]] = {
    "expansion-native-checker": {
        "train": "expansion.native-checker",
        "product": "native_checker",
        "owner": "expansion.native-checker:sole native semantic diagnostic authority and certified-family cutover",
        "nodes": [
            n("NCK0", "Native diagnostic authority and parity-certification constitution",
              ["UAK1", "D8", "E4", "G2", "TCM3", "TIF1", "LRA0", "PUB0"],
              kind="constitution", conflicts=["semantic_authority", "diagnostic_action_service", "public_protocol"],
              resource="docs-light", gate="docs-domain", loc=0, files=0, packages=0),
            n("NCK1", "Executable-region and typed semantic-contribution contract",
              ["NCK0", "UAI0", "PAR0", "IDX0"], kind="contract",
              conflicts=["semantic_authority", "carrier_parser", "source_lineage"],
              resource="docs-light", gate="docs-domain", loc=0, files=0, packages=0),
            n("NCK2", "Incremental diagnostic query and result domain",
              ["NCK1", "G2", "H3", "PUB0"],
              conflicts=["semantic_authority", "semantic_cache_store", "public_protocol"]),
            n("NCK3", "Shared-proof semantic diagnostic rule kernel",
              ["NCK2", "D8", "LRA0"],
              conflicts=["semantic_authority", "flowslice", "diagnostic_action_service"]),
            n("NCK4", "Diagnostic-family manifest, hermetic oracle, certification, and node generator",
              ["NCK3", "TCM4", "VIM1", "PER0"],
              conflicts=["semantic_authority", "vertical_manifest", "performance_evidence"]),
            n("NCK5", "Framework diagnostic contribution ingress and profile isolation",
              ["NCK1", "NCK3", "TIF1", "IDX0", "VIM1"],
              conflicts=["semantic_authority", "capability_catalog", "vertical_manifest"]),
            n("NCK6", "Family-scoped diagnostic authority arbitration and atomic publication",
              ["NCK4", "NCK5", "H2", "H3", "COX0", "PUB0"], kind="cutover",
              conflicts=["diagnostic_action_service", "provider_lifecycle", "lsp_publication", "public_protocol"]),
            n("NCK7", "Shared diagnostic service and consumer-surface integration",
              ["NCK6", "PUB0"],
              conditional=["CLI2:when-opened", "CLI4:when-opened"],
              conflicts=["diagnostic_action_service", "public_protocol", "lsp_publication", "cli_application"]),
            n("NCK8", "Native checker terminal and displaced-authority deletion",
              ["NCK7", "NCKF0", "PER0", "UAO0", "UAP0", "BR0"], kind="terminal", size="S",
              conflicts=["semantic_authority", "diagnostic_action_service", "performance_evidence", "program_authority"],
              release="product"),
        ],
    },
    "expansion-language-service": {
        "train": "expansion.language-service",
        "product": "language_service",
        "owner": "expansion.language-service:one authored-coordinate semantic-operation and edit-transaction authority",
        "nodes": [
            n("LSO0", "Authored-coordinate semantic operation constitution",
              ["UAI0", "UAP0", "TCM4", "H3"], kind="constitution",
              conflicts=["public_protocol", "mapping_geometry", "source_lineage"],
              resource="docs-light", gate="docs-domain", loc=0, files=0, packages=0),
            n("LSO1", "Tolerant carrier recovery and two-rail syntax/semantic diagnostics",
              ["LSO0", "PAR0", "EMB0", "B2", "LRA0"],
              conflicts=["carrier_parser", "mapping_geometry", "diagnostic_action_service", "lsp_publication"]),
            n("LSO2", "Canonical authored target and provenance graph",
              ["LSO0", "IDX0", "ENCL0", "TIF1"],
              conflicts=["semantic_authority", "mapping_geometry", "source_lineage"]),
            n("LSO3", "Definition, type-definition, implementation, and symbol navigation",
              ["LSO2"], conflicts=["semantic_authority", "mapping_geometry", "lsp_publication"]),
            n("LSO4", "References, hierarchy, and bounded occurrence planning",
              ["LSO2", "IDX0"], conflicts=["semantic_authority", "mapping_geometry", "performance_evidence"]),
            n("LSO5", "Semantic rename planning and conflict analysis",
              ["LSO4", "LRA0"], conflicts=["semantic_authority", "diagnostic_action_service", "mapping_geometry"]),
            n("LSO6", "Completion candidates and provider-neutral resolve intents",
              ["LSO0", "LSO2", "H2", "TCM4", "PUB0"],
              conflicts=["provider_lifecycle", "mapping_geometry", "public_protocol"]),
            n("LSO7", "Hover, signature-help, and inlay presentation composition",
              ["LSO0", "LSO2", "H2", "TCM4", "PUB0"],
              conflicts=["provider_lifecycle", "public_protocol", "lsp_publication"]),
            n("LSO8", "Authored edit transaction engine for rename, fixes, and imports",
              ["LSO1", "LSO5", "LSO6", "LRA0", "ENCL0"],
              conflicts=["diagnostic_action_service", "mapping_geometry", "source_lineage", "lsp_publication"]),
            n("LSO9", "Vertical language-service conformance and coexistence matrix",
              ["LSO1", "LSO3", "LSO4", "LSO5", "LSO6", "LSO7", "LSO8", "VIM1", "COX0"],
              kind="proof", conditional=["NCK7:when-opened"],
              conflicts=["vertical_manifest", "capability_catalog", "performance_evidence"]),
            n("LSO10", "Language-service convergence and legacy route deletion",
              ["LSO9", "PER0", "UAI0", "UAP0", "BR0"], kind="terminal", size="S",
              conflicts=["semantic_authority", "mapping_geometry", "lsp_publication", "program_authority"],
              release="product"),
        ],
    },
    "expansion-engine-provisioning": {
        "train": "expansion.engine-provisioning",
        "product": "engine_provisioning",
        "owner": "expansion.engine-provisioning:explicit policy-controlled engine acquisition, resolution, and activation authority",
        "nodes": [
            n("EPR0", "External engine provisioning policy and trust constitution",
              ["UAK1", "CFG0", "H2", "PUB0", "TCM4"], kind="constitution",
              conflicts=["provider_lifecycle", "public_protocol", "program_authority"],
              resource="docs-light", gate="docs-domain", review="security-3", loc=0, files=0, packages=0),
            n("EPR1", "Engine artifact identity, compatibility, integrity, and cache contract",
              ["EPR0", "VID0"], kind="contract",
              conflicts=["provider_lifecycle", "source_lineage", "program_authority"],
              resource="docs-light", gate="docs-domain", review="security-3", loc=0, files=0, packages=0),
            n("EPR2", "Managed download and verified atomic installation channel",
              ["EPR1", "G5"], optional=True, release="non_release",
              conflicts=["provider_lifecycle", "scheduler_admission", "source_lineage"],
              review="security-3", external=["maintainer_managed_engine_acquisition"]),
            n("EPR3", "Bundled sidecar shipping and distribution channel",
              ["EPR1"], optional=True, release="non_release",
              conflicts=["provider_lifecycle", "program_authority", "source_lineage"],
              review="security-3", external=["maintainer_bundled_engine_shipping"]),
            n("EPR4", "Exact authorized engine candidate resolution and selection",
              ["EPR1", "H2"], conditional=["EPR2:when-opened", "EPR3:when-opened"],
              conflicts=["provider_lifecycle", "performance_evidence", "source_lineage"]),
            n("EPR5", "Engine activation epochs, health, and truthful capability publication",
              ["EPR4", "H3", "PUB0", "COX0"], kind="convergence",
              conflicts=["provider_lifecycle", "lsp_publication", "public_protocol", "capability_catalog"]),
            n("EPR6", "Offline, enterprise, and supply-chain conformance terminal",
              ["EPR5", "VIM1", "PER0", "BR0"], kind="terminal", size="S",
              conditional=["CLI4:when-opened"],
              conflicts=["provider_lifecycle", "performance_evidence", "program_authority"],
              review="security-3", release="product"),
        ],
    },
}


def q(value: str) -> str:
    return '"' + value.replace('\\', '\\\\').replace('"', '\\"') + '"'


def arr(values: list[str]) -> str:
    return '[' + ', '.join(q(v) for v in values) + ']'


def render_module(module: str, cfg: dict[str, Any]) -> str:
    out = [f"schema = 4", f"module = {q(module)}", f"pinned_commit = {q(PIN_COMMIT)}", f"pinned_tree = {q(PIN_TREE)}", ""]
    for node in cfg["nodes"]:
        d = {**COMMON, **node}
        d["train"] = cfg["train"]
        d["product"] = cfg["product"]
        d["owner"] = cfg["owner"]
        d["charter"] = f"charters/{module}/{d['id']}.md"
        out.append("[[node]]")
        ordered = [
            "id", "name", "predecessors", "conditional_predecessors", "phase", "train", "product",
            "kind", "semantic_role", "class", "owner", "conflict_domains", "resource_class",
            "gate_profile", "review_profile", "dispatchable", "optional", "release_gating",
            "source_refs", "external_requirements", "activation_gate", "charter",
            "implementation_effort_min", "implementation_effort_default", "review_effort_min",
            "review_effort_default", "verification_effort_min", "verification_effort_default",
            "confirmation_effort_min", "confirmation_effort_default", "size", "max_production_loc",
            "max_production_files", "max_related_packages", "rescope_loc", "rescope_files",
            "rescope_unrelated_packages",
        ]
        for key in ordered:
            value = d[key]
            if isinstance(value, list):
                out.append(f"{key} = {arr(value)}")
            elif isinstance(value, bool):
                out.append(f"{key} = {'true' if value else 'false'}")
            elif isinstance(value, int):
                out.append(f"{key} = {value}")
            else:
                out.append(f"{key} = {q(str(value))}")
        out.append("")
    return "\n".join(out).rstrip() + "\n"


for module, cfg in TRAINS.items():
    (ROOT / f"{module}.toml").write_text(render_module(module, cfg), encoding="utf-8")

print("wrote", len(TRAINS), "DAG modules and", sum(len(x['nodes']) for x in TRAINS.values()), "nodes")
