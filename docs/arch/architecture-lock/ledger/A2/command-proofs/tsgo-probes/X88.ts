type IsAny<T> = 0 extends 1 & T ? true : false;
function makeProps() { OUT: INNER: { try { break INNER } finally { return "a" as const } } return "b" as const }
declare const __v: ReturnType<typeof makeProps>;
export const __shape: null = __v;
declare const __a: IsAny<ReturnType<typeof makeProps>>;
export const __isany: null = __a;
