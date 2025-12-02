// Side effects module - executes on import
console.log("Side effect executed");

// Polyfills, global setup, etc.
if (typeof window !== "undefined") {
  (window as any).__APP_INITIALIZED__ = true;
}
