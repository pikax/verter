/**
 * Dirty twin: a broad any constructor accepts toFixed under a string project.
 * This ABI is rejected; Coupled.vue.d.ts is the control.
 */
export declare class AnyComp {
  constructor(props?: {
    readonly rows?: any;
    readonly project?: any;
    readonly onChange?: (value: any) => void;
    readonly modelValue?: any;
  });
  readonly value: any;
}

export const acceptedToFixed = new AnyComp({
  rows: [{ id: 1, name: "a" }],
  project: (row: { name: string }) => row.name,
  onChange: (value) => value.toFixed(2),
});
export const stp3HoverTarget = acceptedToFixed;
export const stp3DefinitionTarget: typeof AnyComp = AnyComp;
