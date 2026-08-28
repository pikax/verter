from __future__ import annotations

import pathlib
import textwrap
import tomllib
from typing import Any

ROOT = pathlib.Path(__file__).resolve().parent


def load_nodes(toml_name: str) -> dict[str, dict[str, Any]]:
    data = tomllib.loads((ROOT / toml_name).read_text(encoding='utf-8'))
    return {node['id']: node for node in data['node']}


def lines(items: list[str], prefix: str = '- ') -> str:
    return '\n'.join(f'{prefix}{item}' for item in items)


def metadata_comment(node: dict[str, Any]) -> str:
    def csv(values: list[str]) -> str:
        return ','.join(values)

    fields = [
        ('id', node['id']),
        ('name', node['name']),
        ('phase', node['phase']),
        ('train', node['train']),
        ('product', node['product']),
        ('kind', node['kind']),
        ('semantic_role', node['semantic_role']),
        ('class', node['class']),
        ('predecessors', csv(node['predecessors'])),
        ('conditional_predecessors', csv(node['conditional_predecessors'])),
        ('owner', node['owner']),
        ('conflict_domains', csv(node['conflict_domains'])),
        ('resource_class', node['resource_class']),
        ('review_profile', node['review_profile']),
        ('gate_profile', node['gate_profile']),
        ('implementation_effort_min', node['implementation_effort_min']),
        ('implementation_effort_default', node['implementation_effort_default']),
        ('review_effort_min', node['review_effort_min']),
        ('review_effort_default', node['review_effort_default']),
        ('verification_effort_min', node['verification_effort_min']),
        ('verification_effort_default', node['verification_effort_default']),
        ('confirmation_effort_min', node['confirmation_effort_min']),
        ('confirmation_effort_default', node['confirmation_effort_default']),
        ('size', node['size']),
        ('dispatchable', str(node['dispatchable']).lower()),
        ('optional', str(node['optional']).lower()),
        ('release_gating', node['release_gating']),
        ('source_refs', csv(node['source_refs'])),
        ('external_requirements', csv(node['external_requirements'])),
        ('activation_gate', node['activation_gate']),
        ('charter', node['charter']),
        ('max_production_loc', str(node['max_production_loc'])),
        ('max_production_files', str(node['max_production_files'])),
        ('max_related_packages', str(node['max_related_packages'])),
        ('rescope_loc', str(node['rescope_loc'])),
        ('rescope_files', str(node['rescope_files'])),
        ('rescope_unrelated_packages', str(node['rescope_unrelated_packages'])),
        ('initial_state', node.get('initial_state', '')),
    ]
    body = '\n'.join(f'{key}={value}' for key, value in fields)
    return f'<!-- unified-charter-v2\n{body}\n-->'


def render_subblocks(node_id: str, subblocks: list[dict[str, Any]]) -> str:
    rendered: list[str] = []
    for index, sb in enumerate(subblocks, 1):
        sid = sb.get('id', f'{node_id}-SB{index}')
        rendered.append(f'### {sid} - {sb["title"]}')
        rendered.append('')
        rendered.append(f'**Independently testable outcome:** {sb["outcome"]}')
        rendered.append('')
        rendered.append('**Architecture:**')
        rendered.append('')
        rendered.append(lines(sb['architecture']))
        rendered.append('')
        rendered.append('**Expected changes:**')
        rendered.append('')
        rendered.append(lines(sb['changes']))
        rendered.append('')
        rendered.append('**Discriminating proof:**')
        rendered.append('')
        rendered.append(lines(sb['proof']))
        rendered.append('')
    return '\n'.join(rendered).rstrip()


