import OwnerA from "./components/OwnerA.vue";
import OwnerB from "./components/OwnerB.vue";
import { sharedCount } from "./shared/logic";

export type Instance = InstanceType<typeof OwnerA>;
export const a = new OwnerA({ count: sharedCount });
export const b = new OwnerB({ count: sharedCount });
export const stp4HoverTarget: number = a.bump(1);
export const stp4DefinitionTarget: typeof OwnerA = OwnerA;
void b;
