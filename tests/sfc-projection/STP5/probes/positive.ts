export class Comp {
  readonly n: number = 1;
  readonly greeting: string = "hi 😀";
  readonly crlf: string = "a\r\nb";
}

export type Instance = InstanceType<typeof Comp>;

export const instance: Instance = new Comp();

/** STP5-hover */
export const stp5HoverTarget: number = instance.n;

export const stp5DefinitionTarget: typeof Comp = Comp;

export const stp5Emoji: string = instance.greeting;
