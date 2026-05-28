// @ai-generated - Fixtures pinning the empty-path Shallow projection's
// COMPLETE surface fact set + the P2-1 heritage-vs-authored merge rule.
//
// Synthetic + hermetic. Generic names encode resolver contracts, not any
// external library.

// A hybrid surface: named members AND a call signature AND a construct
// signature AND an index signature. The empty-path Shallow projection
// historically DROPPED everything except the named members; these fixtures
// assert all four fact classes survive.
export interface HybridSurface {
  named: string;
  flag?: boolean;
  (token: string): number;
  new (seed: number): HybridSurface;
  [dynamic: string]: unknown;
}

// A pure call-signature carrier (no named members) — the surface must
// publish the call signature, not collapse to an empty / unknown object.
export interface CallOnly {
  (value: string): boolean;
}

// An indexed-only surface — `has_index_signature` + the index key/value
// nodes must survive.
export interface IndexedOnly {
  [key: string]: number;
}

// ---------------------------------------------------------------------------
// P2-1: interface heritage SHADOWS; authored intersection INTERSECTS.
// ---------------------------------------------------------------------------

export interface HeritageBase {
  dup: number;
  baseOnly: number;
}

// REAL interface heritage (extends): the derived `dup: string` SHADOWS the
// inherited `dup: number`. Observable `HeritageDerived['dup']` is `string`.
//
// `nested` is an OBJECT-ALIAS-typed member: under the shallow-by-default rule
// its published `value` is a reference carrier, NOT the expanded
// `{ dup, baseOnly }` object surface. This makes the "Shallow, not Expanded"
// proof DISCRIMINATING — an eager / Expanded projection WOULD materialise
// `nested` into an `Object` node.
export interface HeritageDerived extends HeritageBase {
  dup: string;
  derivedOnly: string;
  nested: HeritageBase;
}

// AUTHORED intersection (`&`): the own `dup: string` does NOT shadow the
// referenced `HeritageBase.dup: number`. Observable `AuthoredIntersection['dup']`
// is `number & string` (the intersection of both arms).
export type AuthoredIntersection = HeritageBase & {
  dup: string;
  authoredOnly: string;
};

// ---------------------------------------------------------------------------
// Union common-member merge: only members present in EVERY arm survive; the
// value is the union of per-arm values; optional if any arm optional;
// readonly if all arms readonly.
// ---------------------------------------------------------------------------

export interface UnionArmA {
  shared: string;
  onlyA: number;
  readonly ro: number;
}

export interface UnionArmB {
  shared: number;
  onlyB: boolean;
  ro: number;
}

export type CommonMembers = UnionArmA | UnionArmB;
