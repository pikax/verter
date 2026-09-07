// The pinned instance data of ONE closure register.
//
// The validator beside this file is the derivation; this file is everything
// that derivation is pinned AGAINST for the typescript-mapper instrument — the
// register and view paths, the raising node and train, the closed claim, atom,
// row, finding and residue universes with their statement and anchor digests,
// the acyclicity carve-out, and the remainder topology.
//
// Separating them is not decoration. Held inside the validator, roughly two
// hundred instance constants meant a second closure package could only fork the
// tool, and every legitimate prose repin edited the tool — so the validator's own
// diff was permanently noisy with data churn, in the one file where a change to
// a refusal has to be easy to see.
//
// What does NOT change is the security property. These values are hand written
// and reviewed, exactly as they were when they sat in the validator: the pinned
// universes exist so that deleting a claim, an atom or a row FAILS instead of
// shrinking what is checked, which is only true while nothing regenerates them
// from the register they judge. A lock file emitted by `--write` would read the
// register for its own contents and could never notice an omission. This file is
// beside the validator, in the same commit and the same review; a repin is a
// line of that review, and `analyze()` still takes the universe, topology and
// carve-out as arguments, so a second register supplies its own pins here rather
// than forking the derivation.
export const REGISTER_RELATIVE = "closure/typescript-mapper/register.toml";
export const VIEW_RELATIVE = "closure/typescript-mapper/closure.md";
export const REGISTER_SCHEMA = "closure-register.schema.json";

/** The block whose rescope raised these remainders. A residue owner must be a strict descendant. */
export const RAISING_NODE = "TCM0R";

/** The train the raising node belongs to. A cited criterion may not leave it. */
export const RAISING_TRAIN = "rev11.typescript-mapper";

/**
 * The pinned closure universe.
 *
 * Findings and residues were already closed sets held here. Claims, atoms, and
 * the deletion/survivor row subjects were not: they were read straight out of
 * the register, so deleting a claim, deleting one of its atoms, or dropping a
 * displaced-route row shrank the universe instead of failing. That is exactly
 * the omitted-claim class this instrument is required to control, so the
 * universe is pinned here, beside the validator, and the register is checked
 * against it for exact set equality in both directions.
 *
 * A claim's `subject` names the artifacts the claim is ABOUT — the ones whose
 * behaviour a proof of that claim judges. It is what makes the acyclicity rule
 * derivable from declared structure instead of volunteered by the proof.
 */
