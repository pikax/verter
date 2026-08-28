from __future__ import annotations

from pathlib import Path
import tomllib

ROOT = Path(__file__).resolve().parents[1]
MODULES = [
    ROOT / 'authority/dag/expansion-native-checker.toml',
    ROOT / 'authority/dag/expansion-native-checker-families.example.toml',
    ROOT / 'authority/dag/expansion-language-service.toml',
    ROOT / 'authority/dag/expansion-engine-provisioning.toml',
]

nodes = []
for path in MODULES:
    data = tomllib.loads(path.read_text(encoding='utf-8'))
    for node in data.get('node', []):
        node = dict(node)
        node['_module'] = path.name
        nodes.append(node)

family_scope = {}
manifest = tomllib.loads((ROOT / 'catalogs/native-checker-family-manifest.toml').read_text(encoding='utf-8'))
for row in manifest.get('slice', []):
    family_scope[row['id']] = row['scope']

train_order = {
    'expansion.native-checker': 0,
    'expansion.language-service': 1,
    'expansion.engine-provisioning': 2,
}

nodes.sort(key=lambda n: (train_order.get(n['train'], 9), n['id']))

lines = [
    '# Charter index', '',
    f'This pack contains **{len(nodes)} DAG charters**: 27 static successor nodes, 30 generated native-checker feature slices, and one generated required-family convergence node.', '',
    '| Node | Train | Kind | Predecessors | Charter | Independently acceptable boundary |',
    '|---|---|---|---|---|---|',
]
for node in nodes:
    charter = node['charter']
    cpath = ROOT / charter
    if node['id'] in family_scope:
        boundary = family_scope[node['id']]
    elif node['id'] == 'NCKF0':
        boundary = 'machine-generated convergence of every required native-checker feature-slice receipt'
    else:
        text = cpath.read_text(encoding='utf-8') if cpath.exists() else ''
        marker = '## Independently acceptable outcome\n\n'
        if marker in text:
            boundary = text.split(marker, 1)[1].split('\n\n', 1)[0].replace('\n', ' ')
        else:
            boundary = node['name']
    if len(boundary) > 180:
        boundary = boundary[:177] + '...'
    preds = ', '.join(node['predecessors']) if node['predecessors'] else '—'
    lines.append(f"| `{node['id']}` | `{node['train']}` | {node['kind']} | {preds} | [`{charter}`](../{charter}) | {boundary} |")

(ROOT / 'generated/CHARTER-INDEX.md').write_text('\n'.join(lines) + '\n', encoding='utf-8')

# Dependency rationale, with the structural rationale derived from node classes and known anchors.
rationale_map = {
    'NCK0': 'Waits on accepted shared flow/query/storage, certified TypeScript observation, TypeInfo, diagnostic/action, and public result contracts before defining diagnostic authority.',
    'NCK1': 'Builds region/contribution contracts only after the checker constitution and universal parser/index identity contracts.',
    'NCK2': 'Requires region identity, same-key query production, stale-safe publication basis, and public typed outcomes.',
    'NCK3': 'Consumes live diagnostic queries plus complete shared flow/relation/call facts and LRA action/provenance law.',
    'NCK4': 'Requires a functioning rule kernel, activated external observation plane, manifest generator, and performance methodology before generating/certifying slices.',
    'NCK5': 'Requires region/contribution contracts and the shared rule kernel before framework contributions can participate without a second checker.',
    'NCK6': 'Arbitration is meaningful only after certification infrastructure, framework isolation, provider epochs, stale-safe publication, coexistence, and public outcomes exist.',
    'NCK7': 'Consumer integration must consume the final shared authority/publication plan rather than reimplementing it; CLI consumers are conditional to avoid reverse dependencies.',
    'NCKF0': 'Generated convergence waits on every manifest-required feature slice and replaces an external “all slices complete” assertion.',
    'NCK8': 'Terminal deletion/promotion waits on consumer closure, generated required-family convergence, performance, universal contracts, and successor promotion authority.',
    'LSO0': 'The operation constitution consumes final identity/coordinate, public/capability, mapper/provider, and stale-publication laws.',
    'LSO1': 'Recovery needs the authored operation law plus parser, embedded mapping, accepted B2 recovery, and diagnostic provenance contracts.',
    'LSO2': 'The target graph requires canonical authored operation identity, bounded index candidates, strict coordinate cutover, and TypeInfo component identity.',
    'LSO3': 'Navigation is a pure consumer of the canonical target graph; no additional broad predecessor is needed.',
    'LSO4': 'Occurrences require the target graph and bounded workspace candidates, but remain independently useful before rename/edit work.',
    'LSO5': 'Rename policy depends on complete role-typed occurrences and action safety/provenance, but not final edit materialization.',
    'LSO6': 'Completion composition needs targets, provider binding/mapper activation, and public capability/outcome contracts.',
    'LSO7': 'Presentation composition uses the same target/provider/public contracts but remains independent of completion and edits.',
    'LSO8': 'Edit materialization waits on recovery stability, semantic rename plans, completion intents, action safety, and exact coordinate conversion.',
    'LSO9': 'Conformance waits on every operation implementation plus VIM/COX; Native Checker consumer conformance is conditional when that product is opened.',
    'LSO10': 'Terminal deletion/promotion waits on exact conformance, performance, identity/public contract locks, and successor promotion.',
    'EPR0': 'Policy must consume universal configuration, ProviderHub, public outcomes, and certified engine binding before any acquisition channel opens.',
    'EPR1': 'Artifact identity/trust contract is downstream only of explicit policy and exact release identity.',
    'EPR2': 'Managed acquisition is optional, explicitly authorized, and requires the artifact/install contract plus bounded scheduler pools.',
    'EPR3': 'Bundled shipping is optional and depends only on policy/artifact contract; it is a release channel, not a runtime lifecycle block.',
    'EPR4': 'Resolution consumes validated artifacts and ProviderHub requirements; optional acquisition/bundle channels become conditional inputs only when opened.',
    'EPR5': 'Activation waits on deterministic selection and then composes stale-safe publication, public capability truth, and coexistence.',
    'EPR6': 'Terminal closure waits on activation, VIM/PER0 evidence, successor promotion, and optional CLI consumer integration.',
}

dep = [
    '# Dependency rationale', '',
    'The predecessor graph is semantic, not resource-based. No edge represents machine capacity, staffing, or a desire to serialize unrelated work.', '',
    '| Node | Direct predecessors | Rationale |', '|---|---|---|',
]
for node in nodes:
    if node['id'].startswith('NCF-'):
        why = 'Generated feature slice waits on NCK4 certification/generation, NCK6 promotion infrastructure, and only the exact semantic/framework/index facts declared by its manifest row.'
    else:
        why = rationale_map.get(node['id'], 'Generated/declared convergence over exact predecessor receipts; no implementation semantics are inferred from branch state.')
    cp = list(node.get('conditional_predecessors', []))
    pred_text = ', '.join(node['predecessors'] + cp) or '—'
    dep.append(f"| `{node['id']}` | {pred_text} | {why} |")

(ROOT / 'architecture/DEPENDENCY-RATIONALE.md').write_text('\n'.join(dep) + '\n', encoding='utf-8')

print('indexed', len(nodes), 'nodes')
