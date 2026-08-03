# Framework public component contract

Component-meta response schema version 6 publishes a framework-neutral public
component contract at full-response tag 26. Every produced Vue or Svelte
declaration projection carries a mandatory `ComponentContractAvailability`;
absence is not representable.

## Availability

`Supported` contains one `ComponentPublicContract`. `Unsupported` fails closed
with the adapter identity, a closed reason, producer-owned typed diagnostics,
and the exact failed output lane or publication surface when applicable.
Output-materialization failure never suppresses an otherwise valid declaration
or becomes a supported-empty contract.

Supported contracts carry aggregate exactness, typed degradation records, and
component-meta-output provenance. Props preserve source order, optionality,
defaults, and A1 publications. Same-name event rows are grouped into
source-ordered overloads with structured names, optional/rest flags, parameter
types, return types, and derived handler shapes. Slots preserve source-ordered
scoped bindings and meaningful typed returns.

Callable event producers carry the return as a producer-owned
`TypePublication` aligned with the payload row. Vue call-signature returns are
addressed by declaration-order signature ordinal; Svelte callback-prop returns
are addressed by member path. The output sink materializes that lane once, and
property/event-map rows alone use the implicit `void` return. Vue analyzer and
normalized rows align by producer kind plus kind-local ordinal, so normalized
call-first ordering cannot move a return onto a same-name property overload.
Return-only output failures identify the distinct `EventReturn`
(`events[].return`) materialization lane through session, FFI, and protocol
diagnostics.

## Single projection authority

`verter_session::framework::public_contract` is the sole projector. It consumes
one `ComponentMetaAnalysis` and the positionally aligned materialized output
lanes under the same fixed store view used for declaration rendering. Vue and
Svelte adapters render declaration carriers only; they do not construct public
contracts.

Each public type reference retains its structured descriptor, A1 publication,
producer diagnostics, and separately branded terminal display. Display is an
output convenience, not semantic input.

## Consumer boundary

Consumers accept `ComponentContractAvailability`, not generated declaration
code, `TscResponse`, component-meta `rawSignature`/`returnType`, or terminal
display. LSP component, event, and slot summaries format the structured
contract directly and fail closed on `Unsupported`.

Full response version 5 is rejected by the version-6 decoder. Tag 25 remains
free for the selective-response train; selective contract parity is outside
this cutover.
