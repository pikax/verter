/**
 * @ts-expect-error suite for advanced generic call-site shapes.
 */
import GenericSelect from "./GenericSelect.vue";
import GenericField from "./GenericField.vue";
import GenericDefault from "./GenericDefault.vue";
import GenericList from "./GenericList.vue";

// Handler shapes linked to T = string
type StringSelect = (v: string) => void;
// @ts-expect-error number handler is not assignable to string select
export const badSelectHandler: StringSelect = (v: number) => {
  void v;
};

// GenericField T extends string | number — format must match value
type StrFormat = (v: string) => string;
// @ts-expect-error string formatter not assignable when value is number in a paired bag
export const badFieldPair: { value: number; format: StrFormat } = {
  value: 1,
  format: (v: string) => v,
};

// GenericDefault T = string
type DefaultProps =
  InstanceType<typeof GenericDefault> extends { $props: infer P } ? P : { value?: string };
// @ts-expect-error value should be string under default T
export const badDefaultNum = { value: 99 } as DefaultProps;

// GenericList constraint
type ListProps = InstanceType<typeof GenericList> extends { $props: infer P } ? P : never;
// @ts-expect-error items elements must have id: string
export const badList: ListProps = {
  items: [{ id: 1 as unknown as string }] as never,
};

void GenericSelect;
void GenericField;
void GenericDefault;
void GenericList;
void badSelectHandler;
void badFieldPair;
void badDefaultNum;
void badList;
