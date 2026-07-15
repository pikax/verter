// A SECOND plain `.ts` importer of the same Vue component, so find-all-references
// from one importer spans BOTH `.ts` importers (and the component source through
// the carrier) — the §2.9 cross-file references contract.
import Comp from "./Comp.vue";

export const secondComp = Comp;
