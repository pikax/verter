# Stencil contract cases

These are reviewed rejection requirements, not executed tests. STN1 implements
every row in `contract.ts` and `stencil-lock.spec.ts` at this directory and owns
STN1-ACV. REG0 discovers that spec; this lock adds no validator, CI gate or root
test script. The later proof command is
`node --test tests/framework-stencil/STN0/stencil-lock.spec.ts`.

Each planted row is an independent edit to the clean products. Reject for its
stated reason. V = version lock, M = capability matrix, A = activation policy,
H = host table. The manifest lists the same cases for discovery.

## STN0-AC1: pinned release

Clean products: stable major 4, exact reference release 4.45.2 with official npm
source and integrity; every profile reference equals that release. A user's
resolved stable 4.x patch need not equal the conformance reference patch. No
package installation or compiler execution is claimed by this lock.

| Twin                  | Planted row                                           | Expected failure                             |
| --------------------- | ----------------------------------------------------- | -------------------------------------------- |
| legacy-major-pin      | V reference version = `2.22.3`                        | Legacy major outside admitted stable 4       |
| floating-pin          | V reference version = `^4.45.2`, `4.x` or `*`         | Evidence requires an exact version           |
| latest-pin            | V reference version = `latest`                        | A dist-tag is not an exact pin               |
| canary-pin            | V reference version has `-canary` or `-alpha`         | Prerelease/canary is not admitted            |
| nightly-pin           | V reference version has `-nightly` or `-dev`          | Nightly/dev is not admitted                  |
| missing-source        | Remove V reference source or integrity                | No authoritative dependency artifact         |
| diverged-pin          | V profile referenceVersion = `4.45.1`                 | Reference pin diverges from referenceRelease |
| unratified-next-major | V admits major 5 while nextMajor remains not-admitted | No ratified exact next-major profile         |

Controls: 4.45.2 passes; a resolved stable 4.44.2 user installation is eligible
under the major policy without rewriting the reference release. Neither a
manifest range nor an unresolved package is itself resolved-version evidence.

## STN0-AC2: owned matrix

Clean products: one row per operation/host/profile, one producer and receiving
acceptance, and no invented product claim. The CEM cell belongs to HWC3-AC2;
STN3-AC2 supplies neutral facts and provenance, never CEM bytes. Cell inclusion
records planned ownership, not shipped support.

The definition operations are `extra-tag-definition`, `extra-watch-definition`
and `extra-listen-definition`. They share LSPX11's extra-definition capability,
but have distinct semantic cell identities: the tag operation receives
STN5-AC2; watch and listen each receive STN5-AC3. Copying any of these rows with
a new id and the same operation/host/profile is still a duplicate.

| Twin                     | Planted row                                                                | Expected failure                                                |
| ------------------------ | -------------------------------------------------------------------------- | --------------------------------------------------------------- |
| unowned-cell             | Remove/empty a cell producer                                               | Cell has no sole producer                                       |
| missing-acceptance       | Remove a cell acceptance                                                   | No receiving acceptance item                                    |
| duplicate-cell           | Copy tag-hover with a new id but identical operation/host/profile          | Duplicate cell identity                                         |
| fabricated-cell          | Add an invented supported operation such as automatic component generation | No ratified operation or receiving acceptance                   |
| build-cell               | Add a Stencil compiler/build cell                                          | Framework compilation/build is excluded                         |
| output-target-build-cell | Turn captured outputTargets into a dist build operation                    | Configuration facts do not authorize build output               |
| runtime-cell             | Add render, hydrate, dev-server or runtime registration                    | Runtime work is excluded                                        |
| cem-wrong-owner          | Change cem-projection producer to STN3                                     | HWC3 alone owns CEM projection/import/export                    |
| cem-stencil-semantics    | Require HWC3 to inspect decorators or EventEmitter syntax                  | Framework binding must precede neutral evidence intake          |
| partial-cem-complete     | Mark refused or partial handoff facts complete                             | Refusal/partial status cannot certify complete CEM              |
| unsupported-facet-empty  | Replace Model or Options UNSUPPORTED with an empty supported facet         | Unmapped facets are unsupported, never fabricated empty support |

