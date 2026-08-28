# Component-meta compat descriptor contract

Component-meta response schema version 5 makes the native `TypeDescriptor`
graph the sole semantic input to `@verter/component-meta/compat`. Version 4 is
rejected. Version 5 adds no protobuf node or tag; it marks the cutover to a
structurally complete payload.

## Native completeness

The native response publishes every descriptor and demanded registry entry
needed to interpret a published surface without reading type text. Registry
publication remains shallow and query-scoped: an imported surface such as
`MemberValueProps` may retain `Fn | Fn[]`, while its demanded workspace-local
`Fn` entry publishes the callable structure. Unreferenced siblings and
package-backed helper internals remain unpublished.

Missing representable structure is a native producer defect or an explicit
unsupported result. Compat does not repair it from text and does not issue
follow-up native resolution requests.

## Structural compat roles

Compat classification uses descriptor variants and fields only:

- a function-or-array event requires a union containing a function arm and an
  array whose element is structurally the same function;
- Booleanish requires the corresponding reference structure;
- NuxtLink-style values require the indexed-access/reference structure;
- button types require the `"button" | "submit" | "reset"` literal union;
- branded strings require an intersection containing `string` and an empty
  object arm;
- void-like event payloads require the `void`, `undefined`, or `never`
  primitive descriptor.

An `unknown` descriptor is unsupported. Its `rawType` is diagnostic/display
data and cannot establish support, a role, an event arity, or a schema arm.

## Display boundary

`rawType`, `rawSignature`, `TerminalTypeDisplay`, and descriptor renderers are
terminal output facilities. They may format a role after structural
classification, but they cannot participate in classification. The compat
`rawType` field remains the mechanical projection of terminal display.

There is no text fallback, heuristic repair, or dual path. Changing terminal
text while keeping the descriptor graph fixed cannot change the selected role,
schema kind, union arm count, or event arity.

## Enforcement

- The default component-meta package test runs `checker.spec.ts`; the exact
  historical exclusion is guarded against reintroduction.
- Native integration covers imported `Fn | Fn[]` plus the callable registry
  dependency.
- Compat tests cover structural positives and hostile display negatives.
- The AST/call-graph guard rejects `rawType`/`rawSignature` reads and renderer
  calls from the semantic classifier closure.