export const LIVE_UNIVERSE = Object.freeze({
  claims: Object.freeze({
    "CLM-PLANE": Object.freeze([
      "A-plane-boundary",
      "A-no-oracle-callback",
      "A-mapper-total",
      "A-projection-classes-closed",
      "A-topology-selected",
    ]),
    "CLM-IDENTITY": Object.freeze([
      "A-basis-not-a-cache-key",
      "A-query-identity-snapshot-independent",
      "A-flight-key-strictly-larger",
      "A-profiles-in-query-identity",
      "A-profile-multiplicity-is-one-question",
      "A-unit-lineage-stable",
      "A-identity-alias-is-compile-error",
    ]),
    "CLM-BINDING": Object.freeze(["A-binding-is-a-witness", "A-no-unbound-semantic-result"]),
    "CLM-LIFECYCLE": Object.freeze([
      "A-publication-requires-live-basis",
      "A-degraded-never-warms",
      "A-slot-keyed-by-query-identity",
      "A-incremental-equals-fresh",
      "A-hang-topology-characterised",
    ]),
    "CLM-OWNERSHIP": Object.freeze([
      "A-single-owner-per-capability",
      "A-no-dual-running-window",
      "A-deletion-rows-concrete",
      "A-survivor-rows-received",
      "A-residue-owner-non-circular",
      "A-receiving-criterion-direct",
      "A-resolution-gate-named",
      "A-authority-consistent",
      "A-downstream-owner-restated",
    ]),
    "CLM-EVIDENCE": Object.freeze([
      "A-fixtures-named",
      "A-command-exact",
      "A-terminal-summary-present",
      "A-node-evidence-re-executed",
      "A-external-refresh-lane-bound",
      "A-counters-consistent",
      "A-zero-unexpected-skips",
      "A-nonzero-selected-work",
      "A-targeted-domain-green",
      "A-implementation-baseline",
    ]),
    "CLM-INSTRUMENT": Object.freeze([
      "A-status-derived-not-authored",
      "A-adapter-allowlisted",
      "A-portable-controls-only",
      "A-human-view-generated",
      "A-check-refuses-unreviewable",
      "A-claim-universe-complete",
      "A-bounded-needs-transfer",
      "A-acyclic-proof-dependency",
      "A-proof-relevance-bound",
      "A-stated-obligation-is-enforced",
      "A-finding-closed-by-atom",
      "A-control-mutation-locatable",
      "A-controls-cover-every-class",
      "A-ledger-model-intact",
    ]),
  }),
  /**
   * The proposition each id asserts, as a digest of its whitespace-normalised
   * text. Pinning the ids alone left the content half author-controlled: a
   * statement could be weakened to whatever its evidence already showed while
   * the id, the coverage and the derived status all stayed put. Repinning a
   * corrected statement is a line of the same review; rewriting one silently
   * is not available.
   *
   * Every prose field the register asserts something WITH is here, not only the
   * three that carry an id of their own. A deletion row's disposition says how
   * a displaced route was rejected; a remainder's statement says which question
   * is being carried; a negative control's mutation and observed outcome are
   * the whole record of what that control demonstrated; a receiving row's gate
   * is what its owner has to clear, constrained by the derivation only to open
   * by naming that owner; a record's skip basis is why its declared skips are
   * expected rather than unexpected. Every one of them was reachable by an edit
   * that moved no id, no count, and no derived status, which is the same hole
   * the claim/atom/finding pins close — and for the control fields it is the
   * one place a hollowing would be invisible to the hollowed-statement control
   * itself.
   */
  statements: Object.freeze({
    "claim:CLM-PLANE": "a33fda7ebc9bba53",
    "claim:CLM-IDENTITY": "9c23ff85a0eeb5ea",
    "claim:CLM-BINDING": "5b69471b42c43e3c",
    "claim:CLM-LIFECYCLE": "a76c771c0c9fd1a1",
    "claim:CLM-OWNERSHIP": "50a05c9598f3fd03",
    "claim:CLM-EVIDENCE": "06fd30878cd0b7eb",
    "claim:CLM-INSTRUMENT": "98cfb8243835a806",
    "atom:A-plane-boundary": "e39900c05faa75f0",
    "atom:A-no-oracle-callback": "e32b60301575e0aa",
    "atom:A-mapper-total": "eb42a02ee51d50bd",
    "atom:A-projection-classes-closed": "0e739397eb63f846",
    "atom:A-topology-selected": "0857d9338dab802f",
    "atom:A-basis-not-a-cache-key": "b782ebf95538d8ce",
    "atom:A-query-identity-snapshot-independent": "fadf52cb0416bcff",
    "atom:A-flight-key-strictly-larger": "b2a7641d7a2df22f",
    "atom:A-profiles-in-query-identity": "633ba6bf08e3b988",
    "atom:A-profile-multiplicity-is-one-question": "a5d0795832359b43",
    "atom:A-unit-lineage-stable": "91e29388144abcae",
    "atom:A-identity-alias-is-compile-error": "6483ea355ed855a8",
    "atom:A-binding-is-a-witness": "5c08100bd4b8d3d8",
    "atom:A-no-unbound-semantic-result": "f6ebbb27d75281b7",
    "atom:A-publication-requires-live-basis": "b635b76d591de3a5",
    "atom:A-degraded-never-warms": "62eab6b463484940",
    "atom:A-slot-keyed-by-query-identity": "d6562a9ad785153a",
    "atom:A-incremental-equals-fresh": "54c18d2ec79981ab",
    "atom:A-hang-topology-characterised": "3c9b8e6f73eff8d5",
    "atom:A-single-owner-per-capability": "06bb39a009aa05d5",
    "atom:A-no-dual-running-window": "2cef7bf1d37f2f55",
    "atom:A-deletion-rows-concrete": "0c79451d4d1cdb1b",
    "atom:A-survivor-rows-received": "84ccb827695506b3",
    "atom:A-residue-owner-non-circular": "f06ca490c6ed2f5e",
    "atom:A-receiving-criterion-direct": "40de0072552fa0d7",
    "atom:A-resolution-gate-named": "de2760d57249c88b",
    "atom:A-authority-consistent": "7e57194dcead7e25",
    "atom:A-downstream-owner-restated": "3bd841fb54d72dd9",
    "atom:A-fixtures-named": "217d0d4c8e036806",
    "atom:A-command-exact": "ad5277458d796772",
    "atom:A-terminal-summary-present": "92a3d03a22524216",
    "atom:A-node-evidence-re-executed": "ebca8d95b08e4654",
    "atom:A-external-refresh-lane-bound": "2052a41c631e146f",
    "atom:A-counters-consistent": "36057347af0bff90",
    "atom:A-zero-unexpected-skips": "bbdef0226518f59b",
    "atom:A-nonzero-selected-work": "bbbd72e93c466f07",
    "atom:A-targeted-domain-green": "b1f7554ea8ef38c1",
    "atom:A-implementation-baseline": "80691025da1c8af2",
    "atom:A-status-derived-not-authored": "bd6d92df361bdcc9",
    "atom:A-adapter-allowlisted": "4ddc62e6ca010e73",
    "atom:A-portable-controls-only": "ae82df728f543cbb",
    "atom:A-human-view-generated": "385d18ef6fa95311",
    "atom:A-check-refuses-unreviewable": "6d1da334d18aff6f",
    "atom:A-claim-universe-complete": "e74875d7589604d3",
    "atom:A-bounded-needs-transfer": "241f2d5943a25de2",
    "atom:A-acyclic-proof-dependency": "327c6de924787c84",
    "atom:A-proof-relevance-bound": "3d6b18e1e31f52ed",
    "atom:A-stated-obligation-is-enforced": "6bad1eddf7b06523",
    "atom:A-finding-closed-by-atom": "0df1de1847b62c83",
    "atom:A-control-mutation-locatable": "bca9f93e1610dc28",
    "atom:A-controls-cover-every-class": "0e6f0bb4bdb3eae1",
    "atom:A-ledger-model-intact": "3b2468c0b2f666b8",
    "finding:C1": "e743680e29457d2a",
    "finding:C2": "867b0f37f98eddd9",
    "finding:C3": "0a57a7494fc9efe3",
    "finding:C4": "4736cf4d9fb9a07a",
    "finding:C5": "6db4726196f26a52",
    "finding:C6": "1fad8aa00754d96e",
    "finding:C7": "cf6630ca1052e86c",
    "finding:C8": "54b4be605aaa6d1f",
    "finding:C9": "c368ba049961d38e",
    "finding:AR1": "8c5d5539fb7be666",
    "finding:AR2": "43c9388b0adf00c5",
    "finding:AR3": "c0ef25aebddea8a7",
    "finding:AR4": "3f73939658d619a0",
    "finding:AR5": "dadf8d2433396c14",
    "finding:AR6": "3153452ffb0ec0f0",
    "finding:AR7": "a52e173fc1dd4587",
    "finding:AR8": "9dd92bd22d673d33",
    "finding:AR9": "a2c226f765e088f8",
    "finding:AR10": "a42645cd8549c570",
    "finding:AR11": "2fc9eb45949324b9",
    "finding:AR12": "c15b93ee9ec196af",
    "finding:AR13": "fdb46bcf14961fbc",
    "finding:AR14": "656679219d92cb10",
    "finding:AR15": "97b905b30090143f",
    "finding:AD1": "5148767e4c81dc44",
    "finding:AD2": "7ab51046feb9dbb9",
    "finding:AD3": "e867c5893f82136b",
    "finding:AD4": "dc97b10f5664ba7c",
    "finding:AD5": "dded5704276e46e0",
    "finding:AD6": "ee1db5385f92fe5f",
    "finding:AD7": "2056c06997683801",
    "finding:AD8": "f8b73998bc32395d",
    "finding:AD9": "034b1a85bdd1075a",
    "finding:AD10": "72c54520b0692ea1",
    "finding:AD11": "bf93251fd81121c4",
    "finding:AD12": "76c1e93cafd114bf",
    "row:Self-certified closure status": "6616d0cb7c842cee",
    "row:Tracked Python or POSIX control": "846911f33c86a135",
    "row:Name-keyed scanner guard": "6f08a482ac9629c7",
    "row:String mapper plane": "f3d21589542c4a78",
    "row:Mapper callback into the semantic oracle": "2094b38b8e96cdee",
    "row:Repaired package and binary provenance": "6ddb45794b91136f",
    "row:Mapper captures": "7d1aa3ddcaf08cb7",
    "row:Semantic probes": "33bb539ab5ad35a6",
    "row:Stale-snapshot characterization": "e71041c6576588cb",
    "row:Cache and lifecycle contract": "4f84bd4462fd0ead",
    "row:Acyclic test specification": "28b00edf6d6eabff",
    "row:Five projection classes": "9ae1f8fbf44d2551",
    "row:Ratified ownership decisions": "e43d057f4b2b867c",
    "row:Concrete deletion and survivor rows": "b6454f7b54fd3222",
    "row:Consolidated ordering and aliasing probe": "55e4400b09120203",
    "residue:TCM0-R-HANG-TOPOLOGY": "77ac2d6f676393a0",
    "residue:TCM0-R-TOPOLOGY-SELECTION": "b8ac88c41ad21f70",
    "residue:TCM0-R-IMPLEMENTATION-BASELINE": "7149f91f1d78ca00",
    "control:CTL-identity-alias.mutation": "b94206db0ef7943c",
    "control:CTL-identity-alias.observed": "b78f3d42701bd5fc",
    "control:CTL-lineage-content.mutation": "c56331b235a231b5",
    "control:CTL-lineage-content.observed": "421f6db08af85b34",
    "control:CTL-profile-set.mutation": "c4e886eaad4dfd4f",
    "control:CTL-profile-set.observed": "931a31b8fdf955c1",
    "control:CTL-profile-multiplicity.mutation": "091fe63562e293eb",
    "control:CTL-profile-multiplicity.observed": "bc5039d5a070964b",
    "control:CTL-authority-edge.mutation": "d9361439d4f8d039",
    "control:CTL-authority-edge.observed": "6c63fa6cfd680cf6",
    "control:CTL-ledger-row.mutation": "a943296a6da74117",
    "control:CTL-ledger-row.observed": "edd96f2740b2f6e5",
    "control:CTL-register-status.mutation": "5fdac66ab9b1e9cd",
    "control:CTL-register-status.observed": "e8260c5fc2be6583",
    "control:CTL-contract-section.mutation": "a06e1efd6b603f29",
    "control:CTL-contract-section.observed": "bb4ea40205688bd2",
    "control:CTL-targeted-selector.mutation": "047fb469b46c266f",
    "control:CTL-targeted-selector.observed": "e4770a8e67b80183",
    "receiving:TCM0-R-HANG-TOPOLOGY#1.gate": "57e7767afc1540ec",
    "receiving:TCM0-R-HANG-TOPOLOGY#2.gate": "84ed714c5d7c6754",
    "receiving:TCM0-R-TOPOLOGY-SELECTION#1.gate": "5db4575360f05438",
    "receiving:TCM0-R-TOPOLOGY-SELECTION#2.gate": "c46be133f185706f",
    "receiving:TCM0-R-IMPLEMENTATION-BASELINE#1.gate": "a3df0ec8a5ff47c8",
    "receiving:TCM0-R-IMPLEMENTATION-BASELINE#2.gate": "72e40192a87f49ab",
    "receiving:TCM0-R-IMPLEMENTATION-BASELINE#3.gate": "91ad285059593c36",
    "receiving:TCM0-R-IMPLEMENTATION-BASELINE#4.gate": "a97f53c6ac04b2c6",
    "proof:P-targeted-domain.skip_basis": "783b55e844e29576",
  }),
  /**
   * Where each atom's evidence and contract bindings POINT, pinned separately
   * from what the atom says.
   *
   * The statement pin covers the proposition; these three fields decide which
   * artifact is allowed to prove it and which contract sentence it rests on,
   * and they were the half left author-controlled. Repointing
   * `evidence_anchor` moves an atom onto whatever a green record happens to
   * touch, and the relevance gate then passes for a reason nobody chose;
   * repointing `contract_section`/`contract_anchor` leaves a pinned statement
   * describing a sentence the contract no longer states, with the statement pin
   * silent because the statement's own bytes never moved. Both are pinned here
   * and matched exactly, so a repoint is a line of the same review.
   *
   * `shipped_obligation` rides in the same digest: it says whether the shipped
   * code meets the atom, and when it does not it names the production path the
   * obligation is unmet at, which is what binds that carry to a receiving owner
   * able to change it. Moving it silently would move the ownership question the
   * remainder was approved to carry — or, in the other direction, retire a
   * disclosed remainder into a bare `met` without anyone reading the bytes that
   * are supposed to have made it true.
   */
  anchors: Object.freeze({
    "atom:A-plane-boundary": "5e259f71f21362ba",
    "atom:A-no-oracle-callback": "39533d1ecef821a4",
    "atom:A-mapper-total": "3a6ec24cce6be447",
    "atom:A-projection-classes-closed": "251f3abddf936019",
    "atom:A-topology-selected": "a66a6fd6829f9e1b",
    "atom:A-basis-not-a-cache-key": "18a36d9efa8ddd64",
    "atom:A-query-identity-snapshot-independent": "f0392eb21461247c",
    "atom:A-flight-key-strictly-larger": "780f237ee3c9ffb1",
    "atom:A-profiles-in-query-identity": "aa8e373d70962407",
    "atom:A-profile-multiplicity-is-one-question": "8329883e62173f86",
    "atom:A-unit-lineage-stable": "33ead0cce0d0ff33",
    "atom:A-identity-alias-is-compile-error": "95037b27781426a7",
    "atom:A-binding-is-a-witness": "993f3b17df666147",
    "atom:A-no-unbound-semantic-result": "5ba534a8afa0c15e",
    "atom:A-publication-requires-live-basis": "1e6b84e939eb9a69",
    "atom:A-degraded-never-warms": "4208e5e279a38f0b",
    "atom:A-slot-keyed-by-query-identity": "82dd558e4ec8ded8",
    "atom:A-incremental-equals-fresh": "0190723f0a685060",
    "atom:A-hang-topology-characterised": "bf5bfe81f7b8b6e6",
    "atom:A-single-owner-per-capability": "ebad966ae9e77e34",
    "atom:A-no-dual-running-window": "df4bd111513761f6",
    "atom:A-deletion-rows-concrete": "65d8c40a4bdff8c3",
    "atom:A-survivor-rows-received": "23456a9425a7813e",
    "atom:A-residue-owner-non-circular": "bd95f7172370381b",
    "atom:A-receiving-criterion-direct": "f14dad9aaaaffe87",
    "atom:A-resolution-gate-named": "319584be579b7431",
    "atom:A-authority-consistent": "5d5ff2e2c560f00d",
    "atom:A-downstream-owner-restated": "160b8a5f5836c812",
    "atom:A-fixtures-named": "0e3edec8ef1e64d3",
    "atom:A-command-exact": "0e3edec8ef1e64d3",
    "atom:A-terminal-summary-present": "0e3edec8ef1e64d3",
    "atom:A-node-evidence-re-executed": "0e3edec8ef1e64d3",
    "atom:A-external-refresh-lane-bound": "4b6ca8d3584feb48",
    "atom:A-counters-consistent": "0e3edec8ef1e64d3",
    "atom:A-zero-unexpected-skips": "0e3edec8ef1e64d3",
    "atom:A-nonzero-selected-work": "0e3edec8ef1e64d3",
    "atom:A-targeted-domain-green": "0d355931223fddb3",
    "atom:A-implementation-baseline": "289bafda107b9ddc",
    "atom:A-status-derived-not-authored": "116c0cc9547f220b",
    "atom:A-adapter-allowlisted": "116c0cc9547f220b",
    "atom:A-portable-controls-only": "116c0cc9547f220b",
    "atom:A-human-view-generated": "491a8f7a2a703c8a",
    "atom:A-check-refuses-unreviewable": "491a8f7a2a703c8a",
    "atom:A-claim-universe-complete": "491a8f7a2a703c8a",
    "atom:A-bounded-needs-transfer": "491a8f7a2a703c8a",
    "atom:A-acyclic-proof-dependency": "491a8f7a2a703c8a",
    "atom:A-proof-relevance-bound": "491a8f7a2a703c8a",
    "atom:A-stated-obligation-is-enforced": "491a8f7a2a703c8a",
    "atom:A-finding-closed-by-atom": "491a8f7a2a703c8a",
    "atom:A-control-mutation-locatable": "491a8f7a2a703c8a",
    "atom:A-controls-cover-every-class": "3d7b5f39f2852f5b",
    "atom:A-ledger-model-intact": "3dd903149d728c59",
  }),
  rows: Object.freeze({
    deletion: Object.freeze([
      "Self-certified closure status",
      "Tracked Python or POSIX control",
      "Name-keyed scanner guard",
      "String mapper plane",
      "Mapper callback into the semantic oracle",
    ]),
    survivor: Object.freeze([
      "Repaired package and binary provenance",
      "Mapper captures",
      "Semantic probes",
      "Stale-snapshot characterization",
      "Cache and lifecycle contract",
      "Acyclic test specification",
      "Five projection classes",
      "Ratified ownership decisions",
      "Concrete deletion and survivor rows",
      "Consolidated ordering and aliasing probe",
    ]),
  }),
});

