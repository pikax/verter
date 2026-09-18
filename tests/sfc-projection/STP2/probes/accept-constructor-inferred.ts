import Comp from "./components/Generic.vue";
import { Foo } from "./foo-control";

export const inferred = new Comp({ test: 0 });
export const inferredFoo = new Foo({ test: 0 });
export const sibling = new Comp({ test: "independent" });

export const inferredValue: number = inferred.value;
export const inferredTest: number = inferred.$props.test;
export const fooValue: number = inferredFoo.value;
export const siblingValue: string = sibling.value;
export const stp2HoverTarget: number = inferred.value;
export const stp2DefinitionTarget: typeof Comp = Comp;
