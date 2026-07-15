// @ai-generated - Synthetic unique-symbol typeinfo fixture.

export declare const brandTag: unique symbol;

export type Branded = {
  [brandTag]: "branded";
  payload: string;
};

// Indexed access via the unique symbol's typeof identity.
export type BrandValue = Branded[typeof brandTag];

// Direct member access via the labelled brand surface.
export type BrandPayload = Branded["payload"];

// keyof should include the unique-symbol key as `typeof brandTag` (the
// distinguishable identity), alongside string keys.
export type BrandedKeys = keyof Branded;
