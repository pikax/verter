// A body may not assume a member absent from the binder constraint.
export function illegalBody<T extends { id: number }>(row: T) {
  return row.missing;
}
