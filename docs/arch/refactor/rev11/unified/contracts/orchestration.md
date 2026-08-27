# Orchestration contract

READY is derived on demand from validated receipts, conditional/external requirements, activation state, and ephemeral leases. A request for a non-READY node stops before mutation and asks the orchestrator for another READY block. Packets contain only one charter, direct predecessor receipt summaries, relevant clauses/profiles, citations, gates and conflict domains. The orchestrator schedules the whole READY frontier subject to conflict/resource leases; there is no giant mutable ledger to babysit.

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-COMP-L1752-B3AE47B17E53

- Kind: `context`
- Source: `compiler-proposal.md:1752-1752`
- Applicability: `CMP6`
- Exact text SHA-256: `b3ae47b17e5379e3465352fa77c48e24d5f0ea2cb79d661440b02ed62c1a4969`

~~~~markdown
# 10. Post-framework non-release convergence and future gates
~~~~

### SRC-ORCH-L616-89077BAC944F

- Kind: `context`
- Source: `orchestration-findings.md:616-616`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `89077bac944f2e9c57080fa2e9557974d4b661046ee6c03ca54aa82ebc707ead`

~~~~markdown
# 11. One canonical DAG, but not one giant release train
~~~~

### SRC-ORCH-L618-718533B408F9

- Kind: `context`
- Source: `orchestration-findings.md:618-618`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `718533b408f94b47d19f1469d9e9dbe7727addfc5bcf7da44cc1c133cb86bbbe`

~~~~markdown
Keep one canonical authority graph.
~~~~

### SRC-ORCH-L620-4D0A3A847FCB

- Kind: `context`
- Source: `orchestration-findings.md:620-620`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `4d0a3a847fcb8a8c8aa6a40d3e96d1b0c6f055e4ce2055e5af0ab9e9f36f9539`

~~~~markdown
However:
~~~~

### SRC-ORCH-L622-597FC0828D76

- Kind: `forbidden`
- Source: `orchestration-findings.md:622-622`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `597fc0828d7615e04e0f9af9e7ddfd8c93ccefd7b6f40c338f4c72727b200fc1`

~~~~markdown
> **One DAG must not mean one serialized train.**
~~~~

### SRC-ORCH-L624-5203C84924EC

- Kind: `context`
- Source: `orchestration-findings.md:624-624`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `5203c84924ecab9acfa11727e49afc563dce6c5955b8588e90570b611060265d`

~~~~markdown
The graph should represent many logical subtrains:
~~~~

### SRC-ORCH-L626-6CABCAECE0C8

- Kind: `context`
- Source: `orchestration-findings.md:626-639`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `6cabcaece0c88c81d3b830191264fcb6bac32a0c77fe243caa69576efe0bda76`

~~~~markdown
```text
Rev11 core
compiler bridge
compiler common
Vue compiler
Svelte compiler
CSS/style
TypeInfo
formatter
lint
CLI
future verticals
...
```
~~~~

### SRC-ORCH-L641-974134A5E575

- Kind: `acceptance`
- Source: `orchestration-findings.md:641-641`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `974134a5e5752e100bedc5f15327451d070f401fc088d3bb44cc3d2568c9405e`

~~~~markdown
A train should be metadata/grouping, not an acceptance boundary.
~~~~

### SRC-ORCH-L643-A66C7C58870F

- Kind: `context`
- Source: `orchestration-findings.md:643-643`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `a66c7c58870f4d3cecf9ef45527232aab72a347ba308c30e07b7312523e1e929`

~~~~markdown
Useful node metadata:
~~~~

### SRC-ORCH-L645-21B47E19E882

- Kind: `context`
- Source: `orchestration-findings.md:645-654`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `21b47e19e88229fafd5945bc9ae1e15d8b87728141b5bb6b0345d858d5848ed8`

~~~~markdown
```toml
id = "..."
train = "compiler.vue"
phase = "rev11" # or successor
product = "vue_compiler"
kind = "implementation"
conflict_domains = ["vue_semantics", "vue_compiler"]
resource_class = "rust-heavy"
release_gating = "none"
```
~~~~

### SRC-ORCH-L656-BD3FF2FABEF4

- Kind: `context`
- Source: `orchestration-findings.md:656-656`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `bd3ff2fabef436392ca3b90e7c064b795b0fdb73fe1d2f02cac2226a06817552`

~~~~markdown
The DAG should encode **correctness dependencies**.
~~~~

### SRC-ORCH-L658-52E08A20BAB3

- Kind: `context`
- Source: `orchestration-findings.md:658-658`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `52e08a20bab3f27c5e8165c4c6c420e179ca430ace3e0d8ff788ef4269bb971b`

~~~~markdown
It should not encode machine availability or scheduling convenience as fake dependency edges.
~~~~

### SRC-ORCH-L660-F52D711103D5

