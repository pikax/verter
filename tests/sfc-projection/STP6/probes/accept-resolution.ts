import Root from "@stp6/lib";
import Concrete from "@stp6/lib/concrete";
import Generic from "@stp6/lib/generic";

export type Instance = InstanceType<typeof Root>;
export const stp6HoverTarget: () => void = ({} as Instance).reset;
export const stp6DefinitionTarget: typeof Root = Root;
void Concrete;
void Generic;
export const constructed: Instance = new Root({ msg: "ok" });
void constructed;
