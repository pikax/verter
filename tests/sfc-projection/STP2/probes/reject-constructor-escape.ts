/**
 * Dirty twin: a broad construct overload accepts wrong props.
 * This ABI is rejected; the clean constructor in Concrete.vue.d.ts is the control.
 */
export declare class BroadComp {
  constructor(...args: any[]);
  readonly $props: { readonly msg: string };
  reset(): void;
}

export const acceptedWrong = new BroadComp({ notMsg: true });
export type Escaped = InstanceType<typeof BroadComp>;
export const stp2HoverTarget: Escaped = acceptedWrong;
export const stp2DefinitionTarget: typeof BroadComp = BroadComp;