- Kind: `context`
- Source: `orchestration-findings.md:660-660`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`

~~~~markdown
---
~~~~

### SRC-ORCH-L684-CD9600941B0D

- Kind: `context`
- Source: `orchestration-findings.md:684-684`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `cd9600941b0ddeedc76144f72c0921b8ba50c2158b0da2d9da05b75caec1aa74`

~~~~markdown
# 13. Schedule the full READY frontier
~~~~

### SRC-ORCH-L686-A6CEC5DB297C

- Kind: `context`
- Source: `orchestration-findings.md:686-686`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `a6cec5db297c750ebd301a6461df91efc477432e02c40f4aa49c569d49f5fb63`

~~~~markdown
The orchestrator should stop thinking primarily in terms of:
~~~~

### SRC-ORCH-L688-B686368E1B56

- Kind: `context`
- Source: `orchestration-findings.md:688-690`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `b686368e1b5605aa1af8ca19640c55f3fe7353b75574e94b36ccbed590cb4396`

~~~~markdown
```text
what is the next block?
```
~~~~

### SRC-ORCH-L692-21CB2DAF2ADE

- Kind: `context`
- Source: `orchestration-findings.md:692-692`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `21cb2daf2ade8199e587f0725d82279eee398bc79210ba7c931e86363f44284b`

~~~~markdown
and instead compute:
~~~~

### SRC-ORCH-L694-E76D1B2FFB1F

- Kind: `context`
- Source: `orchestration-findings.md:694-696`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `e76d1b2ffb1fc8478a3e7df2b1d53c63f91b8ccf6a122a30370f61901bc172c5`

~~~~markdown
```text
READY = all DAG nodes whose authority predecessors are accepted
```
~~~~

### SRC-ORCH-L698-2C108915278C

- Kind: `context`
- Source: `orchestration-findings.md:698-698`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `2c108915278cf487647a0a102e9f11494d99567371e9a627fb6710d1a7110e5b`

~~~~markdown
Then schedule across the complete READY frontier according to:
~~~~

### SRC-ORCH-L700-E22EC10087FF

- Kind: `context`
- Source: `orchestration-findings.md:700-700`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `e22ec10087ff4f7781fadfe9bd35e7d0fdcc497061c94f9bd49169c7a25d54ac`

~~~~markdown
- machine availability;
~~~~

### SRC-ORCH-L701-D98C6DDB8301

- Kind: `context`
- Source: `orchestration-findings.md:701-701`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `d98c6ddb8301aa86099f7c38d60658ab320910c5f1c45cbedb5398aba3d48831`

~~~~markdown
- conflict domains;
~~~~

### SRC-ORCH-L702-D9D2E53EDA47

- Kind: `context`
- Source: `orchestration-findings.md:702-702`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `d9d2e53eda478d6a09e26a4d91d0337049050a32a0479c3c0c87e54b68ddbce1`

~~~~markdown
- model requirements;
~~~~

### SRC-ORCH-L703-21F9D32D6B01

- Kind: `context`
- Source: `orchestration-findings.md:703-703`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `21f9d32d6b01b80b935bcc41c203fda4714641bbd0c0225c03b829428bf556d2`

~~~~markdown
- resource class;
~~~~

### SRC-ORCH-L704-1AD660604303

- Kind: `context`
- Source: `orchestration-findings.md:704-704`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `1ad660604303ad3161c04f72fe798166f58355ea65cebc5b4ae5a250b3ebaea3`

~~~~markdown
- critical-path importance;
~~~~

### SRC-ORCH-L705-5D540D7C9E63

- Kind: `context`
- Source: `orchestration-findings.md:705-705`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `5d540d7c9e638d661e9feff735b932624092a73b4320c14ba2617ecba7fe82ef`

~~~~markdown
- fairness/age;
~~~~

### SRC-ORCH-L706-1870E6725AD9

- Kind: `context`
- Source: `orchestration-findings.md:706-706`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `1870e6725ad9ecbdccaa142701935fb48426659ebacc5cab75f9e6175ad7e7c3`

~~~~markdown
- expected integration conflict.
~~~~

### SRC-ORCH-L708-135DCFE9031F

- Kind: `context`
- Source: `orchestration-findings.md:708-708`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `135dcfe9031fe5cbd18d7e824f8bef8e510f312f8e295953ff31321d06df2631`

~~~~markdown
Machine constraints should not become DAG edges.
~~~~

### SRC-ORCH-L710-8596B59069F5

- Kind: `context`
- Source: `orchestration-findings.md:710-710`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `8596b59069f59c6b643e3d5650cec72f466152aa3069a8aeec88cd6e7b841b53`

~~~~markdown
Example:
~~~~

### SRC-ORCH-L712-0F8F150C5B62

- Kind: `context`
- Source: `orchestration-findings.md:712-718`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `0f8f150c5b62f77dde87e83eb7f937f25762df5d4ee1bbd84c9eaebb4b079ea6`

~~~~markdown
```text
READY:
  C3
  J2
  G2B
  formatter-lock
  compiler-contract
~~~~

### SRC-ORCH-L720-AF1435DB76D9

- Kind: `context`
- Source: `orchestration-findings.md:720-724`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `af1435db76d9b8e7e4fb16fc77bac1c77f02198890e270a8dfc2e46d283edbd8`

~~~~markdown
Machines:
  M1 rust-heavy
  M2 rust-heavy
  M3 docs/architecture
```
~~~~

### SRC-ORCH-L726-02EF2F99962F

- Kind: `context`
- Source: `orchestration-findings.md:726-726`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `02ef2f99962fb311b239d37942f1c0c2ec80292bbe4b29520d4f5899c3e4a1f5`

~~~~markdown
The scheduler assigns leases.
~~~~

### SRC-ORCH-L728-061BD062C6F8

- Kind: `context`
- Source: `orchestration-findings.md:728-728`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `061bd062c6f87f43b1fd0ecf4a572a0d62c871fb2dc158b124ca4646c6777815`

~~~~markdown
The DAG remains unchanged.
~~~~

### SRC-ORCH-L730-F52D711103D5

- Kind: `context`
- Source: `orchestration-findings.md:730-730`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`

~~~~markdown
---
~~~~

### SRC-ORCH-L732-A096F7592194

- Kind: `context`
- Source: `orchestration-findings.md:732-732`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `a096f7592194e581f01a7dafdc83aa350aa26471b868af8273aa7ec7e7645e3b`

~~~~markdown
# 14. Introduce conflict domains rather than over-serializing
~~~~

### SRC-ORCH-L734-0CB5B4434C6C

- Kind: `context`
- Source: `orchestration-findings.md:734-734`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `0cb5b4434c6c75911d51fbb75906dc256bb3bb5eafb503daabd37a7b6d01fe00`

~~~~markdown
Blocks should declare the subsystems whose simultaneous mutation is unsafe.
~~~~

### SRC-ORCH-L736-9EBF3E43DEAC

- Kind: `context`
- Source: `orchestration-findings.md:736-736`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `9ebf3e43deacf2cc0e9c7b70e5f7eb88b57fba467c8a4bc805b6825f5cc390ca`

~~~~markdown
For example:
~~~~

### SRC-ORCH-L738-79F2D1EC6958

- Kind: `context`
- Source: `orchestration-findings.md:738-743`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `79f2d1ec69585fac9b6670899f2e47f27fc5587b30c3904ae30fccfebf8506f0`

~~~~markdown
```toml
conflict_domains = [
  "resolver_core",
  "semantic_authority"
]
```
~~~~

### SRC-ORCH-L745-8C1D35B02E18

- Kind: `context`
- Source: `orchestration-findings.md:745-745`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `8c1d35b02e180f733df7fe8c3bf24653232e86778d38c02f6d56d4feb87b854c`

