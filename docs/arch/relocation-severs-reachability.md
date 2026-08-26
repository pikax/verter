# Relocation severs what the moved code could reach

An invariant enforced by **reachability** is enforced by the dependency graph, and a graph edit
silently repeals it. The rule stays written down, stays believed, and stops being enforced. Nothing
compiles differently.

## The instance this is drawn from

A project-reference depth fuse lived in `verter_workspace`, where it could reach the reader's
admission fence. Unifying the resolver moved it into `verter_semantic`, which cannot see
`verter_workspace` at all. The fuse still tripped; the fence it used to call was no longer
reachable; the trip returned `hit(None, output)` — budget exhaustion reported as a successful miss.
Downstream could not distinguish a fuse trip from a genuine not-found, so the negative was admitted
to the cache, breaching the cache-runtime rule that a return-only outcome never publishes.

The move carried the thing across the boundary and left its instrument behind.

## Why the compiler does not catch it

The asymmetry is the whole problem:

- **A caller that no longer compiles announces itself.** Inbound edges are checked for you.
- **A callee that is merely no longer reachable does not.** The code that used to call outward now
  simply returns something else, and every remaining path type-checks.

We have learned to trust the compiler precisely where it is silent.

## What to do before moving code across a crate boundary

**Enumerate what the code calls outward to, not only what calls it.** For each outward call that the
destination crate cannot see, decide explicitly: move the collaborator too, invert the dependency,
pass the capability in, or accept the loss and record it. An unrecorded loss is the defect.

**Treat "the right type present and not returned" as the tell.** `AttemptFailure::InputResolutionDepthLimit`
existed at the site and was unused. A variant that names exactly the condition the code is in, and is
never constructed, is what a severed capability looks like from the inside — and unlike the defect
itself, it is greppable.

**Check the sibling arms.** The same function's other arm routed correctly; the defect was an
asymmetry inside one match rather than an absent capability. Where one arm reports a typed failure
and its neighbour returns a bare `None`, the neighbour is the suspect.

## The same class in test code

A compile-fail fixture that denies paths in another crate is coupled to the dependency edge between
them. Delete the edge and every error moves to the crate segment: the fixture can no longer name what
it denies, and passes identically whether or not the forbidden thing exists.

For a test, severing produces blindness that announces nothing. For production code it produces a
lost capability that announces nothing. **The second is worse, and neither compiles differently.**
