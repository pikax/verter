// Vendored minimal `svelte/elements` — the Svelte-true JSX intrinsic table.
// Lowercase event attributes (`onclick`/`onchange`/`onintrostart`), a typed
// `currentTarget`, and the `ClassValue` clsx type. Minimal-yet-discriminating:
// the lowercase casing proves the Svelte table is in effect (Vue/React casing
// would reject `onclick` as a literal attribute), `onintrostart` is a
// Svelte-specific attribute Vue's JSX rejects, and `currentTarget` carries the
// element type so `e.currentTarget.value` checks on an `<input>` but a
// canvas-only method is rejected.

// The clsx-style class value (5.16): string, array, or record of booleans.
export type ClassValue =
  | string
  | number
  | boolean
  | null
  | undefined
  | ClassValue[]
  | {
      [key: string]: unknown;
    };

interface DOMEvent<T extends EventTarget> {
  readonly currentTarget: T;
  readonly target: EventTarget | null;
}

interface SvelteBaseAttributes<T extends EventTarget> {
  // Children: text, expressions, nested elements/snippets — permissive in the
  // vendored stub (the real `svelte/elements` types this precisely).
  children?: unknown;
  class?: ClassValue;
  id?: string;
  // Svelte-5 lowercase event attributes (the primary event path).
  onclick?: (event: DOMEvent<T> & MouseEvent) => void;
  onchange?: (event: DOMEvent<T> & Event) => void;
  oninput?: (event: DOMEvent<T> & Event) => void;
  onkeydown?: (event: DOMEvent<T> & KeyboardEvent) => void;
  // A Svelte-specific transition attribute the Vue/React JSX tables reject.
  onintrostart?: (event: DOMEvent<T> & Event) => void;
  onoutroend?: (event: DOMEvent<T> & Event) => void;
  // CSS custom properties pass through as data-* in the projection; allow any
  // `data-*` so the projected `data-v…` attribute checks.
  [dataAttr: `data-${string}`]: unknown;
}

interface HTMLInputAttributes<T extends EventTarget> extends SvelteBaseAttributes<T> {
  value?: string | number;
  type?: string;
  checked?: boolean;
}

interface HTMLSelectAttributes<T extends EventTarget> extends SvelteBaseAttributes<T> {
  value?: string | number;
  // Customizable `<select>` (5.47) — a typed boolean for the gate fixture.
  multiple?: boolean;
}

// Vendored minimal `SVGAttributes` — the SVG-namespace attribute table the
// F10 `@verter/svelte-jsx/svg` shim sources. DISCRIMINATING: it carries
// SVG-specific presentation attributes (`cx`/`cy`/`r`/`fill`/`d`/`points`) and
// shared globals (`class`/`id`/`onclick`), but NO HTML form attributes
// (`value`/`checked`/`type`) — so an HTML-only attribute on an svg element
// FAILS, proving the svg table replaced the HTML table.
export interface SVGAttributes<T extends EventTarget> {
  children?: unknown;
  class?: ClassValue;
  id?: string;
  // Shared SVG presentation globals.
  fill?: string;
  stroke?: string;
  "stroke-width"?: number | string;
  transform?: string;
  // Geometry (subset — enough for the gate fixtures).
  cx?: number | string;
  cy?: number | string;
  r?: number | string;
  x?: number | string;
  y?: number | string;
  width?: number | string;
  height?: number | string;
  d?: string;
  points?: string;
  viewBox?: string;
  onclick?: (event: DOMEvent<T> & MouseEvent) => void;
  [dataAttr: `data-${string}`]: unknown;
}

export interface SvelteHTMLElements {
  div: SvelteBaseAttributes<HTMLDivElement>;
  span: SvelteBaseAttributes<HTMLSpanElement>;
  button: SvelteBaseAttributes<HTMLButtonElement>;
  input: HTMLInputAttributes<HTMLInputElement>;
  select: HTMLSelectAttributes<HTMLSelectElement>;
  option: SvelteBaseAttributes<HTMLOptionElement>;
  canvas: SvelteBaseAttributes<HTMLCanvasElement>;
  ul: SvelteBaseAttributes<HTMLUListElement>;
  li: SvelteBaseAttributes<HTMLLIElement>;
  p: SvelteBaseAttributes<HTMLParagraphElement>;
  // Media / dimension / details bindings (F4) need these intrinsic tags to be
  // valid JSX elements (the bindings themselves are checked via the prelude
  // `__verter_bind_*` helpers, not these attribute records).
  video: SvelteBaseAttributes<HTMLVideoElement>;
  audio: SvelteBaseAttributes<HTMLAudioElement>;
  img: SvelteBaseAttributes<HTMLImageElement>;
  details: SvelteBaseAttributes<HTMLDetailsElement>;
}