~~~~markdown
Two READY blocks with disjoint conflict domains can proceed concurrently.
~~~~

### SRC-ORCH-L747-3E23D3E67C70

- Kind: `context`
- Source: `orchestration-findings.md:747-747`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `3e23d3e67c70d27552e85066ec3cbb70cd8d1e8e262e313fe63454daea93ed2a`

~~~~markdown
Two blocks that both modify `semantic_authority` may need serialization even if there is no conceptual DAG dependency.
~~~~

### SRC-ORCH-L749-0789D501198A

- Kind: `context`
- Source: `orchestration-findings.md:749-749`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `0789d501198a27b26bf2a1c0369145cb33f2898a833ca06476d1192f1607fb3b`

~~~~markdown
This distinction prevents the DAG from becoming polluted with false ordering edges.
~~~~

### SRC-ORCH-L751-F52D711103D5

- Kind: `context`
- Source: `orchestration-findings.md:751-751`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`

~~~~markdown
---
~~~~

### SRC-ORCH-L753-AB32D5C95D96

- Kind: `context`
- Source: `orchestration-findings.md:753-753`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `ab32d5c95d9695b1e579bdd131c3e0cdb5afc8b3c9428a22e56844db8a0a2592`

~~~~markdown
# 15. Separate static authority, historical evidence, runtime state and derived state
~~~~

### SRC-ORCH-L755-90CD9852A427

- Kind: `context`
- Source: `orchestration-findings.md:755-755`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `90cd9852a4279e68279b0744260d0c821252bc96e483427361ebf8b5cbcf1422`

~~~~markdown
The current orchestration model carries too much mutable information in central state.
~~~~

### SRC-ORCH-L757-FCAD5535D21D

- Kind: `context`
- Source: `orchestration-findings.md:757-757`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `fcad5535d21dc5974f39b4fdc996cd4b5403ccdcc28dc5324ed7742a19f35852`

~~~~markdown
Move toward:
~~~~

### SRC-ORCH-L759-763E236AE74A

- Kind: `context`
- Source: `orchestration-findings.md:759-763`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `763e236ae74aea966728eb0a03dad4bf5c8235e7fe16863d95268b041f88dd7e`

~~~~markdown
```text
AUTHORITATIVE STATIC STATE
    DAG
    charters
    architecture decisions
~~~~

### SRC-ORCH-L765-83B1F67D49E1

- Kind: `acceptance`
- Source: `orchestration-findings.md:765-766`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `83b1f67d49e17a962ca778b2d3959905524345ba55495c8a1c5db742214b42ea`

~~~~markdown
AUTHORITATIVE HISTORICAL STATE
    immutable acceptance receipts
~~~~

### SRC-ORCH-L768-4B6EB7264EFE

- Kind: `context`
- Source: `orchestration-findings.md:768-773`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `4b6eb7264efeaa8ba24207a897d64cfc2f53bb0f18f77182a85c843690a0c333`

~~~~markdown
OPERATIONAL / EPHEMERAL STATE
    leases
    active machines
    worktree/ref
    heartbeat
    current implementation slice
~~~~

### SRC-ORCH-L775-7A7129AD28FE

- Kind: `context`
- Source: `orchestration-findings.md:775-777`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `7a7129ad28feb38bf874d33e4516c399f2b539bc05f6601de6d46d9a10171fcc`

~~~~markdown
DERIVED STATE
    generated status/program view
```
~~~~

### SRC-ORCH-L779-0956198306CB

- Kind: `context`
- Source: `orchestration-findings.md:779-779`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `0956198306cb36d4495b634d4fa4dbf2d12086325090296b8aacebf6b0800f6f`

~~~~markdown
Core rule:
~~~~

### SRC-ORCH-L781-CC53D68E85CD

- Kind: `forbidden`
- Source: `orchestration-findings.md:781-781`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `cc53d68e85cd777dac6b5eab769da28c44c7b48797b6bce6ad4d8742338fe686`

~~~~markdown
> **Derived state must not become another authority.**
~~~~

### SRC-ORCH-L783-F52D711103D5

- Kind: `context`
- Source: `orchestration-findings.md:783-783`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`

~~~~markdown
---
~~~~

### SRC-ORCH-L785-A1EB59F96764

- Kind: `context`
- Source: `orchestration-findings.md:785-785`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `a1eb59f9676422319cdfe83d970dc6b055dbcc01935475ea99571399cd1e5a84`

~~~~markdown
# 16. Replace central mutable ledger churn with immutable receipts
~~~~

### SRC-ORCH-L787-2A64F70EBBB6

- Kind: `context`
- Source: `orchestration-findings.md:787-787`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `2a64f70ebbb69856b3206927cb9c4ad485a35ea24e20422140fcc8d5e40dfc1f`

~~~~markdown
Do not continuously rewrite a giant `program-state.toml` with information that Git or immutable evidence can already prove.
~~~~

### SRC-ORCH-L789-5CC1BD2A9DA2

- Kind: `context`
- Source: `orchestration-findings.md:789-789`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `5cc1bd2a9da28910b4a8168bc999d6b4ff1463a874ef6cbcb86ed449a1cfb53b`

~~~~markdown
An accepted block could have a small receipt approximately like:
~~~~

### SRC-ORCH-L791-E6034DF11E7D

- Kind: `context`
- Source: `orchestration-findings.md:791-798`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `e6034df11e7d7b1eb8eaccdad34651fc6a4c19485d92c572d50fcebf18a0eaed`

~~~~markdown
```toml
schema = 2
block = "J2"
control_basis = "..."
candidate = "..."
accepted_integration_commit = "..."
charter = "docs/.../J2.md"
predecessors = ["J1"]
~~~~

### SRC-ORCH-L800-A41DA2B4DE86

- Kind: `context`
- Source: `orchestration-findings.md:800-804`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `a41da2b4de861bfa473524aef0dd798536d0e42dea64315013d7386b02168fe7`

~~~~markdown
reviews = [
  "evidence/J2/conformance.receipt",
  "evidence/J2/architecture.receipt",
  "evidence/J2/adversarial.receipt",
]
~~~~

### SRC-ORCH-L806-1CDDA2DDB3CD

- Kind: `context`
- Source: `orchestration-findings.md:806-808`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `1cdda2ddb3cdb5be896fdf33816f73a53260f0bc5f932ffcf2086e3b55cc3a25`

~~~~markdown
gate = "evidence/J2/gate.receipt"
decision = "accepted"
```
~~~~

