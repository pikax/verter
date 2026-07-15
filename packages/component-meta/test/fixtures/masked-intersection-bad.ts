// A compilable module whose exported interface member references a type
// that does not exist anywhere: `Bad.x` is a genuinely unresolvable member
// value used by the masked same-name intersection fixtures.
export interface Bad {
  x: MissingType;
}
