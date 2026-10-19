# Persistent fallthrough composition inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).
Rows cover the state this owner adds or changes while folding a child
component's fallthrough surface into its parent: the shared root chain, the
shared child evidence, and the opt-in composition work counts.

## verter_session_query

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `RootChain` persistent links (`FallthroughBranch.root_chain`) | Ordered root-step chain of every fallthrough branch: the carrier props projection reads its first and last step (each link shares the chain's terminal link, so the last step is one read), the native and protocol conversions read it in order; a parent prepends one step and shares its child's chain | REQUIRED | A link lives while any branch, resolution, cache candidate or held result shares it; dropped iteratively by its last holder | `analysis/component_meta` | always |
| `RootChain::links` link addresses | Component-meta result footprint, which charges a link shared by several branches once | REQUIRED-lifetime | One footprint measurement | `analysis/component_meta` | always |

## verter_session

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| Child evidence receipt in a parent's fallthrough fact list (`FactVersionRef::Receipt` over the child's own list) | Warm validation of the fallthrough node cache, the fallthrough mirror and the component-meta result: an edit to any hop below the owner invalidates the owner through the shared receipts; a child list reaching the parent's own facts (a cycle) enters flat so the owner-excluding observation filters still keep the owner's facts out | REQUIRED | The parent resolution and every candidate, mirror entry or held result sharing it; pinned on the process retention account from birth, claimed by the first retaining admission | `resolver_core/fallthrough` | always |
| `RootChain` footprint row | Component-meta result retention estimate | REQUIRED-lifetime | One footprint measurement | `component_meta_footprint` | always |
| `FallthroughCompositionWork` root-step and evidence-entry counts, `fallthrough_composition_work`, `reset_fallthrough_composition_work` | none; physical composition work behind the deep-chain growth checks: links a hop allocated before reaching its child's shared chain, and evidence entries written | OPTIONAL | This thread, until reset | `resolver_core/fallthrough` | `cfg(any(test, feature = "semantic-observe"))` |

Default production builds compile the composition counts away; recording has
no runtime enabled check.
