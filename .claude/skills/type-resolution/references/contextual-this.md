# Contextual object receivers

Object-literal methods use the argument of the global `ThisType<T>` marker
in their contextual type, including intersections, unions and generic aliases.
Intersections select the first marker in authored order; unions combine the
marker arguments contributed by their alternatives.
Direct property-value literals retain the nearest enclosing contextual marker.
Array elements, call arguments and conditional arms start independent receivers;
their own contextual marker still applies. Without a marker they use a concrete
contextual type, then the literal's own type.
`noImplicitThis: false` retains the checker's `any` receiver. Arrow functions
keep the receiver of their lexical frame. An authored `this` annotation or a
contextual signature's `this` parameter takes precedence over the contextual
marker. The authored annotation is lowered with the demanded body;
lexical contexts carry receiver identity, never annotation contents.

Contextual targets are lowered in carrier mode. Alias applications are
instantiated in that mode while searching for the marker, preserving its
argument until contextual typing consumes it. Marker discovery, literal inference
and receiver dependency ordering share the same carrier-preserving view.
Recognition checks the resolved declaration against the consuming project's
global namespace; an ambient library file need not own a project.
A similarly named local declaration is
not a marker.
Structural readers still instantiate the empty interface normally.

Top-level contextual literals and calls containing context-sensitive object
arguments serve an initializer body through the ordinary flow evaluator and
call continuation. Discovery and expression facts share their classification.
Their first pass preserves nested object structure, withholds methods that
need receiver typing, and collects receiver-independent method returns.
Those checked values already underwent mutable-property widening, so their
inference deposits preserve authored literal pins.
The first pass carries contextual marker metadata without selecting a receiver.
A marked object with withheld methods contributes no partial object candidate,
even when it contains checked data or spreads. Placeholder-only spread literals
do not contribute an authored open-key `any` spread.
Nested first-pass objects use the expression evaluator's explicit stack.
An object containing only withheld methods contributes no inferred object
candidate; authored `any` properties still contribute their actual shape.

The contextual object pass lends completed properties to the candidate's
existing inference collector. It installs that collector only for the
property's relation and contextual mapper, then suspends it again before
evaluating another body. Getters can therefore infer types consumed by later
actions without restarting the call or replaying earlier contextual properties.
Receiver dependencies on type parameters order their producing properties
before their consumers, independently of authored property order. Spreads and
computed keys do not disable dependencies among known properties. Published
members and construction effects retain authored order; cycles keep source order.
The contextual mapper preserves uninferred binders in non-callable contexts;
fixing them to an empty object would discard the receiver's own method part.
Final applicability and result publication remain the call executor's work.

Object construction retains the enclosing receiver while evaluating property
initializers and selects its own receiver for method bodies. A concrete
contextual receiver's declared members take precedence over the literal's
members. A receiver's still-uninferred object binder admits collected literal
members and forward sibling reads. Pending methods retry while siblings make
progress, so acyclic chains resolve independently of source order; stalled
cycles retain typed degradation. Direct child descent grants receiver inheritance
once, so literals reached through another expression cannot add members to the
enclosing receiver or reset its forward-read state. Inherited inferred marker
receivers share their collected members with direct spread literals, whose
unresolved method sources are revisited after the enclosing methods complete.
A concrete marker admits only its declared members. Ordinary function-valued
properties share method receiver typing;
arrows retain their lexical receiver. Methods with known computed keys use the
same receiver and contextual signature as static keys. Spreads retain contextual
receivers and the ordinary construction-effect ordering, including overwrites.
Provisional spread sources that read pending methods are not projected into the
receiver. Completed anonymous spreads use the shared projection's surface for
both property inference and the replayed call argument.
Partial contextual work remains a typed degradation and cannot
publish a complete cached result.

Coverage lives in `class_self_reference_tests`: inferred data at three widths,
cold and warm answers, relation growth, nested marker intersections, lexical
arrows, ordinary receivers, non-strict `this`, getters consumed by earlier or
later actions, forward siblings, generic aliases, enclosing markers, spreads,
explicit annotations, top-level contextual literals and receiver isolation
across array, call and conditional expression boundaries.
