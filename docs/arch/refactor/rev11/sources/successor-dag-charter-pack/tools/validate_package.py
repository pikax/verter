from __future__ import annotations

from pathlib import Path
import re
import tomllib

ROOT = Path(__file__).resolve().parents[1]
DAG_DIR = ROOT / 'authority/dag'
STATIC_MODULES = [
    'expansion-native-checker.toml',
    'expansion-language-service.toml',
    'expansion-engine-provisioning.toml',
]
GEN_MODULE = 'expansion-native-checker-families.example.toml'

errors: list[str] = []

# Parse every TOML artifact.
for path in sorted(ROOT.rglob('*.toml')):
    try:
        tomllib.loads(path.read_text(encoding='utf-8'))
    except Exception as exc:
        errors.append(f'TOML parse failed {path.relative_to(ROOT)}: {exc}')

# Root generator copies must equal proposed canonical copies.
for name in STATIC_MODULES + [GEN_MODULE]:
    root_copy = ROOT / name
    canonical = DAG_DIR / name
    if not root_copy.exists() or not canonical.exists():
        errors.append(f'missing DAG copy: {name}')
    elif root_copy.read_bytes() != canonical.read_bytes():
        errors.append(f'DAG copy mismatch: {name}')

nodes: dict[str, dict] = {}
module_by_node: dict[str, str] = {}
for name in STATIC_MODULES + [GEN_MODULE]:
    data = tomllib.loads((DAG_DIR / name).read_text(encoding='utf-8'))
    for node in data.get('node', []):
        node_id = node['id']
        if node_id in nodes:
            errors.append(f'duplicate node ID {node_id} in {name} and {module_by_node[node_id]}')
        nodes[node_id] = node
        module_by_node[node_id] = name

# Expected topology counts.
static_ids = [n for n in nodes if not n.startswith('NCF-') and n != 'NCKF0']
family_ids = [n for n in nodes if n.startswith('NCF-')]
if len(static_ids) != 27:
    errors.append(f'expected 27 static nodes, got {len(static_ids)}')
if len(family_ids) != 30:
    errors.append(f'expected 30 NCF nodes, got {len(family_ids)}')
if 'NCKF0' not in nodes:
    errors.append('missing NCKF0')
if len(nodes) != 58:
    errors.append(f'expected 58 total nodes, got {len(nodes)}')

# Manifest/DAG convergence equality.
manifest = tomllib.loads((ROOT / 'catalogs/native-checker-family-manifest.toml').read_text(encoding='utf-8'))
required = [row['id'] for row in manifest.get('slice', []) if row.get('required', True)]
if set(required) != set(family_ids):
    errors.append(f'manifest/NCF set mismatch: manifest-only={sorted(set(required)-set(family_ids))}, dag-only={sorted(set(family_ids)-set(required))}')
if set(nodes['NCKF0']['predecessors']) != set(required):
    errors.append('NCKF0 predecessor set does not equal required manifest slice set')
if 'NCKF0' not in nodes['NCK8']['predecessors']:
    errors.append('NCK8 must depend on NCKF0')
if nodes['NCK8']['external_requirements']:
    errors.append('NCK8 should not rely on external all-slices-complete requirement')

# Deliberate splits.
for node_id, expected_name in {
    'NCK6': 'Family-scoped diagnostic authority arbitration and atomic publication',
    'NCK7': 'Shared diagnostic service and consumer-surface integration',
    'NCK8': 'Native checker terminal and displaced-authority deletion',
    'LSO4': 'References, hierarchy, and bounded occurrence planning',
    'LSO5': 'Semantic rename planning and conflict analysis',
    'LSO8': 'Authored edit transaction engine for rename, fixes, and imports',
    'EPR4': 'Exact authorized engine candidate resolution and selection',
    'EPR5': 'Engine activation epochs, health, and truthful capability publication',
}.items():
    if nodes.get(node_id, {}).get('name') != expected_name:
        errors.append(f'{node_id} split/name mismatch')

# Optional supply-chain channels and external requirements.
for node_id, req in {
    'EPR2': 'maintainer_managed_engine_acquisition',
    'EPR3': 'maintainer_bundled_engine_shipping',
}.items():
    node = nodes[node_id]
    if not node['optional']:
        errors.append(f'{node_id} must be optional')
    if req not in node['external_requirements']:
        errors.append(f'{node_id} missing {req}')

