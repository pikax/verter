import Comp from "./components/Universal.vue";

export type Instance = InstanceType<typeof Comp>;

export const instance: Instance = {} as Instance;

export const stp11DefinitionTarget: typeof Comp = Comp;

// Async setup does not change the instance into a Promise.
export const notPromise: Instance extends PromiseLike<unknown> ? never : true = true;

// Constraint-bounded body: only members of the constraint are available.
export function universalBody<T extends { id: number }>(row: T): number {
  return row.id;
}

// Ordinary TypeScript angle assertion keeps its authored meaning.
export const asserted = <number>(<unknown>1);

export const bound = new Comp({ rows: [{ id: 1, extra: "a" }], pick: (row) => row.id });
export const boundFirst: number | undefined = bound.first?.id;
export const stp11HoverTarget: number = boundFirst === undefined ? 0 : 1;
