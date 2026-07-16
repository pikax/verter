import { BarrelChild } from "./public";

export const barrelComponent = BarrelChild;
export type BarrelContractProps = InstanceType<typeof BarrelChild>["$props"];
export const barrelPropControl: BarrelContractProps["barrelProp"] = "ok";
