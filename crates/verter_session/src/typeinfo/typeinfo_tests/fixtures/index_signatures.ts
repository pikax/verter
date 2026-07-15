// @ai-generated - Synthetic index-signature typeinfo fixture.

export type NumericIndexed = { [key: number]: string };
export type SymbolIndexed = { [key: symbol]: number };
export type DualIndexed = {
  [key: string]: number | boolean;
  [key: number]: number;
};

// Numeric lookup against a numeric index signature.
export type NumericLookup = NumericIndexed[42];

// Symbol lookup against a symbol index signature.
export type SymbolLookup = SymbolIndexed[symbol];

// String lookup against a dual index signature must return the string-key
// value type union (not the number-key value type).
export type DualStringLookup = DualIndexed["any-string-here"];

// Numeric lookup against a dual index signature must return the
// numeric-key value type (number takes priority when both signatures match).
export type DualNumberLookup = DualIndexed[0];
