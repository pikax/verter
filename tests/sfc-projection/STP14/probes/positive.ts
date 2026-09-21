import Comp, { order, stp14Marker, Tag } from "./components/Capture.vue";

export type Instance = InstanceType<typeof Comp>;

export const stp14DefinitionTarget: typeof Comp = Comp;

// A local alias lifted under the component binder keeps `A["id"]` bound to
// the very parameter the use supplies.
const bound = new Comp({
  rows: [{ id: 1 }],
  selected: { item: { id: 1 }, key: 1 },
  tag: Tag,
  marker: stp14Marker,
  order,
});

export const boundFirst: number | undefined = bound.first?.id;
export const stp14HoverTarget: number = boundFirst === undefined ? 0 : 1;

// The later binder default still refers to the earlier parameter.
export const dependentDefault: Instance["$props"]["rows"] = [{ id: 2 }];

// The captured value-space dependencies are nameable from outside.
export const marker: typeof stp14Marker = stp14Marker;
export const tagConstructor: typeof Tag = Tag;
export const ordered: number = order([{ id: 3 }]);
