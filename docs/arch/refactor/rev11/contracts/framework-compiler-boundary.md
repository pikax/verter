# Framework compiler boundary and glossary

**Proposed by:** AMD-005. **Authority:** none until maintainer ratification.

## Product boundary

Verter owns SFC parsing, framework semantic analysis, framework-owned intermediate
models and product plans, generated client and server JavaScript, established public
API/TSC/declaration/diagnostic/CSS/mapping products, and correct generated-code
imports and topology. It does not implement, fork, bundle, replace, or ship Vue or
Svelte runtimes.

Official framework compilers are hermetic test oracles only. Official runtimes are
hermetic executors of generated JavaScript in tests only. Neither may be a production
dependency or fallback.

## Glossary

| term | exact meaning |
|---|---|
| compatibility domain | one immutable upstream source commit/tree plus the exact package closure used to test a claim |
| framework parser | the Vue-owned or Svelte-owned syntax/recovery frontend; never a universal framework AST |
| framework semantic model | a framework-local meaning model with no tagged cross-framework superclass |
| product plan | the minimum framework-local plan for one requested product/profile |
| structured emitter/edit plan | ordered owned output operations carrying source-space and placement meaning |
| compiler artifact set | one typed all-or-nothing result containing exactly the requested products |
| RuntimeClient | generated JavaScript intended to execute on the exact official client runtime |
| RuntimeServer | generated JavaScript intended to execute on the exact official server/SSR runtime |
| PublicApi | established TypeScript-visible component API product; not runtime code |
| Tsc | established TypeScript checking projection; not JavaScript runtime output |
| declaration | established declaration output and its TypeScript-observable behavior |
| compatibility family | Vue VDOM, Vue Vapor, Vue SSR, Svelte client, or Svelte server as separately claimed |
| capability cell | framework/domain/profile/product/options/route combination with one explicit disposition |
| route | public/default or internal invocation path; a route cannot expand semantic meaning |
| official case | immutable upstream test declaration/sample recorded in the official manifest |
| golden | expected oracle artifact generated only from the exact official pin |
| typed demand | a closed, framework-owned request for project information that codegen cannot source locally |
| projection-required | locally defined semantics are closed, but a project-aware provider must satisfy a typed demand |
| fail-closed | typed non-success produced before atomic publication, with no partial artifact |

## Ownership constraints

The only allowed top-level compiler flow is:

```text
SFC source -> framework parser -> framework semantic model
           -> requested product-specific plan
           -> structured emitter/edit plan -> atomic compiler artifact set
```

Within one framework, narrow staged IRs are allowed when their owner and lifetime are
explicit. Cross-framework universal ASTs, semantic hierarchies, runtime IRs, fact
bags, option bags, or Vue-based Svelte lowering are prohibited. No product may be
recovered by reparsing another generated product.

## Atomicity

Success publishes all and only requested artifacts. A refusal, diagnostic failure,
panic containment, link failure, or unsupported combination publishes none of the
JavaScript, PublicApi, Tsc, declaration, CSS, diagnostic map, or source map set. A
diagnostic embedded in a typed non-success is not a partially published diagnostic
product.