def render_charter(node: dict[str, Any], spec: dict[str, Any]) -> str:
    predecessor_lines = []
    pred_contracts = spec.get('predecessor_contracts', {})
    for pred in node['predecessors']:
        predecessor_lines.append(
            f'**{pred}:** {pred_contracts.get(pred, "consume the exact accepted receipt, current digest, and declared public contract; no predecessor behavior may be inferred from branch state alone.")}'
        )
    for pred in node['conditional_predecessors']:
        base = pred.split(':', 1)[0]
        predecessor_lines.append(
            f'**{pred}:** {pred_contracts.get(base, "when opened, bind the exact accepted receipt and include its capability in the final conformance matrix; when unopened, prove the corresponding path performs zero work.")}'
        )

    acceptance = list(spec.get('acceptance', []))
    default_acceptance = [
        f'**{node["id"]}-AC-SOLE:** a planted displaced authority or duplicate route is rejected by a static or runtime guard.',
        f'**{node["id"]}-AC-CONTRACT:** the named APIs, identities, outcomes, and provenance fields are exact, deterministic, and complete for this block.',
        f'**{node["id"]}-AC-INCREMENTAL:** incremental execution equals fresh execution on the same basis; cancelled, stale, partial, or NeedInputs outcomes are never warm-admitted as complete.',
        f'**{node["id"]}-AC-WORK:** equivalent-work counters prove no hidden parse, resolve, index walk, provider call, allocation, copy, or retained candidate beyond the declared demand.',
    ]
    acceptance.extend(default_acceptance)

    perf = list(spec.get('performance', []))
    perf.extend([
        f'Target ceiling: {node["max_production_loc"]} production LOC, {node["max_production_files"]} production files, and {node["max_related_packages"]} related packages.',
        'No wall-time claim is accepted without equivalent-work counters and allocation/RSS evidence for the same semantic work.',
        'After warmup, 100 identical requests must show no unbounded retained-byte growth and no repeated provider or filesystem work unless the request explicitly demands it.',
    ])

    abort = list(spec.get('abort', []))
    abort.extend([
        f'Rescope before mutation above {node["rescope_loc"]} production LOC, {node["rescope_files"]} files, or {node["rescope_unrelated_packages"]} unrelated packages.',
        'Rescope when a public/wire change, concurrency/lifetime change, and semantic algorithm change would otherwise land in one review context.',
        'Abort on any wrong-complete result, stale publication, provenance loss, identity aliasing, silent fallback, or inability to name the sole final owner.',
    ])

    source_rows = spec.get('sources', [])
    source_text = lines(source_rows) if source_rows else '- `docs/arch/refactor/rev11/sources/legacy-arch-reconciliation.md`'

    text = f'''\
{metadata_comment(node)}

# {node['id']} - {node['name']}

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation and release gates, external authorizations, source atom digests, conflict-domain admission, and the landing-frozen candidate before mutation.

The internal subblocks below are binding decomposition and review checkpoints. They do not receive independent dispatch, leases, receipts, or deletion ownership unless the pre-scope architect proves that one subblock is independently acceptable or the block crosses a mandatory rescope trigger. In that case, amend the DAG before production mutation rather than treating a train-sized subblock as an implementation checklist.

## Independently acceptable outcome

{spec['outcome']}

The current owner is **{spec['current_owner']}**. The final and sole owner is **{spec['final_owner']}**.

## Architectural role and end state

{spec['role']}

## Expected production surfaces

{lines(spec['surfaces'])}

These are expected ownership surfaces, not permission to touch all listed paths. The dispatch packet must bind exact path and symbol sets after reconciling the live tree. A newly discovered owner or unrelated package requires an amendment or rescope.

## Named APIs and data boundaries

{lines(spec['apis'])}

## Exact predecessor contracts

{lines(predecessor_lines)}

External custody: {('none beyond the package activation boundary.' if not node['external_requirements'] else '; '.join(node['external_requirements']) + '. Dispatch fails until the canonical authorization receipt exists.')}

## Binding architecture

{lines(spec['principles'])}

## Internal subblocks

{render_subblocks(node['id'], spec['subblocks'])}

## Data, identity, invalidation, and publication laws

{lines(spec['laws'])}

## Migration and cutover

{lines(spec['migration'])}

## Deletions

{lines(spec['deletions'])}

Deletion ownership is exact. This block may delete only the routes and artifacts named above after their replacement is proven on the same candidate. Neighboring legacy deletion remains with its owning node.

## Forbidden designs

{lines(spec['forbidden'])}

The program-wide prohibitions also apply: no dual-running semantic authority, compatibility fallback, string or regex semantic recovery, test-only production bypass, sleep or polling readiness, resource-capacity predecessor, unqualified cache identity, or hidden whole-workspace eager work.

## Acceptance IDs and discriminating proof

{lines(acceptance)}

## Performance and bounded work

{lines(perf)}

## Mandatory rescope and abort conditions

{lines(abort)}

## Targeted verification

{lines(spec['verification'], prefix='1. ')}

The canonical gate profile remains authoritative. Targeted success is iteration evidence only. Final acceptance requires fresh RED/GREEN mutation evidence, the exact gate receipt, and the configured independent review profile on the landing-frozen tree.

## Consumers and unlocks

{lines(spec['consumers'])}

## Source reconciliation

{source_text}

Durable clauses are transferred as digest-bound requirement atoms. Historical path archaeology, obsolete branches, and implementation journals are not copied into the charter. Git history remains the archive.
'''
    return textwrap.dedent(text).strip() + '\n'


def write_train(toml_name: str, specs: dict[str, dict[str, Any]], output_dir: str) -> None:
    nodes = load_nodes(toml_name)
    missing = set(nodes) - set(specs)
    extra = set(specs) - set(nodes)
    if missing or extra:
        raise SystemExit(f'spec mismatch for {toml_name}: missing={sorted(missing)} extra={sorted(extra)}')
    out = ROOT / output_dir
    out.mkdir(parents=True, exist_ok=True)
    for node_id, node in nodes.items():
        (out / f'{node_id}.md').write_text(render_charter(node, specs[node_id]), encoding='utf-8')
