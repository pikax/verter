---
ruling_id: "NO-COMPAT-OR-LEGACY-CODE"
type: "maintainer-ruling"
date: "2026-08-21"
date_source: "stated"
binds: ["all blocks", "all crates", "all packages"]
source_file: "MAINTAINER-RULING-NO-COMPAT-OR-LEGACY-CODE.md"
summary: "Verter keeps NO legacy or compatibility code unless the maintainer explicitly states otherwise. It supports the latest version only; a user wanting older behaviour uses an older Verter version. This forbids compatibility shims, dual paths, deprecated-but-retained APIs, version-branching behaviour, and modules named or framed as compat/legacy. It does NOT forbid reproducing the CURRENT supported upstream version's observable behaviour — that is conformance, and conformance code must be named for what it is."
supersedes: []
superseded_by: []
contradicts: []
notes: "Issued on finding a `svelte_compat` module introduced in Track J. That module is in fact a validation-only reproduction of the currently-supported svelte@5.56.3 CSS parse control flow, needed to reproduce one upstream first-error race — conformance, not backward compatibility — so the naming is the violation there rather than the existence. The separate and sharper question it raises, whether a hand-rolled reproduction of upstream's reader is a second CSS parser under ONE-CSS-PARSER-PARSE-ONCE, is referred to the architecture authority rather than settled here."
---

# Maintainer Ruling — no compat, no legacy

**Status:** RATIFIED by the maintainer, 2026-08-21.

Recorded verbatim:

> we do not keep legacy or compat code unless explicitly stated by me and rules,
> we only strive to support the latest version, if users want old behaviour they
> just use a older verter version!

## The rule

Verter supports the **latest** version only. A user who wants older behaviour
uses an older Verter release. That is what releases are for.

Forbidden without an explicit maintainer statement:
- compatibility shims and adapters kept "for now";
- dual paths where a new one and an old one both live;
- deprecated-but-retained APIs;
- behaviour that branches on a version to preserve an older result;
- modules named or framed as `compat` / `legacy` / `back-compat`.

This reinforces the existing clean-cutover requirement: a superseded path is
deleted in the same change that replaces it, never left beside it.

## What this does NOT forbid

Reproducing the **currently supported** upstream version's observable behaviour
is CONFORMANCE, not compatibility. Matching `svelte@5.56.3`'s diagnostics, or
Vue's emitted output, is the product working correctly — not legacy support.

Such code must be **named for what it is**. `compat` in a name asserts a
backward-compatibility purpose this project does not have, and a reader cannot
tell a conformance oracle from a legacy shim by its name alone.

## Applied

The `svelte_compat` module introduced in Track J is a validation-only
reproduction of the supported Svelte version's CSS parse control flow. Its
existence is conformance; its NAME is the violation, and it is to be renamed to
say so.

Whether a hand-rolled reproduction of upstream's reader is permissible at all
under `ONE-CSS-PARSER-PARSE-ONCE` — the module's own documentation states it
cannot be expressed as a thin read over the shared token stream without
diverging from upstream's observable behaviour — is a separate architectural
question, referred to the architecture authority.
