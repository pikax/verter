import { ConstrainedFoo } from "./foo-control";

/** Constraint: boolean does not extend string | number. */
export const wrong = new ConstrainedFoo({ test: true });
export const stp2HoverTarget = wrong;
export const stp2DefinitionTarget: typeof ConstrainedFoo = ConstrainedFoo;
