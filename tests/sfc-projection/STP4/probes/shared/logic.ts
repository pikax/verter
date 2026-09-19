export type SharedCount = number;
export const sharedCount: SharedCount = 1;
export function bump(n: SharedCount): SharedCount {
  return n + 1;
}
