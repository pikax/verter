import { h } from "vue";
import Comp from "./components/Ratified.vue";

// Wrong-prop dirty twin for the STP8-abi-contamination utility recipe: h() and
// TSX must reject payloads the ratified constructor's props surface rejects. A
// checker-only public shape that stops observing the constructor would leave
// these green, which is exactly the contamination this case forbids.
export const wrongRows = h(Comp, { rows: "not-an-array" });
export const wrongProject = <Comp rows={["x"]} project={(row: number) => row} />;

void wrongRows;
void wrongProject;