### SRC-ORCH-L810-944816635D0B

- Kind: `context`
- Source: `orchestration-findings.md:810-810`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `944816635d0b09fb01a7c68a24930b8b46bcd2b63f8494ae964f5843250bb686`

~~~~markdown
Do not store redundant facts merely because they can be stored.
~~~~

### SRC-ORCH-L812-3F0A53663CCA

- Kind: `context`
- Source: `orchestration-findings.md:812-812`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `3f0a53663ccad7174e1701e2aca43659f9d5155327b709e41a3761947eb5ab2c`

~~~~markdown
Derive where possible:
~~~~

### SRC-ORCH-L814-F44F3B2CF808

- Kind: `context`
- Source: `orchestration-findings.md:814-814`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `f44f3b2cf8085191759402a9701e0186c204c02c4ea0f927d72a6289eb521f24`

~~~~markdown
- candidate tree from candidate SHA;
~~~~

### SRC-ORCH-L815-C9B550B1BBFB

- Kind: `context`
- Source: `orchestration-findings.md:815-815`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `c9b550b1bbfbff1fdef61a8240a118779a88c49b87474b7d82519d151997f5ee`

~~~~markdown
- accepted tree from integration SHA;
~~~~

### SRC-ORCH-L816-2FD2ED10D6BB

- Kind: `context`
- Source: `orchestration-findings.md:816-816`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `2fd2ed10d6bba07979969150bc577ae5b613aae23207f982c4b409fddf375ba0`

~~~~markdown
- ancestry from Git;
~~~~

### SRC-ORCH-L817-D7E4E2C85226

- Kind: `context`
- Source: `orchestration-findings.md:817-817`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `d7e4e2c85226b18e8f34835b5782fbb8c4447d07ea3510e939a3717336c36af3`

~~~~markdown
- charter content from `control_basis + path`;
~~~~

### SRC-ORCH-L818-9D18C79D6EDA

- Kind: `context`
- Source: `orchestration-findings.md:818-818`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `9d18c79d6eda27f92830a8aceb3648fef4506fa29095f1bcf89f400b6612f21f`

~~~~markdown
- DAG content from `control_basis`;
~~~~

### SRC-ORCH-L819-D389DE371C6A

- Kind: `context`
- Source: `orchestration-findings.md:819-819`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `d389de371c6aa714ecf7f9b3479b0d5740c31b87d682c0de4edca569c5be45cd`

~~~~markdown
- review identity from immutable review receipts;
~~~~

### SRC-ORCH-L820-DDBF2EE219B6

- Kind: `context`
- Source: `orchestration-findings.md:820-820`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `ddbf2ee219b6805f27f7731380823a496cc7d2c6c1f08f857b9adbafb9295e26`

~~~~markdown
- code/tree equivalence mechanically.
~~~~

### SRC-ORCH-L822-012480A0C256

- Kind: `requirement`
- Source: `orchestration-findings.md:822-822`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `012480a0c256beec109ee45944229a80a825b8161233a5a328a894ccb1020ca8`

~~~~markdown
Persist only facts that cannot be reconstructed safely.
~~~~

### SRC-ORCH-L824-F52D711103D5

- Kind: `context`
- Source: `orchestration-findings.md:824-824`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`

~~~~markdown
---
~~~~

### SRC-ORCH-L826-1F5CC89B4E44

- Kind: `context`
- Source: `orchestration-findings.md:826-826`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `1f5cc89b4e44b2dea07862397ae60de8453dc947a14232f6dd1a23ba121538af`

~~~~markdown
# 17. Runtime leases should not mutate governance
~~~~

### SRC-ORCH-L828-33F329AD3D6A

- Kind: `context`
- Source: `orchestration-findings.md:828-828`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `33f329ad3d6ab5339ba1fdc2a9360bcb3b944885440e764bb93abc742d99add4`

~~~~markdown
An active agent should obtain operational state such as:
~~~~

### SRC-ORCH-L830-5666AC7505F7

- Kind: `context`
- Source: `orchestration-findings.md:830-838`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `5666ac7505f7476aeeee4f6fbb14ae17ac1c8ffcd7b716a527668f8e6e8a53a6`

~~~~markdown
```text
block
branch/ref
control basis
machine
lease epoch
heartbeat
expiry
```
~~~~

### SRC-ORCH-L840-ECC2C5C2A427

- Kind: `context`
- Source: `orchestration-findings.md:840-840`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `ecc2c5c2a427d44fa0dd718372f466f03c8b0ae12a2f423273004b7ee1a86636`

~~~~markdown
That should not require governance commits every time an agent:
~~~~

### SRC-ORCH-L842-47AEBA1E5614

- Kind: `context`
- Source: `orchestration-findings.md:842-842`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `47aeba1e5614d7dd222d9a24a34fa55e3c2d739589c8b0f2acb2d1d015219893`

~~~~markdown
- starts;
~~~~

### SRC-ORCH-L843-4A5F1B392772

- Kind: `context`
- Source: `orchestration-findings.md:843-843`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `4a5f1b3927726e0df426ed69d46c33ce70337e93eecb69ab922daa84f8145b58`

~~~~markdown
- stops;
~~~~

### SRC-ORCH-L844-050DE0C536FB

- Kind: `context`
- Source: `orchestration-findings.md:844-844`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `050de0c536fba1302b670cc2ca2945b9a9968b74d0583d468b2a404a90c518f4`

~~~~markdown
- changes implementation slice;
~~~~

### SRC-ORCH-L845-70F268783828

- Kind: `context`
- Source: `orchestration-findings.md:845-845`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `70f2687838286919ff9a5304c58bcaffd1df0eccba307d4e41a1dd88c5c99600`

~~~~markdown
- clears context;
~~~~

### SRC-ORCH-L846-E586EA8488F3

- Kind: `context`
- Source: `orchestration-findings.md:846-846`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `e586ea8488f3028e8c201781c4e4bbd3796188409f73d48f0f394fa4936c7d95`

~~~~markdown
- moves between machines;
~~~~

### SRC-ORCH-L847-7680D3F53CCB

- Kind: `context`
- Source: `orchestration-findings.md:847-847`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `7680d3f53ccbfe7c9038f76ae1ed2443bd4c1afbbaa3106e380e033c1cdbd778`

~~~~markdown
- resumes.
~~~~

### SRC-ORCH-L849-3A6E0751E767

- Kind: `context`
- Source: `orchestration-findings.md:849-849`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `3a6e0751e76773fda8f8f41da57ebb8bcb8f39252127b2eade2f5d8bcae81fc1`

~~~~markdown
This state is ephemeral.
~~~~

### SRC-ORCH-L851-024E947C5AF5

- Kind: `acceptance`
- Source: `orchestration-findings.md:851-851`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `024e947c5af51eb49834c028d9e5d093b5e395f76cf2a7d3b41ba57950108791`

~~~~markdown
Only acceptance creates permanent historical evidence.
~~~~

### SRC-ORCH-L853-F52D711103D5

- Kind: `context`
- Source: `orchestration-findings.md:853-853`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`

