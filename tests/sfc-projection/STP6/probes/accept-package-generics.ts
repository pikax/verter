import SourceComp from "./source/Generic.vue";
import PackedComp from "@stp6/lib/generic";

export const source = new SourceComp({
  rows: [{ id: 1, name: "a" }],
  project: (row) => row.name,
});
export const packed = new PackedComp({
  rows: [{ id: 1, name: "a" }],
  project: (row) => row.name,
});

export const sourceValue: string = source.value;
export const packedValue: string = packed.value;
export const stp6HoverTarget: string = packed.value;
export const stp6DefinitionTarget: typeof PackedComp = PackedComp;

type SourceInst = InstanceType<typeof SourceComp>;
type PackedInst = InstanceType<typeof PackedComp>;
export const packedMatchesSource: PackedInst = {} as SourceInst;
export const sourceMatchesPacked: SourceInst = {} as PackedInst;

packed.$emit("change", "ok");
packed.$emit("update:modelValue", "ok");
source.$emit("change", "ok");
source.$emit("update:modelValue", "ok");

export const packedSlot = {} as Parameters<NonNullable<(typeof packed)["$slots"]["default"]>>[0];
export const packedSlotRowId: number = packedSlot.row.id;
export const packedSlotValue: string = packedSlot.value;
export const packedModel: string | undefined = packed.$props.modelValue;
export const packedRef: string = packed.value;

export const sourceSlot = {} as Parameters<NonNullable<(typeof source)["$slots"]["default"]>>[0];
export const sourceSlotRowId: number = sourceSlot.row.id;
export const sourceSlotValue: string = sourceSlot.value;
export const sourceModel: string | undefined = source.$props.modelValue;
export const sourceRef: string = source.value;

export const callbackOnly = new PackedComp({
  project: (row: { id: number }) => String(row.id),
});
export const callbackOnlyValue: string = callbackOnly.value;
