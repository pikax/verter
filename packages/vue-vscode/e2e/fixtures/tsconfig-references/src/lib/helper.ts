export interface AliasedShape {
  readonly id: number;
  readonly label: string;
}
export function makeShape(id: number, label: string): AliasedShape {
  return { id, label };
}
