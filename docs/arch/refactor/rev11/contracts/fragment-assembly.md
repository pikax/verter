# Fragment and assembly contract

**Proposed by:** AMD-005. **Authority:** none until maintainer ratification.

B4 owns logical source units, stable source identities, placement, mapping
composition, and atomic compiler-artifact publication. It selects a concrete emitter
or edit representation only after evidence, never by precommitting this program to
`CodeTransform` or another current mechanism.

Each fragment declares: framework/domain, product/profile, source unit and source
space, placement point, syntactic contract (complete module, statement list,
expression, declaration, style, or metadata), imports/exports/helpers, map segments,
and dependencies. A fragment must parse under its declared contract. Final assembly
must parse as its declared ECMAScript/TypeScript module and link against exact real
packages.

Assembly is product-plan driven. It cannot infer placement by reparsing generated
text, recover one product from another, inject an undeclared helper, or publish an
intermediate fragment as a final artifact. Source-map composition preserves the
original-source, generated-fragment, and assembled-output spaces explicitly; no raw
offset crosses a source-space boundary.

Publication is one atomic transaction. The artifact set contains exactly requested
products and their contract-required parts. Requesting an IDE/provider companion
implicitly requests its non-optional `SourceProjectionMap`; the companion and map are
produced, delivered, and published atomically. Optional runtime/build map content
(`RuntimeSourceMapData` and terminal `EncodedSourceMap`) is produced and attached only
when that map product is requested. No hidden product is used merely to build another.
Any fragment, assembly, mapping, link, or capability failure produces typed
non-success and publishes no partial set.
