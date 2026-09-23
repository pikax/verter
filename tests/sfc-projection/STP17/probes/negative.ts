import Comp from "./components/Saver.vue";

export type Instance = InstanceType<typeof Comp>;

// STP17-collision: a handler valid only for a looser channel is still
// checked against the declared callback prop, never waved through.
export const broken: Instance["$props"] = {
  onSave: (id: string): void => {
    void id;
  },
};
