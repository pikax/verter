// Vendored minimal `svelte/animate` (5.56) — the `AnimationConfig` type the
// `animate:` projection checker (`__verter_animate`) consumes. An animate
// function receives the element + from/to rects + params and returns an
// `AnimationConfig`.
//
// The params type is a NAMED exported interface (as in the real package), not an
// inline object literal — see the note in `transition.d.ts` on why the gate
// requires named params to discriminate wrong-typed values reliably.

export interface AnimationConfig {
  delay?: number;
  duration?: number;
  easing?: (t: number) => number;
  css?: (t: number, u: number) => string;
  tick?: (t: number, u: number) => void;
}

/// The `flip` animation params — a wrong-typed param (`delay: "0"`) fails.
export interface FlipParams {
  delay?: number;
  duration?: number;
  easing?: (t: number) => number;
}

// The built-in `flip` animation the gate fixtures exercise.
export function flip(
  node: Element,
  directions: { from: DOMRect; to: DOMRect },
  params?: FlipParams,
): AnimationConfig;
