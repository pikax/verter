interface TypeDiagnosticsSourceMapInput {
  typesSourceMap?: string;
}

export function getTypeDiagnosticsSourceMap(
  compiled: TypeDiagnosticsSourceMapInput,
): string | null {
  const map = compiled.typesSourceMap;
  if (!map) return null;
  return map.length > 2 ? map : null;
}