~~~~markdown
---
~~~~

### SRC-ORCH-L892-D31A8FEE88F5

- Kind: `context`
- Source: `orchestration-findings.md:892-892`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `d31a8fee88f5db13a26c09d2d8ea53c88273b7940a1fce2f0f8e64adf8a60b2c`

~~~~markdown
# 19. Keep `program/architecture-lock` as canonical integration
~~~~

### SRC-ORCH-L894-95186B005F10

- Kind: `context`
- Source: `orchestration-findings.md:894-894`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `95186b005f10c4d9d5bcbe1cb50e6ffd5b0db51053372e4829bfc5efc495c254`

~~~~markdown
This is the corrected branch policy.
~~~~

### SRC-ORCH-L896-14111027B370

- Kind: `context`
- Source: `orchestration-findings.md:896-896`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `14111027b370120a0061ce52922b44f7dd5b827851636a4e6e61c501954beb48`

~~~~markdown
Recommended topology:
~~~~

### SRC-ORCH-L898-98C53FA424A3

- Kind: `context`
- Source: `orchestration-findings.md:898-905`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `98c53fa424a3f822889a6caadf8e5184bc9d2eb920b018258d5c285c3d1249b4`

~~~~markdown
```text
                    canonical authority
                program/architecture-lock
                         ↑
                    accepted merges
                  ↗      ↑      ↖
              block/C2 block/J2 block/H2A
```
~~~~

### SRC-ORCH-L907-AC051D0ED531

- Kind: `context`
- Source: `orchestration-findings.md:907-907`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `ac051d0ed531b0d5853eea72883b1d581891570ccc8322ba6a52f8d598f4ed17`

~~~~markdown
Independent train branches may exist:
~~~~

### SRC-ORCH-L909-AAD1AEC06866

- Kind: `context`
- Source: `orchestration-findings.md:909-914`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `aad1aec06866c1a70bac32f0eb27907d43e88dc9863e81dae84964f4a4781ec1`

~~~~markdown
```text
train/compiler
train/css
train/typeinfo
...
```
~~~~

### SRC-ORCH-L916-1914F1177A12

- Kind: `context`
- Source: `orchestration-findings.md:916-916`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `1914f1177a12d891b209efb7aa413b7eb6d2000dfefe2347cb90f0b84c7f5098`

~~~~markdown
but their accepted units ultimately merge into `program/architecture-lock`.
~~~~

### SRC-ORCH-L918-7A478E641B2F

- Kind: `context`
- Source: `orchestration-findings.md:918-918`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `7a478e641b2f849d135ae67f4194189a934ad8c503130a79ce3c543a532e6761`

~~~~markdown
Do **not** make `architecture-lock` consume a code/product branch as its upstream authority.
~~~~

### SRC-ORCH-L920-5EE321D38304

- Kind: `context`
- Source: `orchestration-findings.md:920-920`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `5ee321d3830407f612c731e4c87e0274f219f23aaa4c17cbdc0450276c06de63`

~~~~markdown
If a clean `refactor/product-branch` remains useful, treat it as something like:
~~~~

### SRC-ORCH-L922-96455414BFE5

- Kind: `context`
- Source: `orchestration-findings.md:922-928`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `96455414bfe58482b01e9f8acd24e5e65dc72e64271d9399b231243ac4bfbbd0`

~~~~markdown
```text
program/architecture-lock
         │
         │ accepted code projection / cherry-pick / generated sync
         ▼
refactor/product-branch
```
~~~~

### SRC-ORCH-L930-F287AFE25056

- Kind: `context`
- Source: `orchestration-findings.md:930-930`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `f287afe25056f8dace285ec5c94c93b1c028a3b5ae294649973de041d2fd64db`

~~~~markdown
It is a clean derivative/product history, not the canonical program authority.
~~~~

### SRC-ORCH-L932-361554DB4479

- Kind: `requirement`
- Source: `orchestration-findings.md:932-932`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `361554db4479858fcd9f8950ed015aa7be92ffbf5634f2ef010fc9ccf5ccc1b3`

~~~~markdown
The exact mechanics of maintaining that derivative branch should be planned separately so it does not reintroduce SHA/ledger busywork.
~~~~

### SRC-ORCH-L934-F52D711103D5

