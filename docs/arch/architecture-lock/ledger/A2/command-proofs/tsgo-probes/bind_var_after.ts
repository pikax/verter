type IsAny<T> = 0 extends 1 & T ? true : false;
function makeProps() { var x: "a" | "b" = "a"; const f = () => x; x = "b"; return f }
declare const __v: ReturnType<typeof makeProps>;
export const __shape: null = __v;
declare const __a: IsAny<ReturnType<typeof makeProps>>;
export const __isany: null = __a;
