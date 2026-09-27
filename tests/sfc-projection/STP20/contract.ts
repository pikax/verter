export const acceptedProducts = [
  "PropCheckObligation",
  "CallerAndSetupPropsContract",
  "SpreadCertaintyPolicy",
] as const;

export type AcceptedProduct = (typeof acceptedProducts)[number];