- Kind: `context`
- Source: `orchestration-findings.md:934-934`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`

~~~~markdown
---
~~~~

### SRC-ORCH-L936-89563588DA3D

- Kind: `requirement`
- Source: `orchestration-findings.md:936-936`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `89563588da3d46e79a541f0764db561b93379a65bc956f8763bd1ff5075b9b7d`

~~~~markdown
# 20. Prefer exact candidate preservation through merge commits
~~~~

### SRC-ORCH-L938-D8A58C406C5A

- Kind: `requirement`
- Source: `orchestration-findings.md:938-938`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `d8a58c406c5a159cd9535c88fe532a1f3ec440419a13b8d383448ca33095d6b2`

~~~~markdown
Once a candidate has completed final review, preserve it.
~~~~

### SRC-ORCH-L940-BD1DBB716418

- Kind: `context`
- Source: `orchestration-findings.md:940-940`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `bd1dbb71641813ca5854b346620d1dc259444123ed957f00393505ac99711ce7`

~~~~markdown
If architecture-lock advances while another candidate is under review:
~~~~

### SRC-ORCH-L942-AA3462427E60

- Kind: `context`
- Source: `orchestration-findings.md:942-942`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `aa3462427e6085ef4872b6977e91a9bf42661691a78b30aeda72721d9e480ace`

~~~~markdown
### No conflict
~~~~

### SRC-ORCH-L944-F31427AB9EBA

- Kind: `requirement`
- Source: `orchestration-findings.md:944-944`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `f31427ab9eba7bee2073999b72f03e6efc71ce0e6c6ab8f69069d82473208e9f`

~~~~markdown
Merge the frozen candidate as an exact parent of a new integration commit.
~~~~

### SRC-ORCH-L946-771B0E2F780D

- Kind: `context`
- Source: `orchestration-findings.md:946-946`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `771b0e2f780d1fcd7ed6bf710e390804a7ef0d766612601a0431311f7e9a1f08`

~~~~markdown
Do not rewrite the reviewed candidate purely to retain artificial linear history.
~~~~

### SRC-ORCH-L948-533D4612BA18

- Kind: `context`
- Source: `orchestration-findings.md:948-948`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `533d4612ba182bf3de6099302b09a0414b04b7666da65227ed42289fe180e554`

~~~~markdown
### Conflict
~~~~

### SRC-ORCH-L950-B3D071D8C8EC

- Kind: `context`
- Source: `orchestration-findings.md:950-950`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `b3d071d8c8ec471528b768b69f568c967aec0999f811756592d59eddba9dba8a`

~~~~markdown
Do not let the landing orchestrator creatively resolve significant conflicts.
~~~~

### SRC-ORCH-L952-31EB99E10E51

- Kind: `context`
- Source: `orchestration-findings.md:952-952`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `31eb99e10e512128e014e144569731455346acf99bd510f695bab0ba23dab98e`

~~~~markdown
Return the block to implementation:
~~~~

### SRC-ORCH-L954-ADA0CAE66314

- Kind: `context`
- Source: `orchestration-findings.md:954-959`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `ada0cae66314a6e32f40dd9696793969d3734839f5a8d757b4b4db9003c4075d`

~~~~markdown
```text
update basis
resolve conflict
produce new candidate
re-run affected validation
```
~~~~

### SRC-ORCH-L961-746FA85A78AF

- Kind: `requirement`
- Source: `orchestration-findings.md:961-961`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `746fa85a78afbbda49649a2074ee10648475d45876a5533d9d0f8f24b67c2d6b`

~~~~markdown
This preserves the meaning of exact-candidate review.
~~~~

### SRC-ORCH-L963-F52D711103D5

- Kind: `context`
- Source: `orchestration-findings.md:963-963`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`

~~~~markdown
---
~~~~

### SRC-ORCH-L965-E11E4AC25EC1

- Kind: `context`
- Source: `orchestration-findings.md:965-965`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `e11e4ac25ec1983749145bdac45d44b99d8691e9db6ed71046a4d641c6cbd2d8`

~~~~markdown
# 21. Distinguish candidate identity from integration identity
~~~~

### SRC-ORCH-L967-97F9B82C034F

- Kind: `acceptance`
- Source: `orchestration-findings.md:967-967`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `97f9b82c034f1f1c5ac8f5eb42ee9a681f94a5387f005caeba5a7cb4d3838bde`

~~~~markdown
An acceptance receipt should distinguish:
~~~~

### SRC-ORCH-L969-F1A0A04ED407

- Kind: `context`
- Source: `orchestration-findings.md:969-973`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `f1a0a04ed407b2f0e1783fe6589341d4bb889c352cbf2f5345834cb7efe0a847`

~~~~markdown
```text
candidate_sha
integration_sha
control/receipt_sha
```
~~~~

### SRC-ORCH-L975-C37574728E05

- Kind: `context`
- Source: `orchestration-findings.md:975-975`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `c37574728e05e39c7152dddceecc73c62737bcbfd3f82fa756941eae4d3fab2c`

~~~~markdown
These are different concepts.
~~~~

### SRC-ORCH-L977-8FABB6CF5DA3

- Kind: `context`
- Source: `orchestration-findings.md:977-977`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `8fabb6cf5da332566c31466df3693880951047b78d1728d994bd6acca0b41468`

~~~~markdown
`candidate_sha`:
~~~~

### SRC-ORCH-L979-C3A6A21B9291

- Kind: `requirement`
- Source: `orchestration-findings.md:979-979`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `c3a6a21b9291becc3eeae8689723da0f5fd135207e3d38c6e9a322322a49fec2`

~~~~markdown
> exact implementation reviewed.
~~~~

### SRC-ORCH-L981-79C0A07400D1

- Kind: `context`
- Source: `orchestration-findings.md:981-981`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `79c0a07400d142feac63132f24f387b7df75f8905f96005e09c83256abc39e9a`

~~~~markdown
`integration_sha`:
~~~~

### SRC-ORCH-L983-E764CF019F54

- Kind: `context`
- Source: `orchestration-findings.md:983-983`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `e764cf019f5473fa9303b639aa050ab25bfa4a01c9bb5c3320b8bf8c538c222d`

~~~~markdown
> commit on `program/architecture-lock` containing that candidate in the cumulative accepted tree.
~~~~

### SRC-ORCH-L985-E76DB07B4F20

- Kind: `context`
- Source: `orchestration-findings.md:985-985`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `e76db07b4f203802139e299ccb64f0d4610b5f2eafee964c960a3bb286facc4e`

~~~~markdown
`receipt/control_sha`:
~~~~

### SRC-ORCH-L987-6A4790AE5B53

- Kind: `context`
- Source: `orchestration-findings.md:987-987`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `6a4790ae5b53bc4750b6242c9aa04549a3482c82d95afd7b2a112bd0ba4c07aa`

~~~~markdown
> optional tiny subsequent control-state/receipt commit.
~~~~

### SRC-ORCH-L989-0EA2718C763F

- Kind: `context`
- Source: `orchestration-findings.md:989-989`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `0ea2718c763faf3968bafd6e134548f95d6fa7d83711c8a43d12984e467192b3`

~~~~markdown
The invariant can require:
~~~~

### SRC-ORCH-L991-0CFB1412CBB5

- Kind: `context`
- Source: `orchestration-findings.md:991-993`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `0cfb1412cbb510ad4a0e23094c74767a7516e9cd10fdc630794f4b5e0489e864`

~~~~markdown
```text
candidate_sha is ancestor/parent of integration_sha
```
~~~~

