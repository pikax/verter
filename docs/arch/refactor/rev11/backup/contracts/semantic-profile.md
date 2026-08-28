# Profile and Policy Classification Contract

**Status:** Normative identity-classification contract.

# 1. Distinct classes

| Class | Meaning | Examples | Reuse/key consequence |
|---|---|---|---|
| `TypeScriptSemanticProfileId` | interpretation/compatibility semantics | strictness, nullability, exact optional property behavior, module/resolution semantics, JSX/type-language rules, selected TypeScript compatibility family | semantic query, compile projection, and semantic facts |
| `OutputProfileId` | generated program semantics/shape | client/server target, dev/prod semantics, feature transforms, framework/compiler target | compile plan and generated artifact |
| `PresentationProfileId` | human-facing rendering only | display flags, path-display policy, diagnostic text locale/presentation version | rendered text/diagnostic materialization only |
| `SerializationProfileId` | wire/encoding contract | schema/domain, canonical encoding, graph export format, field policy | serialized bytes only unless decoding compatibility affects use |
| `ResultContractId` | observable complete-result requirement | operation, exactness, capability, unsupported policy, requested approximation, required mapping/diagnostic/serialization outcome | semantic flight/cache compatibility |
| `ExecutionPolicy` | waiter-local resource/scheduling limits | deadline, cancellation, priority, work/time/memory budget | never changes complete result identity; exhaustion is partial/failure |

A field belongs to the earliest class whose observable meaning it can change. It is never copied into every class “for safety.”

# 2. Closed semantic-profile schema

The implementation defines a canonical typed schema. Every behavior-affecting compiler option is classified in a reviewed table as:

- semantic;
- output;
- presentation;
- serialization;
- execution-only;
- irrelevant to the operation;
- unsupported.

Unknown fields or unsupported values fail closed. “Private fields as applicable” is not an acceptable profile definition.

# 3. Canonicalization

- normalize equivalent forms before hashing;
- domain-separate encodings by class and compatibility domain;
- include field tags and canonical value encoding;
- exclude host path strings, map iteration order, timestamps, random seeds, and process-global defaults;
- collision-sensitive IDs use full fingerprints or verified equality;
- canonical bytes and schema/domain identity are test vectors.

# 4. Cross-class rules

- presentation changes do not invalidate semantic facts or generated code when no presentation is requested;
- serialization changes do not rerun semantic computation when the typed result remains available;
- output changes do not silently change semantic interpretation;
- execution budget changes cannot produce a different value labeled `Complete`;
- a provider/framework/compiler compatibility change belongs to its named domain and is included wherever it affects meaning.

# 5. Required tests

- every public/config option is classified exactly once;
- semantically equivalent configurations yield equal canonical IDs;
- one-field semantic changes invalidate semantic reuse;
- presentation-only changes reuse semantic facts but rerender;
- serialization-only changes reuse typed results but re-encode;
- execution-policy changes do not change complete output bytes;
- unknown fields/values fail closed;
- native, prepared, managed, and WASM implementations use the same canonical test vectors.
