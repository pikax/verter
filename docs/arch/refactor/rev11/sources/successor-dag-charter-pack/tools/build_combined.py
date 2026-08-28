from pathlib import Path
import tomllib

root=Path(__file__).resolve().parents[1]
mods=['expansion-native-checker.toml','expansion-language-service.toml','expansion-engine-provisioning.toml']
parts=['# Rev11 successor static charters\n', '> Combined review artifact. Canonical individual charter paths remain authoritative within this proposal pack.\n']
for mod in mods:
    data=tomllib.loads((root/'authority/dag'/mod).read_text(encoding='utf-8'))
    parts.append(f"\n# Module `{data['module']}`\n")
    for node in data['node']:
        parts.append('\n---\n')
        parts.append((root/node['charter']).read_text(encoding='utf-8'))
(root/'generated/REV11-SUCCESSOR-STATIC-CHARTERS.md').write_text('\n'.join(parts), encoding='utf-8')

data=tomllib.loads((root/'authority/dag/expansion-native-checker-families.example.toml').read_text(encoding='utf-8'))
parts=['# Native Checker generated feature-slice charters\n', '> Generated review artifact. The family manifest is the source of node identity/scope and individual charter files remain the proposal authority.\n']
for node in data['node']:
    parts.append('\n---\n')
    parts.append((root/node['charter']).read_text(encoding='utf-8'))
(root/'generated/NATIVE-CHECKER-FAMILY-CHARTERS.md').write_text('\n'.join(parts), encoding='utf-8')
print('combined review files rebuilt')
