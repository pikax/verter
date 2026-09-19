import { startProject, startRows } from "./operations";

/**
 * Sequential first-channel ABI: project-then-rows types `row` as unknown,
 * so permuting independent attributes changes contextual types.
 */
export const seqA = startRows([{ id: 1, name: "a" }]).thenProject((row) => row.name);
export const seqB = startProject((row) => row.name).thenRows([{ id: 1, name: "a" }]);
export const seqSame: typeof seqA.value = seqB.value;
export const stp3HoverTarget = seqA;
export const stp3DefinitionTarget = startRows;
