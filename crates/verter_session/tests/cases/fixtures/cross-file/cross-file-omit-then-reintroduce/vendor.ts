export interface Vendor {
  state: number;
  onStateChange: (next: number) => void;
  renderFallbackValue: () => string;
  inherited_member: boolean;
}