### SRC-ORCH-L995-B542CFDF1D80

- Kind: `requirement`
- Source: `orchestration-findings.md:995-995`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `b542cfdf1d80e65e646c7ab0803ec44b4b61410d96ac33bf068c22aff6a27776`

~~~~markdown
rather than pretending all three identities must be identical.
~~~~

### SRC-ORCH-L997-F52D711103D5

- Kind: `context`
- Source: `orchestration-findings.md:997-997`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`

~~~~markdown
---
~~~~

### SRC-ORCH-L999-31B4750D709E

- Kind: `context`
- Source: `orchestration-findings.md:999-999`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `31b4750d709e797f2055fedf6a5dcb1996ec660f35282b5b23137a86e2420993`

~~~~markdown
# 22. Integration needs its own semantic safety check
~~~~

### SRC-ORCH-L1001-390A25757D23

- Kind: `context`
- Source: `orchestration-findings.md:1001-1001`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `390a25757d23a278689ae5bb91673342dd5f0bd2d5090342351ea8a03a58587b`

~~~~markdown
A conflict-free Git merge does not guarantee semantic compatibility.
~~~~

### SRC-ORCH-L1003-5B4AFA0BA728

- Kind: `context`
- Source: `orchestration-findings.md:1003-1003`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `5b4afa0ba728bb8c074ae08a93bddaa81060144b28d476f40d14982ccce023e8`

~~~~markdown
Therefore after combining independently accepted candidates, run an integration gate appropriate to the touched conflict domains.
~~~~

### SRC-ORCH-L1005-9217670C254C

- Kind: `context`
- Source: `orchestration-findings.md:1005-1005`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `9217670c254cb269147bd0bc7290dfb677e023ebc50c4de19809bde1a6210647`

~~~~markdown
It does not necessarily need to rerun every expensive block-specific test.
~~~~

### SRC-ORCH-L1007-F90A0FE2093D

- Kind: `context`
- Source: `orchestration-findings.md:1007-1007`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `f90a0fe2093d6a3a57fbc0a765b83f8648600b160c87ff8497277dd11f83b2ff`

~~~~markdown
Think:
~~~~

### SRC-ORCH-L1009-2F696ADD730B

- Kind: `acceptance`
- Source: `orchestration-findings.md:1009-1013`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `2f696add730b7dcb7054532414e7b3ebec92ed43e730225402dfb97b0fa76f9d`

~~~~markdown
```text
block-specific acceptance gate
        +
cross-block integration gate
```
~~~~

### SRC-ORCH-L1015-ABFAECFAF38C

- Kind: `context`
- Source: `orchestration-findings.md:1015-1015`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `abfaecfaf38cd7e27acf2204cf5de35b940809be270ef6e815df08947b5aeb20`

~~~~markdown
The latter checks what could have changed because of concurrent integration.
~~~~

### SRC-ORCH-L1017-F52D711103D5

- Kind: `context`
- Source: `orchestration-findings.md:1017-1017`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`

~~~~markdown
---
~~~~

### SRC-ORCH-L1019-2330412FC4E5

- Kind: `context`
- Source: `orchestration-findings.md:1019-1019`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `2330412fc4e554e0e2ca75205efaedc2ddb9e23ed81fdb30341f838d8bf43b5a`

~~~~markdown
# 23. Avoid landing-time ledger work becoming the critical path
~~~~

### SRC-ORCH-L1021-A3E0FB72BD48

- Kind: `context`
- Source: `orchestration-findings.md:1021-1021`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `a3e0fb72bd48f2a8c4e3f7d0044b7271de7e2b8fca7b491fd33ddc007b23df83`

~~~~markdown
The landing path should be short:
~~~~

### SRC-ORCH-L1023-8425307C16CA

- Kind: `context`
- Source: `orchestration-findings.md:1023-1033`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `8425307c16cada1f8d4597fc14fdffd7f7b7745e0d34106f1a692e0ea69fb55a`

~~~~markdown
```text
candidate accepted
      ↓
integration compatibility check
      ↓
merge into architecture-lock
      ↓
tiny immutable receipt
      ↓
READY frontier recomputed
```
~~~~

### SRC-ORCH-L1035-46961CDE2D61

- Kind: `context`
- Source: `orchestration-findings.md:1035-1035`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `46961cde2d61623a9e7dff42aa3e0eb0134aec4414a659ea4cf8170d6e748e5a`

~~~~markdown
Avoid:
~~~~

### SRC-ORCH-L1037-D93E30A9AA73

- Kind: `context`
- Source: `orchestration-findings.md:1037-1043`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `d93e30a9aa73bdb50565bff3aaffcb0352eb212e912d7754edbf7eaa59f791b1`

~~~~markdown
```text
rewrite several central docs
recalculate hand-maintained SHAs
change duplicated state tables
repair generated-but-manually-edited summaries
rerun document ratification
```
~~~~

### SRC-ORCH-L1045-8A0305F31FEB

- Kind: `context`
- Source: `orchestration-findings.md:1045-1045`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `8a0305f31feb116d501a041b52be18b3d01988ab12f88d8b7402deb0c7755aac`

~~~~markdown
for ordinary accepted blocks.
~~~~

### SRC-ORCH-L1047-F52D711103D5

