import DirectChild from "./components/DirectChild.svelte";
import type { ComponentProps } from "svelte";

export const directComponent = DirectChild;
export type DirectContractProps = ComponentProps<typeof DirectChild>;
export const directPropControl: DirectContractProps["contractProp"] = "ok";
