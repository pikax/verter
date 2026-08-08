// @ai-generated - Rootless-callable `.call` fixture.
//
// Both callables below are ROOTLESS: a function-typed parameter and a local
// arrow have no authored top-level declaration anchor, so their signatures
// carry no occurrence. `.call` on them resolves through the ambient
// `Function` surface of the project containing the call site (the lexical
// demand canonical), then rebases onto the extracted callable.

// (1) A function-typed PARAMETER.
export function callParam(fn: (x: string) => 1) {
  return fn.call(undefined, "x");
}
export type ParamCallResult = ReturnType<typeof callParam>;

// (2) A LOCAL arrow.
export function callLocalArrow() {
  const local = (x: string): 1 => 1;
  return local.call(undefined, "x");
}
export type LocalArrowCallResult = ReturnType<typeof callLocalArrow>;