- Kind: `context`
- Source: `orchestration-findings.md:1047-1047`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`

~~~~markdown
---
~~~~

### SRC-ORCH-L1344-A75B1C4A89BB

- Kind: `context`
- Source: `orchestration-findings.md:1344-1344`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `a75b1c4a89bbac6fc6da70fc1918ab21adcbd8110fe9d1c19872b19f42f97327`

~~~~markdown
# 34. The Compiler proposal already demonstrates better execution decomposition
~~~~

### SRC-ORCH-L1346-7C1ECB6E9984

- Kind: `context`
- Source: `orchestration-findings.md:1346-1346`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `7c1ecb6e9984b76710e3096a1122e5cc362802d93e037236266a97c7d24fa6cb`

~~~~markdown
The new compiler architecture proposal is structurally healthier than C1/J1.
~~~~

### SRC-ORCH-L1348-F9EAD8044284

- Kind: `context`
- Source: `orchestration-findings.md:1348-1348`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `f9ead804428420cd3e553f962950e1de47f509bd3d45e1ca2d223a5b42faea19`

~~~~markdown
It separates common work into nodes such as:
~~~~

### SRC-ORCH-L1350-BC190FBEE92D

- Kind: `context`
- Source: `orchestration-findings.md:1350-1357`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `bc190fbee92d8716749239d58078c639ddfe0b337a10641ff43e59a9b87d52ae`

~~~~markdown
```text
CMP0 request/policy/identity
CMP1 demand + semantic admission
CMP2 data-oriented structure
CMP3 target planning
CMP4 emission/artifacts
CMP5 convergence
```
~~~~

### SRC-ORCH-L1359-8B21781739A7

- Kind: `context`
- Source: `orchestration-findings.md:1359-1359`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `8b21781739a7fd693bdf5d34be06e64c40bf28a0ae2931f2b586789a36609612`

~~~~markdown
and then creates independent Vue and Svelte compiler trains.
~~~~

### SRC-ORCH-L1361-18A05F5BA46D

- Kind: `context`
- Source: `orchestration-findings.md:1361-1361`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `18a05f5ba46db78802fc2a8f2a22f477ca488ac2de420bb195ca0910ad7b7cb5`

~~~~markdown
That should be treated as a useful template:
~~~~

### SRC-ORCH-L1363-B2B4DBFCC819

- Kind: `context`
- Source: `orchestration-findings.md:1363-1363`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `b2b4dbfcc819d630488592120f6265e136c3f31fc17834bbeff84ff6d953d8b0`

~~~~markdown
> ambitious architecture can be decomposed without weakening it.
~~~~

### SRC-ORCH-L1365-AF2BA00870E0

- Kind: `context`
- Source: `orchestration-findings.md:1365-1365`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `af2ba00870e0f1f9a022693ac1e01516a51c2822f8a08d61099bfd3c3426646d`

~~~~markdown
The proposed bounded bridge around:
~~~~

### SRC-ORCH-L1367-C6C6711185F9

- Kind: `context`
- Source: `orchestration-findings.md:1367-1377`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `c6c6711185f961730ce0e0da6951171a559bb920c75f5fb4fb129489b2b3a6f4`

~~~~markdown
```text
C1
 ↓
CCA0
 ↓
CCA1
 ↓
CCA2
 ↓
C2
```
~~~~

### SRC-ORCH-L1379-AEC97DC559AA

- Kind: `context`
- Source: `orchestration-findings.md:1379-1379`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `aec97dc559aa4916a9f5cdbb35960264a6c4918e81b4278b1798afba89612a91`

~~~~markdown
is also preferable to injecting the entire future compiler architecture into C2.
~~~~

### SRC-ORCH-L1381-5738962EDCB6

- Kind: `context`
- Source: `orchestration-findings.md:1381-1381`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `5738962edcb6b12245a120dfb616e02bb41e08e14dfde9c131a91343f1999664`

~~~~markdown
Keep C2 bounded.
~~~~

### SRC-ORCH-L1383-F52D711103D5

- Kind: `context`
- Source: `orchestration-findings.md:1383-1383`
- Applicability: `BR0`, `ORC0`
- Exact text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`

~~~~markdown
---
~~~~

### SRC-EXP-L1669-C23D0133ECFE

- Kind: `context`
- Source: `successor-expansion.md:1669-1669`
- Applicability: `CLI3`
- Exact text SHA-256: `c23d0133ecfe03940f6d37decc04e5139e73e386336bec82f68c387bd92b0f1d`

~~~~markdown
### 15.6 Non-active horizontal semantics ledger
~~~~

### SRC-EXP-L1671-281415DAE3E4

- Kind: `context`
- Source: `successor-expansion.md:1671-1671`
- Applicability: `CLI3`
- Exact text SHA-256: `281415dae3e4d81ecd68f9846b5e1a4330e50919516ed505961cd1d5654f6d6f`

~~~~markdown
After the architecture and one full new vertical are proven, prioritization should compare new framework work against horizontal semantics that benefit several verticals at once:
~~~~

### SRC-EXP-L1673-1541AD7A4CCE

- Kind: `context`
- Source: `successor-expansion.md:1673-1673`
- Applicability: `CLI3`
- Exact text SHA-256: `1541ad7a4cce8fdd170b77ec1f6215cda11583ad9a67298a02120df0de1ddd7e`

~~~~markdown
- CSS Modules, Sass/SCSS/Less semantic references, custom properties, and later evidence-gated utility-framework semantics;
~~~~

### SRC-EXP-L1674-EEA0DCA83B30

- Kind: `context`
- Source: `successor-expansion.md:1674-1674`
- Applicability: `CLI3`
- Exact text SHA-256: `eea0dca83b30c883b81bf6a8637fdcea11ea36719ec23b0d7ec86585559d6533`

~~~~markdown
- Vite/source-module facts such as aliases, assets, query imports, `import.meta.glob`, and environment typing, without bundler/HMR ownership;
~~~~

### SRC-EXP-L1675-D89AB7C50DC9

- Kind: `context`
- Source: `successor-expansion.md:1675-1675`
- Applicability: `CLI3`
- Exact text SHA-256: `d89ab7c50dc98ce6f3e8f7ff9aa42823b6dedfbd05719868b19c2c8f341fe9c6`

~~~~markdown
- JSON/JSONC/YAML and statically captured configuration projections, without executable configuration in Rust/WASM;
~~~~

### SRC-EXP-L1676-2A0D33DCADDB

- Kind: `context`
- Source: `successor-expansion.md:1676-1676`
- Applicability: `CLI3`
- Exact text SHA-256: `2a0d33dcaddb84b6601f525fc063a94ac156549dfce8791403fc75b88b697502`

~~~~markdown
- package exports/imports/workspaces and monorepo cross-package component relationships.
~~~~

### SRC-EXP-L1678-BF3A33A10B21

- Kind: `context`
- Source: `successor-expansion.md:1678-1678`
- Applicability: `CLI3`
- Exact text SHA-256: `bf3a33a10b21a90d8ec8149d689b9607300c6b85162d6c1142b9b5aca549f2b0`

~~~~markdown
These are portfolio records, not active DAG nodes or hidden vertical prerequisites. Each needs its own authority/reuse dossier and may be selected ahead of a lower-value framework when measured cross-vertical unlock exceeds the next vertical score.
~~~~