/**
 * The records whose own run EXECUTES the artifact their claim is about, and the
 * reason each one is nevertheless admissible.
 *
 * The acyclicity rule bars a record from covering a claim when the record's
 * command is the verdict for that claim's subject. One record has to sit on the
 * boundary of that rule: a validator's controls have to run the validator, so
 * the suite that adversarially exercises this module necessarily imports it.
 *
 * What makes that admissible is a property of the CASES rather than of the
 * import: a control plants a mutation and passes only when this module REFUSES
 * it, so a permissive module fails it. It is not a property of every case in
 * that file — the suite also holds registration and pin checks that assert over
 * its own declarations, and a baseline that mutates nothing — so the property
 * is enforced there per case rather than asserted of the whole file. Both
 * halves are derived from what the case DID: the plant is counted only when the
 * fixture it produced differs from the baseline, so an empty mutator no longer
 * satisfies it, and the refusal is counted at the places that derive one, so a
 * case that plants and then finds the validator content fails. The non-planting
 * cases are registered through a separate wrapper and their names are pinned in
 * the suite, which is what stops a case that stops planting from migrating
 * quietly into the exempted set.
 *
 * The pin is exact and stale-failing in both directions: a record listed here
 * that no longer executes a subject fails, because a carve-out no one needs is
 * a carve-out no one is reviewing.
 */
