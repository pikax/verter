export class Comp {
  readonly n: number = 1;
}

export type Instance = InstanceType<typeof Comp>;

export const instance: Instance = new Comp();

/** STP1-hover */
export const stp1HoverTarget: number = instance.n;

export const stp1DefinitionTarget: typeof Comp = Comp;
