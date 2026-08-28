# Conformance and golden contract

**Proposed by:** AMD-005. **Authority:** none until maintainer ratification.

## Case ledger

Every applicable official case has exactly one disposition:

- `imported`: exact official case is executed by the harness;
- `equivalent`: an independently named harness case proves the same official behavior
  and records the official source identity;
- `not_applicable`: the behavior is outside Verter's ratified product boundary;
- `unsupported_fail_closed`: the canonical request rejects it before publication; or
- `blocked`: not yet classified or satisfied.

Every row records domain, immutable source locator/object, profile axes, product,
disposition, reason, and evidence ID. A supported cell cannot retain `blocked`. An
enabled successful cell cannot retain a semantic known-divergence allowlist.

## Golden provenance

Expected goldens are generated only from the exact official pin in an isolated,
locked, offline install. A generation record binds source commit/tree, package-lock
digest, generator commit/tree, normalized options, environment, raw artifact digest,
normalizer version/digest, and normalized digest. Candidate Verter output is a
read-only input and can never update an expectation.

Goldens are immutable per domain. Review rejects missing provenance, lock drift,
network access, candidate-sourced expectations, or an output patch between compiler
and assertion.

## Coverage strategy

Small core axes are exhaustive: framework family, client/server where applicable,
development/production, source maps on/off, Vue VDOM/Vapor, Svelte runes/legacy where
applicable, script kind, and component/module product claims. Secondary options use
pairwise coverage plus explicit high-risk interactions (macros/types, scoped/slotted
CSS metadata, hydration, async, namespaces, custom elements, events/bindings,
components/slots, and server output). The manifest explains every omitted Cartesian
combination; uncontrolled full Cartesian generation is prohibited.

## Per-case acceptance

Where applicable, each successful case proves requested products, atomic publication,
fragment contract, assembled parse, real-package link, normalized structure,
helper/import/call topology, official-runtime execution, SSR, hydration, diagnostics,
mappings, TypeScript observations, route equivalence, zero unrequested work, and
locked performance gates. “Not applicable” must identify the product-boundary rule;
“unsupported fail-closed” must identify a typed request rejection test.
