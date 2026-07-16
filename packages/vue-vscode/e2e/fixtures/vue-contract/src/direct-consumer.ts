import DirectChild from "./components/DirectChild.vue";

export const directComponent = DirectChild;
export type DirectContractProps = InstanceType<typeof DirectChild>["$props"];
export const directPropControl: DirectContractProps["contractProp"] = "ok";
