# Contextual object receivers

Object-literal methods use the argument of the global `ThisType<T>` marker
in their contextual type, including intersections and aliases. Without that
marker they use a concrete contextual type, then the literal's own type.
`noImplicitThis: false` retains the checker's `any` receiver. Arrow functions
keep the receiver of their lexical frame.

The declaration carrier keeps the global marker application and its argument
until contextual typing consumes it. Recognition checks the resolved global
declaration identity in the consuming project's global namespace; an ambient
library file need not own a project. A similarly named local declaration is
not a marker.
Structural readers still instantiate the empty interface normally.

Top-level calls containing context-sensitive object arguments serve an
initializer body through the ordinary flow evaluator and call continuation.
Their first pass preserves nested object structure, withholds methods that
need receiver typing, and collects receiver-independent method returns.
Those checked values already underwent mutable-property widening, so their
inference deposits preserve authored literal pins.
Nested first-pass objects use the expression evaluator's explicit stack.
An object containing only withheld methods contributes no inferred object
candidate; authored `any` properties still contribute their actual shape.

The contextual object pass lends completed properties to the candidate's
existing inference collector. It installs that collector only for the
property's relation and contextual mapper, then suspends it again before
evaluating another body. Getters can therefore infer types consumed by later
actions without restarting the call or replaying earlier contextual properties.
Final applicability and result publication remain the call executor's work.

Object construction retains the enclosing receiver while evaluating property
initializers and selects its own receiver for method bodies. A concrete
contextual receiver's declared members take precedence over the literal's
members. Partial contextual work remains a typed degradation and cannot
publish a complete cached result.

Coverage lives in `class_self_reference_tests`: inferred data at three widths,
cold and warm answers, relation growth, nested marker intersections, lexical
arrows, ordinary receivers, non-strict `this`, and getters consumed by actions.
