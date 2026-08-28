# Review and fix-cycle contract

This policy uses the honest-operator trusted-local assurance model authorized by the 2026-08-28 superseding directive. Every substantive node and every system, product, convergence, cutover, and terminal node requires three fresh, distinct harness review tasks on one exact finalized candidate tree using the assigned lenses and computed effort. `programctl harness-record` imports each prompt and report by one exact-byte read and records the task identity, provider, model, effort, lease, round, node, and candidate bindings. These are operator attestations and audit records, not cryptographic proof of harness authenticity or malicious-owner resistance. P0/P1 block final acceptance. P2/P3 follow the current owning disposition policy. Any content or effort-policy change invalidates the current evidence. Final acceptance is current-round exact-candidate clean 3/3 plus fresh verification and confirmation.

Every node publishes per-role low/medium/high minima and defaults. Admission deterministically takes the maximum of those values and stable risk/surface/kind/concurrency/semantic-authority escalation rules; an explicit override may only raise it. The architecture consult is separate and fixed to read-only OpenAI Codex `gpt-5.6-sol` at `xhigh`. Two review/fix cycles are the soft maximum: a P0/P1 after the second cycle requires that neutral Architect to decide continue/stop and record an additional-round cap before another admission. Reports are honest and terse, and disposable task/worktree cleanup is reported.

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-ORCH-L1-0B3D2393E277

- Kind: `context`
- Source: `orchestration-findings.md:1-1`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `0b3d2393e277386d153e4090d311912f9334d791fc621d923369d56b86c291ee`

~~~~markdown
# Rev11 orchestration / rescoping findings for Codex PRO
~~~~

### SRC-ORCH-L3-D1EF91F2621F

- Kind: `context`
- Source: `orchestration-findings.md:3-3`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `d1ef91f2621f170a40335d4992e154131724888f63c8afaa2fdfd45668d23dde`

~~~~markdown
Use the following as architectural input when revisiting the Rev11 plan, DAG, charters, and orchestration model.
~~~~

### SRC-ORCH-L5-0ED8FFAEDDC0

- Kind: `acceptance`
- Source: `orchestration-findings.md:5-5`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `0ed8ffaeddc07f20bd81489b57447a37fadb0f7b00cb0350f85c5ccedf0e44fb`

~~~~markdown
The goal is **not to weaken Rev11, C1, J1, or any other ambitious architectural work**. The goal is to preserve the architecture while fixing the execution shape that turned some nominal “blocks” into multi-day trains with unnecessarily large acceptance surfaces, excessive governance churn, poor parallelism, and avoidable model/token cost.
~~~~

### SRC-ORCH-L7-B337B543D6B7

- Kind: `context`
- Source: `orchestration-findings.md:7-7`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `b337b543d6b7af6725c8f3d9f3edfeb405fd777ac5695b2e9405c18b99bfad50`

~~~~markdown
One important correction to earlier discussion: **keep** **`program/architecture-lock`** **as the canonical integration/control branch. Do not redesign the system so that** **`architecture-lock`** **consumes** **`refactor/product-branch`****.** Independent trains/blocks may execute on their own branches and merge into `program/architecture-lock`. If a clean code-only/product branch is retained, it should be downstream/derived from accepted architecture-lock work, not the authority that architecture-lock follows.
~~~~

### SRC-ORCH-L9-F52D711103D5

