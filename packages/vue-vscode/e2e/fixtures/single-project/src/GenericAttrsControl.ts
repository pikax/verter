// The plain-TypeScript ORACLE for the `generic=` / `attrs=` delegation tests.
//
// `GenericAttrsComp.vue` declares those two script-setup attributes, and they
// lower to exactly the signature below in the generated IDE surface. A hover
// inside either attribute value must therefore behave like a hover on the
// matching token here. The tests compare the two instead of asserting a
// hand-written string, because the interesting failure is "the carrier answers
// something DIFFERENT from plain TypeScript", and a literal expectation cannot
// tell that apart from "the expectation was typed wrong".
//
// It also pins the negative half. TypeScript returns no quickinfo for a
// primitive keyword type node, so hovering the constraint keyword itself is
// empty HERE too. Without this file that emptiness in a `.vue` reads like a
// mapping defect; with it, it reads as the parity it actually is.
//
// Nothing above may repeat the tests' anchor text verbatim: they locate their
// cursor with a plain substring search over the whole file, so a prose copy of
// an anchor wins over the code and the probe lands in this comment. That is not
// hypothetical — it is what the first version of this file did, and the oracle
// guard in the suite is what caught it.
export function genericAttrsControl<T extends string>(_attrs: { class: string; id?: string }) {
  void _attrs;
  return {} as T;
}
