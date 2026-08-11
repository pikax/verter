type IsAny<T> = 0 extends 1 & T ? true : false;
function sink(v: boolean) { }
function makeProps(k: number) { let x: "a" | "b" = "a"; switch (k) { case 1: (() => { x = "b"; return true })(); break; default: break } return x }
declare const __v: ReturnType<typeof makeProps>;
export const __shape: null = __v;
declare const __a: IsAny<ReturnType<typeof makeProps>>;
export const __isany: null = __a;
