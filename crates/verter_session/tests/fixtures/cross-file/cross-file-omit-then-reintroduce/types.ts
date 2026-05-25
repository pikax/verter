import type { Vendor } from "./vendor";

export interface CarrierProps extends Omit<
  Vendor,
  "state" | "onStateChange" | "renderFallbackValue"
> {
  state: string;
  onStateChange: (next: string) => void;
  renderFallbackValue: () => number;
}
