export class Comp {
  readonly n: number = 1;
}

export type Instance = InstanceType<typeof Comp>;

export const instance: Instance = new Comp();

/** STP1-anchor-negative: number is not assignable to string (TS2322). */
export const stp1Negative: string = instance.n;
