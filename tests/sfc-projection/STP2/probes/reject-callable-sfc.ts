/**
 * Dirty twin for STP2-not-callable: a callable SFC type can be invoked, but
 * InstanceType extraction is not constructor-shaped.
 */
export declare function CallableComp(props?: { msg: string }): { msg: string };

export const called = CallableComp({ msg: "x" });
export type Inst = InstanceType<typeof CallableComp>;
export const stp2HoverTarget = called;
export const stp2DefinitionTarget: typeof CallableComp = CallableComp;