Controls: outputTargets remain distinct static registration/hydration identities
through CENV1C without execution. Checked-in components.d.ts is an ordinary type
input; stale/missing declarations are reported, never regenerated. HWC3 can
consume the handoff fields without knowing any Stencil semantics. Slots originate
in render JSX, not a nonexistent slot decorator.

## STN0-AC3: host, activation and tag

Clean products: only LSPX11 enhancements, listed tsgo operations and typed
limitations per cell, canonical package/decorator identity, and planned tag 8
landing with the adapter. Neither a TS plugin nor an ordinary TS result is a
Stencil enhancement.

| Twin                       | Planted row                                                                 | Expected failure                                    |
| -------------------------- | --------------------------------------------------------------------------- | --------------------------------------------------- |
| ts-plugin-host             | Change any host to typescript-plugin                                        | Excluded host                                       |
| tsgo-passthrough           | Return the generated interface or ordinary TS hover/rename                  | Re-provided tsgo/TS result                          |
| missing-host-cell          | Delete a matrixCells reference from H                                       | Host table and matrix do not cover the same cells   |
| unknown-profile            | Change a cell profile to react-tsx                                          | Profile outside the release/activation lock         |
| missing-tsgo-operation     | Remove getSymbolAtPosition from tag-definition                              | Declared operation prerequisites incomplete         |
| missing-limitation         | Remove eventEmitterTypeArgument from tag-hover                              | Required typed refusal no longer declared           |
| parsed-typeToString        | Recover EventEmitter payload by parsing typeToString                        | Display strings are not semantic authority          |
| wrong-wire-tag             | Change M frameworkTag value from 8                                          | Wrong ratified tag                                  |
| open-canonical-tag         | Replace FRAMEWORK_TAG_STENCIL with OPEN_CANONICAL                           | Generic tag cannot identify the adapter             |
| extension-only-activation  | Activate every .tsx file in a package                                       | Extension alone is not canonical framework evidence |
| local-decorator-activation | Accept a local or another package's Component/Prop decorator                | Spelling cannot establish identity                  |
| other-framework-activation | Activate a React/Preact/Solid/Qwik-owned file as Stencil                    | FWA1 exclusivity violated                           |
| forced-legacy-activation   | Admit resolved major 2 because setting is on                                | Explicit on cannot admit an excluded release        |
| inactive-work              | Permit adapter, index, TypeInfo, tsgo or contributor work when off/inactive | Inactive must do zero work                          |

| Twin                      | Planted row                                                                | Expected failure                                        |
| ------------------------- | -------------------------------------------------------------------------- | ------------------------------------------------------- |
| rename-followup           | Add a follow-up rename after tsgo handles a TS member position             | TS-position rename is final; step back when covered     |
| rename-generated-edit     | Include components.d.ts in the rename transaction                          | Generated file is untouched; report regenerate-required |
| rename-collision          | Admit a colliding attribute rename                                         | Collision must be refused                               |
| rename-attribute-override | Rename an attribute derived from a member with explicit attribute override | Explicit override is refused                            |

Controls: canonical aliases/re-exports resolve through TS symbols; computed tag,
config or slot names remain unknown. An unresolved prop type is Approximate;
unavailable EventEmitter type arguments retain typed refusal and emit
`$/verter/tsgoLimitation` once per operation/project. Plain TSX and competitor-owned
positions cause no enhancement. LSPX11, not this lock, verifies client merging.

Incremental/cache, cancellation and bounded-work implementation tests are not
applicable to this data-only delivery: it changes no semantic execution path.
The producers' receiving acceptances own those proofs; this document claims no
test execution or machine-checked support.
