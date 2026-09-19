import { type Component, type ComponentPublicInstance } from "vue";
import Comp, { Comp as Alias } from "@stp6/lib";
import Concrete from "@stp6/lib/concrete";
import * as Lib from "@stp6/lib";

export type Instance = InstanceType<typeof Comp>;

export const instance: Instance = {} as Instance;

export function reset(): void {
  instance.reset();
}

export const stp6HoverTarget: () => void = instance.reset;
export const stp6PropsMsg: string = instance.$props.msg;
export const stp6DefinitionTarget: typeof Comp = Comp;

export const fromSubpath: InstanceType<typeof Concrete> = {} as InstanceType<typeof Concrete>;
export const fromAlias: InstanceType<typeof Alias> = {} as Instance;
export const fromNs: InstanceType<typeof Lib.default> = {} as Instance;
export const fromNsNamed: InstanceType<typeof Lib.Comp> = {} as Instance;

const asComponent: Component = Comp;
const asCpi: ComponentPublicInstance<
  { readonly msg: string },
  { reset(): void },
  {},
  {},
  {},
  { reset: [] }
> = instance;
export const constructed: Instance = new Comp({ msg: "ok" });
void asComponent;
void asCpi;
void constructed;
void fromSubpath;
void fromAlias;
void fromNs;
void fromNsNamed;
void instance.$emit;
void instance.$slots;
void instance.$props;