export const SUBJECT_EXERCISING_PROOFS = Object.freeze({
  "P-instrument-controls":
    "its suite imports the module its claim is about, which is admissible because every case that suite registers as a control plants a mutation and passes only when that module REFUSES it — a permissive module fails it rather than passing. The suite derives both halves per case from what the case produced: a mutator whose fixture does not differ from the baseline planted nothing, and a case that reached no derived refusal is not a control. The cases which instead assert over its own declarations are registered separately and pinned by name",
});

/** Closed finding universe. An omitted, added, or misrouted entry fails. */
export const MUST_CLOSE_FINDINGS = Object.freeze([
  "C1",
  "C3",
  "C4",
  "C5",
  "C6",
  "C8",
  "AR1",
  "AR2",
  "AR3",
  "AR4",
  "AR5",
  "AR6",
  "AR7",
  "AR8",
  "AR9",
  "AR10",
  "AR11",
  "AR12",
  "AR13",
  "AR14",
  "AR15",
  "AD1",
  "AD2",
  "AD3",
  "AD5",
  "AD6",
  "AD7",
  "AD8",
  "AD9",
  "AD10",
  "AD11",
  "AD12",
]);

/** Closed remainder universe: finding id -> the only residue that may receive it. */
export const RESIDUE_FINDINGS = Object.freeze({
  C2: "TCM0-R-HANG-TOPOLOGY",
  AD4: "TCM0-R-HANG-TOPOLOGY",
  C7: "TCM0-R-TOPOLOGY-SELECTION",
  C9: "TCM0-R-IMPLEMENTATION-BASELINE",
});

