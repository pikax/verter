import { handleHelpers } from "../../../utils";
import _Name from "./name.ts?raw";
import _Slots from "./slots.ts?raw";
import _TSXPatch from "./tsx-patch.ts?raw";

export const Name = handleHelpers(_Name);
export const Slots = handleHelpers(_Slots);
export const TSXPath = handleHelpers(_TSXPatch);
