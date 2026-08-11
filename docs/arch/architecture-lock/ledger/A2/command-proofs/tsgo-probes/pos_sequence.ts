type IsAny<T> = 0 extends 1 & T ? true : false;
function sink(v: boolean) { }
function makeProps() { let x: "a" | "b" = "a"; ((() => { x = "b"; return true })(), 0); return x }
declare const __v: ReturnType<typeof makeProps>;
export const __shape: null = __v;
declare const __a: IsAny<ReturnType<typeof makeProps>>;
export const __isany: null = __a;
