# Preact contract counterexamples

This is reviewed data, not executed test evidence. PRE1 implements every row
below in `contract.ts` and `preact-lock.spec.ts` under PRE1-ACV. The planned
command is `node --test tests/framework-preact/PRE0/preact-lock.spec.ts`;
REG0's discovery runner finds it without a root `test:scripts` edit.

For each acceptance item, the clean leg loads all four products from
`manifest.json` unchanged and accepts them. Each planted leg changes only the
named boundary, so it must reject for the stated reason. Do not treat an
unrelated parse error as the expected rejection. Run native and compat cells
against each admitted release separately, with and without optional signals.

## PRE0-AC1 — pinned release

| Planted row              | Change                                                                         | Expected failure reason                                                             |
| ------------------------ | ------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------- |
| floating-pin             | Replace a reference pin with `^10.29.8`, `10.x` or `>=11`                      | A reference pin must be one exact stable version.                                   |
| latest-pin               | Replace a pin with `latest`                                                    | A distribution tag is not an exact version.                                         |
| canary-pin               | Replace a pin with `canary`, `nightly` or `11.0.0-rc.2`                        | Tags and all prereleases are excluded, even inside admitted majors.                 |
| legacy-8-pin             | Admit `8.5.3`                                                                  | Preact 8 is a legacy major outside the release set.                                 |
| diverged-pin             | Change a release copy/source/tarball version while retaining its reference pin | Every reference to a pinned artifact must agree on package and exact version.       |
| signals-not-optional     | Set signals `optional` to false or make it required by Preact                  | Signals is optional and independently versioned.                                    |
| unratified-major         | Admit Preact 12 or signals 3                                                   | The release set admits Preact 10/11 and the recorded signals 2 line only.           |
| two-releases-per-package | Select both Preact release ids in one package contribution manifest            | One Preact release per package contribution; profiles do not create extra releases. |
| signals-peer-mismatch    | Activate signals 2.11.3 with installed Preact 10.24.0                          | Optional signals must satisfy its published Preact peer requirement.                |

## PRE0-AC2 — owned matrix

| Planted row                 | Change                                                                                    | Expected failure reason                                                       |
| --------------------------- | ----------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------- |
| unowned-cell                | Delete a cell's producer                                                                  | Every admitted operation has exactly one producer.                            |
| duplicate-cell              | Duplicate an operation × host × profile cell, including under a new id or second producer | Cell identity is the tuple, not merely a unique string id.                    |
| missing-acceptance          | Delete a receiving acceptance                                                             | Every producer cell names one receiving acceptance.                           |
| wrong-producer-acceptance   | Give a PRE5 cell a PRE8 acceptance                                                        | The receiving acceptance must belong to the named producer.                   |
| fabricated-cell             | Add a claimed framework completion, runtime output or named-slot operation                | No support outside the ratified operation and facet set.                      |
| build-cell                  | Promote the build exclusion to an admitted preset/build/transform cell                    | Build is excluded from this contract.                                         |
| runtime-cell                | Promote the runtime exclusion to an execution/render/hydration cell                       | Runtime work is excluded; static call classification is not runtime delivery. |
| exclusion-without-reason    | Delete an exclusion reason                                                                | Exclusions must explain their boundary truthfully.                            |
| derived-facet-authoritative | Mark callback events or children/render-prop slots authoritative/native                   | Emits and Slots are derived with their recorded provenance.                   |
| unsupported-facet-empty     | Replace Model or Options UNSUPPORTED with an empty supported facet                        | Unmapped facets remain UNSUPPORTED.                                           |

## PRE0-AC3 — two activation profiles

| Planted row                     | Change                                                                       | Expected failure reason                                                                        |
| ------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| collapsed-profiles              | Give native and compat the same profile id or omit one                       | Native and compat are distinct profiles of one Preact adapter.                                 |
| compat-activates-react          | Change compat framework/adapter to React, or dispatch both contributors      | Alias-derived compat activates Preact only; one contributor per file.                          |
| alias-by-import-spelling        | Activate compat from unresolved `react` text rather than the PM alias target | Canonical PM resolution and provenance are required.                                           |
| pragma-without-resolved-release | Let `@jsxImportSource preact` admit a missing or unadmitted Preact release   | A pragma narrows selection; it cannot manufacture an installed release.                        |
| on-bypasses-admission           | Let `on` admit an unsupported version                                        | `on` never bypasses admission; manifest-free activation requires an explicit admitted release. |
| off-still-does-work             | Let a pragma or PM role override `off`                                       | `off` disables the entire vertical with zero work.                                             |
| signals-without-package         | Publish signal facts without resolved `@preact/signals`                      | Optional facts require FWA1 package evidence.                                                  |
| signals-unadmitted-package      | Publish signal facts for an excluded signals release                         | Package presence alone does not prove admission.                                               |

## PRE0-AC4 — host table and wire tag

| Planted row                | Change                                                                              | Expected failure reason                                                                                         |
| -------------------------- | ----------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------- |
| ts-plugin-cell             | Change a host to `typescript-plugin`                                                | Every admitted host is `lsp-enhancement` through LSPX11.                                                        |
| reprovided-tsgo-cell       | Set `reprovidesTsgo` true or include ordinary TS hover/definition/diagnostic output | Verter contributes only owned Preact enhancements.                                                              |
| missing-tsgo-operations    | Remove the operations field or required type/symbol operation from a hover cell     | Every cell records its required tsgo operations; an explicit empty list is valid only for fact-only operations. |
| host-matrix-drift          | Delete a host operation mapping or point it at a missing cell                       | The host table and matrix must join in both directions for every profile.                                       |
| invented-tsgo-operation    | Treat props-member or callback-signature enumeration as available tsgo API calls    | These operations are limitations when TypeInfo cannot answer, not callable tsgo reflection APIs.                |
| limitation-marked-complete | Publish a refused callback/props fact as Complete                                   | Missing authority yields typed Unavailable and `$/verter/tsgoLimitation`.                                       |
| parsed-type-string         | Recover semantic members/parameters by parsing `typeToString`                       | The string is display only, never a type authority.                                                             |
| open-canonical-tag         | Replace the tag with `OPEN_CANONICAL`/5                                             | OPEN_CANONICAL is structural and cannot identify an adapter.                                                    |
| wrong-preact-tag           | Replace `FRAMEWORK_TAG_PREACT = 6` with another name/value                          | The Preact tag allocation is fixed at 6.                                                                        |
| ownership-bypass           | Answer an inactive or stepped-down cell without LSPX11 `owns`                       | Inactive/stepped-down capabilities do zero work.                                                                |

These rows specify future behavioral proofs. This contract changes no parser,
activation implementation, state/cache owner, authored map, provider or public
transport. Cold/warm, cancellation, edit/revert, determinism and runtime probes
are therefore not applicable here. PRE1/PRE2 and the named producers own those
checks when they implement the corresponding boundary. No existing Preact
route is displaced and there are no legacy deletions.
