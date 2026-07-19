//! tsgo toolchain provisioning — the SINGLE owner of tsgo binary discovery,
//! platform mapping, version policy, candidate validation, and the bundled
//! offline fallback contract.
//!
//! tsgo is the TypeScript 7 native compiler, distributed as the platform
//! binary `tsc[.exe]` inside the per-platform `@typescript/typescript-<os>-<arch>`
//! npm packages. Verter supports ONE version channel — see [`policy`] — and
//! resolves the engine through an ordered first-working resolver — see
//! `discovery` (slice 4) — replacing the historical divergent per-consumer
//! discovery implementations.
//!
//! Ownership boundary (ratified design): this module owns tsgo paths, platform
//! mapping, version policy, candidate provenance, validation, and the bundle
//! location. It does NOT own `TypeProvider`, rename/code-action behavior, LSP
//! provider routing, shared editor attachment, or tsserver process semantics —
//! those stay in `verter_type_runtime` / `verter_lsp` (the provider-feature
//! branch's lane).

pub mod policy;
