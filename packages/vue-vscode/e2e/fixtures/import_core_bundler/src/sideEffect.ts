// A side-effect-only module (no exports consumed). Importing it must resolve
// the module but create NO template component binding.
globalThis.__sideEffectRan = true;

declare global {
  // eslint-disable-next-line no-var
  var __sideEffectRan: boolean;
}
