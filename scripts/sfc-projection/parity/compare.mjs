function key(row, stock) {
  return JSON.stringify(
    stock ? [row.file, row.line, row.code] : [row.file, row.line, row.col, row.code],
  );
}

function multiset(rows, stock) {
  const result = new Map();
  for (const row of rows) {
    const id = key(row, stock);
    if (!result.has(id)) result.set(id, []);
    result.get(id).push(row);
  }
  return result;
}

/** Strict multiset, first-file order, and (for two commands) exit equality. */
export function compare(left, right, { stock = false, leftExit, rightExit } = {}) {
  const leftSet = multiset(left, stock);
  const rightSet = multiset(right, stock);
  const differences = [];
  for (const id of new Set([...leftSet.keys(), ...rightSet.keys()])) {
    const leftRows = leftSet.get(id) ?? [];
    const rightRows = rightSet.get(id) ?? [];
    const delta = rightRows.length - leftRows.length;
    if (delta !== 0)
      differences.push({
        class: delta > 0 ? "right-only" : "left-only",
        count: Math.abs(delta),
        left: leftRows,
        right: rightRows,
      });
  }
  const set = differences.length === 0;
  // A global row names no file, so it has no place in the file order.
  const files = (rows) => [
    ...new Set(rows.flatMap((row) => (row.file === null ? [] : [row.file]))),
  ];
  const leftOrder = files(left);
  const rightOrder = files(right);
  const order = JSON.stringify(leftOrder) === JSON.stringify(rightOrder);
  if (!order) differences.push({ class: "order", left: leftOrder, right: rightOrder });
  const exit = leftExit === undefined || rightExit === undefined ? null : leftExit === rightExit;
  return { equal: set && order && exit !== false, gates: { set, order, exit }, differences };
}
