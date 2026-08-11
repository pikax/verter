type IsAny<T> = 0 extends 1 & T ? true : false;
type A = { a: number }; type B = { b: number }
function isA(x: A | B): x is A { return "a" in x }
function isB(x: A | B): x is B { return "b" in x }
function makeProps(x: A | B) { return { v: isA(x) ? (isB(x) ? x : "ok") : "no" } }
declare const __v: ReturnType<typeof makeProps>;
export const __shape: null = __v;
declare const __a: IsAny<ReturnType<typeof makeProps>>;
export const __isany: null = __a;
