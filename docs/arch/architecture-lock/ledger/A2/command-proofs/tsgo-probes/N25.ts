type IsAny<T> = 0 extends 1 & T ? true : false;
type A = { kind: "a"; a: number }; type B = { kind: "b"; b: number }
function isA(x: A | B): x is A { return x.kind === "a" }
function isB(x: A | B): x is B { return x.kind === "b" }
function makeProps(x: A | B) { return { v: (() => { if (isA(x)) { if (isB(x)) return x; return "ok" as const } return "no" as const })() } }
declare const __v: ReturnType<typeof makeProps>;
export const __shape: null = __v;
declare const __a: IsAny<ReturnType<typeof makeProps>>;
export const __isany: null = __a;
