/**
 * @ts-expect-error suite for Svelte advanced generics (script generic="...").
 */
import GenericSelect from "./GenericSelect.svelte";
import GenericField from "./GenericField.svelte";
import GenericDefault from "./GenericDefault.svelte";
import type { ComponentProps } from "svelte";

type StringSelect = (v: string) => void;
// @ts-expect-error number handler not assignable to string select
export const badSelectHandler: StringSelect = (v: number) => {
  void v;
};

type StrFormat = (v: string) => string;
// @ts-expect-error string format with number value pairing
export const badFieldPair: { value: number; format: StrFormat } = {
  value: 1,
  format: (v: string) => v,
};

type DefaultProps = ComponentProps<typeof GenericDefault>;
// @ts-expect-error default T is string
export const badDefault = { value: 99 } as DefaultProps;

void GenericSelect;
void GenericField;
void GenericDefault;
void badSelectHandler;
void badFieldPair;
void badDefault;
