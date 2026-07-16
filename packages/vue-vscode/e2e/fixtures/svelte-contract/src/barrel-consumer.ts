import { BarrelChild } from "./public";
import type { ComponentProps } from "svelte";

export const barrelComponent = BarrelChild;
export type BarrelContractProps = ComponentProps<typeof BarrelChild>;
export type BarrelFrameworkProps = ComponentProps<typeof BarrelChild>;
export const barrelPropControl: BarrelContractProps["barrelProp"] = "ok";
export const barrelFrameworkPropControl: BarrelFrameworkProps["barrelProp"] = "ok";

// @ts-expect-error - the native ComponentProps carrier must retain the authored string prop.
export const invalidBarrelInstanceProp: BarrelContractProps["barrelProp"] = 42;
// @ts-expect-error - Svelte's public ComponentProps helper must see the same prop.
export const invalidBarrelFrameworkProp: BarrelFrameworkProps["barrelProp"] = 42;
