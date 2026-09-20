export class Comp {
  value = "ok";
}

export const stp12HoverTarget: string = "ok";
export const stp12DefinitionTarget = stp12HoverTarget;

export type Instance = InstanceType<typeof Comp>;

const instance: Instance = new Comp();
void instance.value;
