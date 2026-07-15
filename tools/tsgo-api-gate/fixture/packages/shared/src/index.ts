// Reached via a project reference from the off-disk carrier.
export interface SharedUser {
  id: number;
  displayName: string;
}

export function makeUser(id: number): SharedUser {
  return { id, displayName: `user-${id}` };
}
