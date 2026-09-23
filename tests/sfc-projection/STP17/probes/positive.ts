import Comp from "./components/Saver.vue";

export type Instance = InstanceType<typeof Comp>;

export const instance: Instance = {} as Instance;

export const stp17DefinitionTarget: typeof Comp = Comp;

// STP17-collision: a handler for the shared `onSave` key satisfies both the
// declared callback prop and the `save` emit payload.
const onSave = (id: number): void => {
  void id;
};
export const props: Instance["$props"] = { onSave, title: "b" };
export function fire(): void {
  instance.$emit("save", 1);
}

// STP17-overwrite: the effective `title` is the later definite write.
export const effectiveTitle: string | undefined = props.title;

export const stp17HoverTarget: number = instance.saveCount;

// InstanceType of the imported component preserves its public API.
export const api: Instance = instance;