# Charter existence, metadata equality, and required sections.
static_sections = [
    '## Independently acceptable outcome',
    '## Architectural role and end state',
    '## Expected production surfaces',
    '## Named APIs and data boundaries',
    '## Exact predecessor contracts',
    '## Binding architecture',
    '## Internal subblocks',
    '## Data, identity, invalidation, and publication laws',
    '## Migration and cutover',
    '## Deletions',
    '## Forbidden designs',
    '## Acceptance IDs and discriminating proof',
    '## Performance and bounded work',
    '## Mandatory rescope and abort conditions',
    '## Targeted verification',
    '## Consumers and unlocks',
    '## Source reconciliation',
]
for node_id, node in nodes.items():
    cpath = ROOT / node['charter']
    if not cpath.exists():
        errors.append(f'missing charter {node_id}: {node["charter"]}')
        continue
    text = cpath.read_text(encoding='utf-8')
    header_match = re.search(r'<!-- unified-charter-v2\n(.*?)\n-->', text, re.S)
    if not header_match:
        errors.append(f'missing metadata header: {node_id}')
        continue
    meta = {}
    for line in header_match.group(1).splitlines():
        if '=' in line:
            k, v = line.split('=', 1)
            meta[k] = v
    expected = {
        'id': node_id,
        'name': node['name'],
        'predecessors': ','.join(node['predecessors']),
        'conditional_predecessors': ','.join(node['conditional_predecessors']),
        'train': node['train'],
        'product': node['product'],
        'kind': node['kind'],
        'charter': node['charter'],
    }
    for key, value in expected.items():
        if meta.get(key) != value:
            errors.append(f'{node_id} charter metadata mismatch {key}: {meta.get(key)!r} != {value!r}')
    if node_id.startswith('NCF-') or node_id == 'NCKF0':
        for section in ['## Independently acceptable outcome', '## Architecture' if node_id == 'NCKF0' else '## Architectural boundary', '## Internal subblocks', '## Acceptance IDs', '## Forbidden designs', '## Verification']:
            if section not in text:
                errors.append(f'{node_id} missing generated section {section}')
    else:
        for section in static_sections:
            if section not in text:
                errors.append(f'{node_id} missing static section {section}')
        subblocks = len(re.findall(r'^### [A-Z0-9-]+-SB\d+ - ', text, re.M))
        if subblocks < 5:
            errors.append(f'{node_id} has only {subblocks} detailed subblocks')

# Predecessor sanity: intra-package references must exist; external refs are allowed.
all_known_external = {
    'UAK1','D8','E4','G2','TCM3','TIF1','LRA0','PUB0','UAI0','PAR0','IDX0','H3','VIM1','PER0','H2','COX0','UAO0','UAP0','BR0',
    'TCM4','ENCL0','EMB0','B2','CLI2','CLI4','CFG0','VID0','G5',
}
for node_id, node in nodes.items():
    for pred in node['predecessors']:
        if pred not in nodes and pred not in all_known_external:
            errors.append(f'{node_id} unknown predecessor {pred}')
    for cp in node['conditional_predecessors']:
        pred = cp.split(':',1)[0]
        if pred not in nodes and pred not in all_known_external:
            errors.append(f'{node_id} unknown conditional predecessor {pred}')

# Required support docs/catalogs.
required_paths = [
    'README.md', 'architecture/END-STATE.md', 'architecture/DEPENDENCY-RATIONALE.md',
    'architecture/CHARTER-QUALITY-GATE.md', 'amendments/existing-node-amendments.md',
    'sources/legacy-arch-reconciliation.md', 'generated/CHARTER-INDEX.md',
    'catalogs/legacy-arch-disposition.example.toml', 'catalogs/external-requirements.additions.toml',
    'catalogs/review-profile.security-3.example.toml', 'authority/root-module-registration.example.toml',
]
for rel in required_paths:
    if not (ROOT / rel).exists():
        errors.append(f'missing support artifact {rel}')

# Ban known bad/stale wording in final authority/charters.
scan_roots = [ROOT/'authority', ROOT/'charters', ROOT/'architecture', ROOT/'amendments']
banned = {
    'machine-certified diagnostic parity manifest complete': 'external completion assertion replaced by NCKF0',
    'NCK0-NCK7': 'old native checker range',
}
for base in scan_roots:
    for path in base.rglob('*'):
        if path.is_file() and path.suffix in {'.md','.toml'}:
            text = path.read_text(encoding='utf-8')
            for phrase, reason in banned.items():
                if phrase in text:
                    errors.append(f'banned stale phrase {phrase!r} in {path.relative_to(ROOT)} ({reason})')

if errors:
    print('PACKAGE VALIDATION FAILED')
    for err in errors:
        print('-', err)
    raise SystemExit(1)

print(f'PACKAGE VALIDATION PASSED: {len(nodes)} nodes, {len(static_ids)} static, {len(family_ids)} feature slices, 1 generated convergence')