- Kind: `context`
- Source: `orchestration-findings.md:9-9`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`

~~~~markdown
---
~~~~

### SRC-ORCH-L1129-D2E20071F132

- Kind: `context`
- Source: `orchestration-findings.md:1129-1129`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `d2e20071f132e536771c76027b1322140c85d9cdb272f32a5b7e1e1dff169b90`

~~~~markdown
# 27. Model effort should be allocated by architectural risk
~~~~

### SRC-ORCH-L1131-C28193D449AB

- Kind: `context`
- Source: `orchestration-findings.md:1131-1131`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `c28193d449ab23f2108bbf563a985f56e0809ca2cbb9dba553a3be4cde41d750`

~~~~markdown
Do not use maximum reasoning effort everywhere.
~~~~

### SRC-ORCH-L1133-D311414C88D0

- Kind: `context`
- Source: `orchestration-findings.md:1133-1133`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `d311414c88d03b36dbe3667c255901ff94406ee659a1c0cfd56222df66754446`

~~~~markdown
Use the expensive models where mistakes multiply downstream cost.
~~~~

### SRC-EXP-L1714-52BA65A7F12E

- Kind: `context`
- Source: `successor-expansion.md:1714-1714`
- Applicability: `CLI3`
- Exact text SHA-256: `52ba65a7f12e385cbcc661da10e31ca32730bdd5e75fb6191c9ed3b81ed4ba20`

~~~~markdown
## 18. Evidence, review questions, and candid risks
~~~~

### SRC-EXP-L1730-5A16DD9444F4

- Kind: `requirement`
- Source: `successor-expansion.md:1730-1730`
- Applicability: `CLI3`
- Exact text SHA-256: `5a16dd9444f4976f89e86924af975cbf366e5601f7c6db0a2339e0055292ea40`

~~~~markdown
### 18.2 Questions every architecture review must attack
~~~~

### SRC-EXP-L1732-3C091B351A8F

- Kind: `context`
- Source: `successor-expansion.md:1732-1732`
- Applicability: `CLI3`
- Exact text SHA-256: `3c091b351a8f29d18e142126fba30c18cfb64105ad0a2b4f28a34ee4909c1f83`

~~~~markdown
1. Does any “shared” abstraction contain a hidden Vue, React, HTML, or Next semantic branch?
~~~~

### SRC-EXP-L1733-813F3361E0A1

- Kind: `context`
- Source: `successor-expansion.md:1733-1733`
- Applicability: `CLI3`
- Exact text SHA-256: `813f3361e0a1a0a9badd29adf08d3b09145649cb5b2abb33b1b539a05140dc8a`

~~~~markdown
2. Can a post-snapshot TypeScript fact influence the transform that created that snapshot?
~~~~

### SRC-EXP-L1734-2C7570691961

- Kind: `context`
- Source: `successor-expansion.md:1734-1734`
- Applicability: `CLI3`
- Exact text SHA-256: `2c7570691961e029676e9fdf55296f9a0ae21e59a3d96ca141fc62aede64e092`

~~~~markdown
3. Can two parser, type, config, map, cache, index, or public-schema authorities answer the same question?
~~~~

### SRC-EXP-L1735-AF9C10BDBC54

- Kind: `context`
- Source: `successor-expansion.md:1735-1735`
- Applicability: `CLI3`
- Exact text SHA-256: `af9c10bdbc54e7dafca22922921f27361964d759574cbd7a20d3dc6db94ae135`

~~~~markdown
4. Can a disabled or selected-but-unrequested profile do observable work?
~~~~

### SRC-EXP-L1736-63CE88414980

- Kind: `context`
- Source: `successor-expansion.md:1736-1736`
- Applicability: `CLI3`
- Exact text SHA-256: `63ce8841498055a88cce80f8fed9c3fd93262a84d917de15f06e19df6f2ea4a5`

~~~~markdown
5. Can two framework releases collide in activation, caches, rules, diagnostics, or metadata?
~~~~

### SRC-EXP-L1737-70E973307C78

- Kind: `context`
- Source: `successor-expansion.md:1737-1737`
- Applicability: `CLI3`
- Exact text SHA-256: `70e973307c78a61c9a5d560cb804c329f135ec45ab4d448cc75274ceea65c605`

~~~~markdown
6. Can an untagged offset cross Rust, FFI, LSP, CLI, or a cache boundary?
~~~~

### SRC-EXP-L1738-335359831466

- Kind: `context`
- Source: `successor-expansion.md:1738-1738`
- Applicability: `CLI3`
- Exact text SHA-256: `3353598314666ddc350b7a09dda36f45a481dab58670851450c0c5ca35fb0f3f`

~~~~markdown
7. Can cancellation, overflow, ambiguity, or missing input become an admitted empty success?
~~~~

### SRC-EXP-L1739-F31A4D7EB034

- Kind: `context`
- Source: `successor-expansion.md:1739-1739`
- Applicability: `CLI3`
- Exact text SHA-256: `f31a4d7eb034fe6dfcad4bb8d4cda2d99495fde7f0d13cf60353d01348f2364e`

~~~~markdown
8. Can a Custom Element claim confuse declaration, registration, scope, framework component identity, and runtime reachability?
~~~~

### SRC-EXP-L1740-7BF519FC65E2

- Kind: `context`
- Source: `successor-expansion.md:1740-1740`
- Applicability: `CLI3`
- Exact text SHA-256: `7bf519fc65e29dfe8e8402d5e70ec3c9708cc24c97154d1f44ad69a3d10e2482`

~~~~markdown
9. Can a project profile select/create a TypeScript program or overwrite framework/TypeScript authority?
~~~~

### SRC-EXP-L1741-E909CDA6178F

- Kind: `requirement`
- Source: `successor-expansion.md:1741-1741`
- Applicability: `CLI3`
- Exact text SHA-256: `e909cda6178fdc2c98f6d00676b31d7913472baf05277fc34f7c6c4e48c9063f`

~~~~markdown
10. Can a skill generate or implement work without an exact accepted manifest, charter, authority digest, and independent review?
~~~~
