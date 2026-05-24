// @ai-generated - Synthetic indexed-access-with-union-key typeinfo fixture.

export type Surface = {
  alpha: number;
  beta: string;
  gamma: boolean;
  delta: null;
};

export type AlphaBeta = Surface["alpha" | "beta"];
export type EveryMember = Surface[keyof Surface];

// Pick-style equivalent: Pick<Surface, "alpha" | "beta"> = { alpha; beta }.
export type PickAlphaBeta = Pick<Surface, "alpha" | "beta">;
