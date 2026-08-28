# TypeScript-product conformance contract

**Proposed by:** AMD-005. **Authority:** none until maintainer ratification.

Public API, TSC/TSX, and declaration products are judged by the exact TypeScript
domain governing the relevant Revision 11 operation, TypeScript compiler/API
observable behavior, ratified Verter product contracts, and independently authored
Verter-local regression fixtures. Framework language-tools are excluded.

The repository currently contains more than one exact TypeScript domain, including
workspace packages pinned to `6.0.3`, the root test/tool domain at `7.0.2`, the bundled
TSGO protocol domain `7.0.2`, and an exact native-preview build-tool package. These
are distinct owned domains, not a license to choose whichever output passes. BF1
records the owner and consumer for every exercised route; a domain change requires
its owning Revision 11 process.

Each case records compiler/API version, normalized compiler options, libraries,
module resolution inputs, virtual files, queried spans/symbols/types/diagnostics,
emitted declarations if requested, and stable observations. Acceptance covers
diagnostic code/category/location, assignability and exposed types, JSX/component
surface, declarations, source mappings, and direct/prepared/batch/project route
equivalence where those routes exist.

Source-local Vue macros are BV1-owned. Imported/project-aware macro information is a
closed typed demand emitted by BV1 and fulfilled by C3; C3 cannot replace Vue
semantics or code generation. Missing project information returns the typed state
specified by the capability cell, not a guessed language-tools-compatible result.

Fixtures cannot be copied or derived from language-tools or third-party repositories.
A difference from either is neither acceptance nor failure without an independent
TypeScript/Verter contract observation.
