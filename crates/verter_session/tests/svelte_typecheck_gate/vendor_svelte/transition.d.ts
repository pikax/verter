// Vendored minimal `svelte/transition` (5.56) — the `TransitionConfig` type the
// `transition:`/`in:`/`out:` projection checker (`__verter_transition`)
// consumes. A transition function returns a `TransitionConfig` (or a factory of
// one); the projected checker binds the host element instance + params and
// checks the function's call signature against them.
//
// The params types are NAMED exported interfaces (as in the real package), not
// inline object literals — under the TSGO native-preview a wrong-typed value on
// a KNOWN OPTIONAL property of an INLINE imported object type is not flagged,
// whereas a named interface discriminates reliably (the gate relies on this).

export interface TransitionConfig {
  delay?: number;
  duration?: number;
  easing?: (t: number) => number;
  css?: (t: number, u: number) => string;
  tick?: (t: number, u: number) => void;
}

/// The `fly` transition params — a wrong-typed param (`y: "200"`) fails.
export interface FlyParams {
  delay?: number;
  duration?: number;
  x?: number;
  y?: number;
  opacity?: number;
}

/// The `fade` transition params.
export interface FadeParams {
  delay?: number;
  duration?: number;
  easing?: (t: number) => number;
}

// The built-in `fly` transition the gate fixtures exercise.
export function fly(node: Element, params?: FlyParams): TransitionConfig;

// `fade` — a params-friendly transition (its params are fully optional).
export function fade(node: Element, params?: FadeParams): TransitionConfig;