export const ALLOWED_RESIDUES = Object.freeze([
  "TCM0-R-HANG-TOPOLOGY",
  "TCM0-R-TOPOLOGY-SELECTION",
  "TCM0-R-IMPLEMENTATION-BASELINE",
]);

/**
 * The pinned remainder topology: which atom leaves through which remainder, and
 * which blocks receive that remainder, in order.
 *
 * The residue set alone is not the shape the authority fixes. It fixes the
 * whole routing — "C2 and AD4, then TCM3 then TCM4", "C7, TCM2 projection then
 * TCM3 semantic topology", "C9, TCM1 through TCM3 pre-change comparisons and
 * TCM4 activated verification". Checking only that a transfer lands on SOME
 * admissible residue, and that a receiving owner is SOME same-train descendant
 * declaring the right role, accepts every permutation of that shape: the
 * topology remainder could be routed to the hang remainder, or the block that
 * must produce the baseline could be dropped from the sequence that receives
 * it, and neither is a defect the derivation would see. Both mappings are
 * therefore pinned here, beside the residue set, and matched exactly.
 */
export const REQUIRED_TRANSFERS = Object.freeze({
  "A-hang-topology-characterised": "TCM0-R-HANG-TOPOLOGY",
  "A-topology-selected": "TCM0-R-TOPOLOGY-SELECTION",
  "A-implementation-baseline": "TCM0-R-IMPLEMENTATION-BASELINE",
});

export const REQUIRED_RECEIVING = Object.freeze({
  "TCM0-R-HANG-TOPOLOGY": Object.freeze(["TCM3", "TCM4"]),
  "TCM0-R-TOPOLOGY-SELECTION": Object.freeze(["TCM2", "TCM3"]),
  "TCM0-R-IMPLEMENTATION-BASELINE": Object.freeze(["TCM1", "TCM2", "TCM3", "TCM4"]),
});

/** The pinned remainder topology the live register is measured against. */
export const LIVE_TOPOLOGY = Object.freeze({
  transfers: REQUIRED_TRANSFERS,
  receiving: REQUIRED_RECEIVING,
});
