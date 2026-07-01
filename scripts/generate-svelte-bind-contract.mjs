#!/usr/bin/env node
/*
  Generates
  crates/verter_compiler/src/svelte/bind_contract_data.rs
  from the CLOSED Svelte-5-documented binding-name vocabulary authored below.

  This registry IS the SHARED source of truth for the wide `bind:` family: for
  every documented element binding name it pins the bound value's TS TYPE and
  its DIRECTION (read / write / read-write) plus the element/tag constraint that
  selects it (the IDE columns), AND the RUNTIME emission metadata — which
  `$.bind_*` helper / `bind_property` form the client backend calls, the call
  ARITY, the `bind_property` EVENT name, the prelude CLEANUP, and the
  should_proxy policy (the runtime columns). The Svelte IDE projector consults
  the value type + direction to emit a type-checked assignment-compatibility
  check in the projected `.svelte.tsx`; the runtime client emitter consults the
  runtime metadata to emit the DATA-DRIVEN `$.bind_*` call. One authored
  registry is the authority for both consumers.

  Directions (from the bound LOCAL's perspective):
    - "rw"  read-write: Svelte both reads the local to set the DOM and writes
            DOM changes back into the local → the local is INVARIANT with `V`.
    - "r"   read-direction (readonly DOM property, DOM → local only): the local
            RECEIVES `V` from the DOM and can never write back → `V` must be
            assignable to the local; a userland write to the binding is rejected.

  `bind:this` and `bind:group` are SPECIAL (host-instance assignment-compat /
  checkbox-vs-radio array shape) and carry a `special` marker so the projector
  routes them to their dedicated checkers rather than the generic value-type
  check. Their `value_type` column is documentary.

  The generated file is byte-pinned by
  crates/verter_compiler/tests/svelte_bind_contract_freshness.rs — a registry
  edit without a regen (or a hand-edit of the generated file) fails that gate.
  Regenerate with `node scripts/generate-svelte-bind-contract.mjs`.
*/

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));

// The CLOSED Svelte 5 element-binding vocabulary. Each row:
//   name        the binding local (`value` in `bind:value`)
//   direction   "rw" | "r" | "w"
//   value_type  the TS type of the bound value (a `svelte`/DOM type expression).
//               For `host` value types, `{HOST}` is substituted by the projector
//               with the element's host-instance type (`__VerterHostEl<"tag">`
//               for an intrinsic, `InstanceType<typeof C>` for a component).
//   tags        the applicable lowercase tag set, or "*" for any element, or a
//               "contenteditable" pseudo-constraint (any element with the
//               attribute — the projector does not enforce it, documentary).
//   host_scope  OPTIONAL special-host scope, ORTHOGONAL to `tags`. Absent (the
//               default) ⇒ BindHostScope::Element (regular elements per `tags`, plus
//               `<svelte:body>` / `<svelte:element>` for the universal `*` /
//               `contenteditable` binds — svelte's `invalid_elements: [svelte:window,
//               svelte:document]` rows). "window" / "document" ⇒ the bind is valid ONLY
//               on `<svelte:window>` / `<svelte:document>` (svelte's `valid_elements`).
//               "universal" ⇒ valid on EVERY host incl. the four special hosts (svelte's
//               no-valid/no-invalid rows: `focused`, `this`).
//   special     "this" | "group" | null — routes to a dedicated checker (IDE).
//   policy      OPTIONAL accepted-target-shape policy. Absent (the default) ⇒
//               BindTargetPolicy::LvalueOrFunctionPair (a function-pair {get, set}
//               target is accepted). "identifier_or_member_only" ⇒ a
//               SequenceExpression target is an official reject
//               (bind_group_invalid_expression) — only `bind:group`.
//
// RUNTIME columns (consumed by the native client backend, matching pinned
// svelte@5.56.3 emit shapes):
//   official_helper  the OFFICIAL `svelte@5.56.3` helper IDENTITY — the
//               machine-readable fact of which `$.bind_*` / `bind_property` form the
//               official compiler emits, preserved even for rows the native runtime does
//               NOT emit yet. One of: "value" | "select_value" | "checked" | "group" |
//               "current_time" | "paused" | "played" | "element_size" |
//               "content_editable" | "property" | "this" | "files" | "focused" |
//               "volume" | "muted" | "playback_rate" | "buffered" | "seekable" |
//               "seeking" | "ended" | "ready_state" | "resize_observer". Every value is
//               oracle-verified against the pinned compiler.
//   support     "supported" | "unsupported" — whether the native client runtime currently
//               EMITS the bind. An "unsupported" row keeps its REAL official_helper (never
//               an erased sentinel); the IDE projector still type-checks it, but
//               `resolve_runtime_bind` fails it closed. The official-DOM binds outside the
//               native runtime's current emission set (files/focused/playbackRate/volume/
//               muted, the media-readiness buffered/seekable/seeking/ended/readyState, the
//               resize-observer family, indeterminate, and the media-dimension
//               naturalWidth/naturalHeight/videoWidth/videoHeight) are "unsupported". The
//               implementation that supports one of these rows flips its support +
//               helper routing and adds goldens.
//   arity       "get_set" | "set_only" | "property" — the getter/setter shape.
//   event       the `$.bind_property` event name (only for official_helper "property"),
//               else "".
//   prelude     "none" | "remove_input_defaults" | "remove_textarea_child" —
//               the per-host default-clearing statement emitted before the bind.
//   should_proxy  whether the setter takes the 3rd `true` proxy-flag arg. FALSE
//               for every ordinary DOM bind (the `$.set(…, true)` flag is a 5f
//               component/window-host policy, never an ordinary DOM setter).
const REGISTRY = [
  // bind:this — host-instance assignment-compat (read-direction). value_type is
  // documentary; the projector substitutes the host-instance type and routes to
  // the `this` checker. Runtime routing is host-specific (element host = 5c,
  // component host = 5f); the helper is the dedicated `$.bind_this`.
  {
    name: "this",
    direction: "r",
    value_type: "{HOST}",
    tags: "*",
    // `this: { omit_in_ssr }` has no valid/invalid_elements ⇒ valid on EVERY host
    // (regular elements + all four special hosts). The host-specific `$.set(…, true)`
    // proxy flag is HOST-driven at projection (false on a regular element, true on a
    // special host), so the row's `should_proxy` stays the element baseline `false`.
    host_scope: "universal",
    special: "this",
    official_helper: "this",
    support: "supported",
    arity: "get_set",
    event: "",
    prelude: "none",
    should_proxy: false,
  },

  // bind:group — checkbox-vs-radio shared selection. Routed to the `group`
  // checker which inspects the sibling `type` attribute; value_type documentary.
  // Runtime: `$.bind_group(binding_group, [], el, get, set)`.
  {
    name: "group",
    direction: "rw",
    value_type: "unknown",
    tags: "input",
    special: "group",
    // `bind:group` is the SOLE identifier/member-only bind: official throws
    // `bind_group_invalid_expression` for ANY function-pair (SequenceExpression)
    // target (verified svelte@5.56.3 BindDirective.js — the throw precedes the
    // two-element length check). Every other bind defaults to LvalueOrFunctionPair.
    policy: "identifier_or_member_only",
    official_helper: "group",
    support: "supported",
    arity: "get_set",
    event: "",
    prelude: "remove_input_defaults",
    should_proxy: false,
  },

  // bind:files — the FileList written to/read from a file input. Official emits the
  // DEDICATED `$.bind_files(input, get, set)` (get/set). The native client runtime does
  // not emit it yet (`support: "unsupported"` ⇒ fails closed); the official helper
  // identity is preserved. The IDE row stays real.
  {
    name: "files",
    direction: "rw",
    value_type: "FileList | null",
    tags: "input",
    special: null,
    official_helper: "files",
    support: "unsupported",
    arity: "get_set",
    event: "",
    prelude: "none",
    should_proxy: false,
  },

  // bind:focused — whether the element currently holds focus (read-direction;
  // DOM → local, set on focus/blur). Official emits the DEDICATED
  // `$.bind_focused(el, set)` (universal.js) — verified svelte@5.56.3 emits
  // `$.bind_focused(input, ($$value) => $.set(x, $$value))`. `focused: {}` in the
  // official `binding_properties` registry carries no `valid_elements`, so it
  // applies to ANY element (`*`); the svelte attribute type is `readonly` (a
  // read-direction binding). The native client runtime EMITS it (`support:
  // "supported"`): a regular element emits `$.bind_focused(el, ($$value) => $.set(x,
  // $$value))` and the `<svelte:window>` host emits `$.bind_focused($.window,
  // ($$value) => $.set(x, $$value, true))` (the proxy flag is HOST-driven — see the
  // `should_proxy` note below). The IDE row + the runtime row share this contract.
  {
    name: "focused",
    direction: "r",
    value_type: "boolean",
    // `focused: {}` has no valid/invalid_elements ⇒ valid on ANY host (regular
    // elements + all four special hosts incl. `<svelte:window>`). The native client
    // runtime emits the dedicated `$.bind_focused(host, set)` (universal.js); the host
    // expression is the element var on a regular element and `$.window` on the window
    // host, and the `$.set(…, true)` proxy flag is HOST-driven at projection (false on a
    // regular element, true on the window host), so the row's `should_proxy` stays the
    // element baseline `false`.
    tags: "*",
    host_scope: "universal",
    special: null,
    official_helper: "focused",
    support: "supported",
    arity: "set_only",
    event: "",
    prelude: "none",
    should_proxy: false,
  },

  // bind:indeterminate — the checkbox indeterminate boolean (read-write). Its official
  // helper IS the generic `$.bind_property('indeterminate','change',el,set,get)`. The
  // native client runtime does not emit it yet (`support: "unsupported"` ⇒ fail closed);
  // refusal rides the support status, NOT the helper identity (the official helper is the
  // generic property form, which the runtime CAN emit — only support gates it). The IDE
  // row stays real (the projector type-checks the bind via value_type/direction); the
  // implementation that supports it flips support + adds goldens.
  {
    name: "indeterminate",
    direction: "rw",
    value_type: "boolean",
    tags: "input",
    special: null,
    official_helper: "property",
    support: "unsupported",
    arity: "property",
    event: "change",
    prelude: "none",
    should_proxy: false,
  },

  // <details bind:open> — the open boolean. `$.bind_property('open','toggle',el,set,get)`.
  {
    name: "open",
    direction: "rw",
    value_type: "boolean",
    tags: "details",
    special: null,
    official_helper: "property",
    support: "supported",
    arity: "property",
    event: "toggle",
    prelude: "none",
    should_proxy: false,
  },

  // contenteditable bindings — the element text content as a string.
  // `$.bind_content_editable('innerHTML'|'innerText'|'textContent', el, get, set)`.
  {
    name: "innerHTML",
    direction: "rw",
    value_type: "string",
    tags: "contenteditable",
    special: null,
    official_helper: "content_editable",
    support: "supported",
    arity: "get_set",
    event: "",
    prelude: "none",
    should_proxy: false,
  },
  {
    name: "innerText",
    direction: "rw",
    value_type: "string",
    tags: "contenteditable",
    special: null,
    official_helper: "content_editable",
    support: "supported",
    arity: "get_set",
    event: "",
    prelude: "none",
    should_proxy: false,
  },
  {
    name: "textContent",
    direction: "rw",
    value_type: "string",
    tags: "contenteditable",
    special: null,
    official_helper: "content_editable",
    support: "supported",
    arity: "get_set",
    event: "",
    prelude: "none",
    should_proxy: false,
  },

  // Writable media bindings (HTMLMediaElement; read-write). currentTime/paused are
  // supported, with dedicated helpers. playbackRate/volume/muted have dedicated OFFICIAL
  // helpers (`$.bind_playback_rate` / `$.bind_volume` / `$.bind_muted`, all get/set) that
  // the native client runtime does not emit yet (`support: "unsupported"` ⇒ fail closed);
  // their REAL official helper identity is preserved (a generic `$.bind_property` form
  // would be the wrong helper). The implementation that supports them flips support.
  {
    name: "currentTime",
    direction: "rw",
    value_type: 'HTMLMediaElement["currentTime"]',
    tags: "audio,video",
    special: null,
    official_helper: "current_time",
    support: "supported",
    arity: "get_set",
    event: "",
    prelude: "none",
    should_proxy: false,
  },
  {
    name: "playbackRate",
    direction: "rw",
    value_type: 'HTMLMediaElement["playbackRate"]',
    tags: "audio,video",
    special: null,
    official_helper: "playback_rate",
    support: "unsupported",
    arity: "get_set",
    event: "",
    prelude: "none",
    should_proxy: false,
  },
  {
    name: "volume",
    direction: "rw",
    value_type: 'HTMLMediaElement["volume"]',
    tags: "audio,video",
    special: null,
    official_helper: "volume",
    support: "unsupported",
    arity: "get_set",
    event: "",
    prelude: "none",
    should_proxy: false,
  },
  {
    name: "muted",
    direction: "rw",
    value_type: 'HTMLMediaElement["muted"]',
    tags: "audio,video",
    special: null,
    official_helper: "muted",
    support: "unsupported",
    arity: "get_set",
    event: "",
    prelude: "none",
    should_proxy: false,
  },
  {
    name: "paused",
    direction: "rw",
    value_type: 'HTMLMediaElement["paused"]',
    tags: "audio,video",
    special: null,
    official_helper: "paused",
    support: "supported",
    arity: "get_set",
    event: "",
    prelude: "none",
    should_proxy: false,
  },

  // Readonly media bindings (DOM → local only; a userland write is rejected).
  // `duration` and `played` are supported (the generic `$.bind_property` read-only form /
  // the dedicated `$.bind_played` setter-only helper). The rest
  // (`buffered`/`seekable`/`seeking`/`ended`/`readyState`) have dedicated official helpers
  // (`$.bind_buffered`/`$.bind_seekable`/`$.bind_seeking`/`$.bind_ended`/
  // `$.bind_ready_state`) that the native client runtime does not emit yet
  // (`support: "unsupported"` ⇒ fail closed); their real identity is preserved. The IDE
  // columns stay real; the implementation that supports each flips support + adds goldens.
  {
    name: "duration",
    direction: "r",
    value_type: 'HTMLMediaElement["duration"]',
    tags: "audio,video",
    special: null,
    official_helper: "property",
    support: "supported",
    arity: "property",
    event: "durationchange",
    prelude: "none",
    should_proxy: false,
  },
  {
    name: "buffered",
    direction: "r",
    value_type: 'HTMLMediaElement["buffered"]',
    tags: "audio,video",
    special: null,
    official_helper: "buffered",
    support: "unsupported",
    arity: "set_only",
    event: "",
    prelude: "none",
    should_proxy: false,
  },
  {
    name: "seekable",
    direction: "r",
    value_type: 'HTMLMediaElement["seekable"]',
    tags: "audio,video",
    special: null,
    official_helper: "seekable",
    support: "unsupported",
    arity: "set_only",
    event: "",
    prelude: "none",
    should_proxy: false,
  },
  {
    name: "played",
    direction: "r",
    value_type: 'HTMLMediaElement["played"]',
    tags: "audio,video",
    special: null,
    official_helper: "played",
    support: "supported",
    arity: "set_only",
    event: "",
    prelude: "none",
    should_proxy: false,
  },
  {
    name: "seeking",
    direction: "r",
    value_type: 'HTMLMediaElement["seeking"]',
    tags: "audio,video",
    special: null,
    official_helper: "seeking",
    support: "unsupported",
    arity: "set_only",
    event: "",
    prelude: "none",
    should_proxy: false,
  },
  {
    name: "ended",
    direction: "r",
    value_type: 'HTMLMediaElement["ended"]',
    tags: "audio,video",
    special: null,
    official_helper: "ended",
    support: "unsupported",
    arity: "set_only",
    event: "",
    prelude: "none",
    should_proxy: false,
  },
  {
    name: "readyState",
    direction: "r",
    value_type: 'HTMLMediaElement["readyState"]',
    tags: "audio,video",
    special: null,
    official_helper: "ready_state",
    support: "unsupported",
    arity: "set_only",
    event: "",
    prelude: "none",
    should_proxy: false,
  },

  // Readonly dimension bindings (DOM → local only; number).
  // `$.bind_element_size(el, 'name', set)`.
  {
    name: "clientWidth",
    direction: "r",
    value_type: "number",
    tags: "*",
    special: null,
    official_helper: "element_size",
    support: "supported",
    arity: "set_only",
    event: "",
    prelude: "none",
    should_proxy: false,
  },
  {
    name: "clientHeight",
    direction: "r",
    value_type: "number",
    tags: "*",
    special: null,
    official_helper: "element_size",
    support: "supported",
    arity: "set_only",
    event: "",
    prelude: "none",
    should_proxy: false,
  },
  {
    name: "offsetWidth",
    direction: "r",
    value_type: "number",
    tags: "*",
    special: null,
    official_helper: "element_size",
    support: "supported",
    arity: "set_only",
    event: "",
    prelude: "none",
    should_proxy: false,
  },
  {
    name: "offsetHeight",
    direction: "r",
    value_type: "number",
    tags: "*",
    special: null,
    official_helper: "element_size",
    support: "supported",
    arity: "set_only",
    event: "",
    prelude: "none",
    should_proxy: false,
  },

  // Readonly media-dimension bindings on <img>/<video>. Their official helper IS the
  // generic read-only `$.bind_property` form (naturalWidth/naturalHeight on the `load`
  // event, videoWidth/videoHeight on the `resize` event), but the native client runtime
  // does not emit them yet (`support: "unsupported"` ⇒ fail closed); refusal rides
  // support, NOT the (emittable property) helper. `naturalWidth`/`naturalHeight` are
  // `<img>`-only and `<img>` is NOT in the client element allowlist, so they are
  // unreachable as a bind host (router-level only); `videoWidth`/`videoHeight` are
  // reachable on `<video>` and fail closed at the runtime router. The IDE columns stay
  // real; the implementation that supports each flips support + adds goldens.
  {
    name: "naturalWidth",
    direction: "r",
    value_type: "number",
    tags: "img",
    special: null,
    official_helper: "property",
    support: "unsupported",
    arity: "property",
    event: "load",
    prelude: "none",
    should_proxy: false,
  },
  {
    name: "naturalHeight",
    direction: "r",
    value_type: "number",
    tags: "img",
    special: null,
    official_helper: "property",
    support: "unsupported",
    arity: "property",
    event: "load",
    prelude: "none",
    should_proxy: false,
  },
  {
    name: "videoWidth",
    direction: "r",
    value_type: "number",
    tags: "video",
    special: null,
    official_helper: "property",
    support: "unsupported",
    arity: "property",
    event: "resize",
    prelude: "none",
    should_proxy: false,
  },
  {
    name: "videoHeight",
    direction: "r",
    value_type: "number",
    tags: "video",
    special: null,
    official_helper: "property",
    support: "unsupported",
    arity: "property",
    event: "resize",
    prelude: "none",
    should_proxy: false,
  },

  // Readonly resize-observer bindings (DOM → local only; any element). Official emits the
  // DEDICATED `$.bind_resize_observer(el, '<name>', set)` family — NOT the generic property
  // form — which the native client runtime does not emit yet
  // (`support: "unsupported"` ⇒ `resolve_runtime_bind` returns `None`, the bind fails
  // closed); the real `$.bind_resize_observer` identity is preserved (a generic property
  // form would be the wrong helper). The IDE columns stay real (the projector type-checks
  // the bind); the implementation that supports these flips support + adds goldens.
  {
    name: "contentRect",
    direction: "r",
    value_type: "DOMRectReadOnly",
    tags: "*",
    special: null,
    official_helper: "resize_observer",
    support: "unsupported",
    arity: "set_only",
    event: "",
    prelude: "none",
    should_proxy: false,
  },
  {
    name: "contentBoxSize",
    direction: "r",
    value_type: "readonly ResizeObserverSize[]",
    tags: "*",
    special: null,
    official_helper: "resize_observer",
    support: "unsupported",
    arity: "set_only",
    event: "",
    prelude: "none",
    should_proxy: false,
  },
  {
    name: "borderBoxSize",
    direction: "r",
    value_type: "readonly ResizeObserverSize[]",
    tags: "*",
    special: null,
    official_helper: "resize_observer",
    support: "unsupported",
    arity: "set_only",
    event: "",
    prelude: "none",
    should_proxy: false,
  },
  {
    name: "devicePixelContentBoxSize",
    direction: "r",
    value_type: "readonly ResizeObserverSize[]",
    tags: "*",
    special: null,
    official_helper: "resize_observer",
    support: "unsupported",
    arity: "set_only",
    event: "",
    prelude: "none",
    should_proxy: false,
  },

  // ── Special-host bindings (`<svelte:window>` / `<svelte:document>`) ──
  // These bind names carry `valid_elements: ['svelte:window' | 'svelte:document']`
  // in svelte's `binding_properties`, so they resolve ONLY on their special host
  // (`host_scope: "window" | "document"`) and the wrong-host pairs fail closed at the
  // router. Their host expression is the global (`$.window` / `$.document`), NOT a DOM
  // var, and EVERY one carries the `should_proxy: true` host setter
  // (`$.set(local, $$value, true)`) — the window/document host setters that 5f-b wires.

  // <svelte:window> dimension reads — `$.bind_window_size('<name>', set)` (name FIRST,
  // NO host expr, setter-only). innerWidth/innerHeight/outerWidth/outerHeight.
  {
    name: "innerWidth",
    direction: "r",
    value_type: "number",
    tags: "*",
    host_scope: "window",
    special: null,
    official_helper: "window_size",
    support: "supported",
    arity: "set_only",
    event: "",
    prelude: "none",
    should_proxy: true,
  },
  {
    name: "innerHeight",
    direction: "r",
    value_type: "number",
    tags: "*",
    host_scope: "window",
    special: null,
    official_helper: "window_size",
    support: "supported",
    arity: "set_only",
    event: "",
    prelude: "none",
    should_proxy: true,
  },
  {
    name: "outerWidth",
    direction: "r",
    value_type: "number",
    tags: "*",
    host_scope: "window",
    special: null,
    official_helper: "window_size",
    support: "supported",
    arity: "set_only",
    event: "",
    prelude: "none",
    should_proxy: true,
  },
  {
    name: "outerHeight",
    direction: "r",
    value_type: "number",
    tags: "*",
    host_scope: "window",
    special: null,
    official_helper: "window_size",
    support: "supported",
    arity: "set_only",
    event: "",
    prelude: "none",
    should_proxy: true,
  },

  // <svelte:window> scroll positions — `$.bind_window_scroll('x'|'y', get, set)`. The
  // runtime axis name is REMAPPED ('x' for scrollX, 'y' for scrollY) and the helper is
  // READ-WRITE (get+set), unlike the set-only window-size binds. `bidirectional: true`.
  {
    name: "scrollX",
    direction: "rw",
    value_type: "number",
    tags: "*",
    host_scope: "window",
    special: null,
    official_helper: "window_scroll",
    support: "supported",
    arity: "get_set",
    event: "",
    prelude: "none",
    should_proxy: true,
  },
  {
    name: "scrollY",
    direction: "rw",
    value_type: "number",
    tags: "*",
    host_scope: "window",
    special: null,
    official_helper: "window_scroll",
    support: "supported",
    arity: "get_set",
    event: "",
    prelude: "none",
    should_proxy: true,
  },

  // <svelte:window bind:online> — `$.bind_online(set)` (setter-only, NO name, NO host).
  {
    name: "online",
    direction: "r",
    value_type: "boolean",
    tags: "*",
    host_scope: "window",
    special: null,
    official_helper: "online",
    support: "supported",
    arity: "set_only",
    event: "",
    prelude: "none",
    should_proxy: true,
  },

  // <svelte:window bind:devicePixelRatio> — the generic property form on the `resize`
  // event: `$.bind_property('devicePixelRatio', 'resize', $.window, set)` (read-only ⇒ no
  // getter).
  {
    name: "devicePixelRatio",
    direction: "r",
    value_type: "number",
    tags: "*",
    host_scope: "window",
    special: null,
    official_helper: "property",
    support: "supported",
    arity: "property",
    event: "resize",
    prelude: "none",
    should_proxy: true,
  },

  // <svelte:document bind:activeElement> — the DEDICATED `$.bind_active_element(set)`
  // (setter-only, NO name, NO host expr — NOT the generic `$.bind_property`).
  {
    name: "activeElement",
    direction: "r",
    value_type: "Element | null",
    tags: "*",
    host_scope: "document",
    special: null,
    official_helper: "active_element",
    support: "supported",
    arity: "set_only",
    event: "",
    prelude: "none",
    should_proxy: true,
  },

  // <svelte:document> property reads — `$.bind_property('<name>', '<event>', $.document,
  // set)` (read-only ⇒ no getter). Per-name event: fullscreenElement→fullscreenchange,
  // pointerLockElement→pointerlockchange, visibilityState→visibilitychange.
  {
    name: "fullscreenElement",
    direction: "r",
    value_type: "Element | null",
    tags: "*",
    host_scope: "document",
    special: null,
    official_helper: "property",
    support: "supported",
    arity: "property",
    event: "fullscreenchange",
    prelude: "none",
    should_proxy: true,
  },
  {
    name: "pointerLockElement",
    direction: "r",
    value_type: "Element | null",
    tags: "*",
    host_scope: "document",
    special: null,
    official_helper: "property",
    support: "supported",
    arity: "property",
    event: "pointerlockchange",
    prelude: "none",
    should_proxy: true,
  },
  {
    name: "visibilityState",
    direction: "r",
    value_type: "DocumentVisibilityState",
    tags: "*",
    host_scope: "document",
    special: null,
    official_helper: "property",
    support: "supported",
    arity: "property",
    event: "visibilitychange",
    prelude: "none",
    should_proxy: true,
  },
];

function rsStr(s) {
  return JSON.stringify(s);
}

function directionVariant(dir) {
  switch (dir) {
    case "rw":
      return "BindDirection::ReadWrite";
    case "r":
      return "BindDirection::Read";
    default:
      throw new Error(`unknown direction: ${dir}`);
  }
}

function specialVariant(special) {
  switch (special) {
    case "this":
      return "BindSpecial::This";
    case "group":
      return "BindSpecial::Group";
    case null:
      return "BindSpecial::None";
    default:
      throw new Error(`unknown special: ${special}`);
  }
}

function hostScopeVariant(scope) {
  switch (scope) {
    case undefined:
    case "element":
      return "BindHostScope::Element";
    case "window":
      return "BindHostScope::Window";
    case "document":
      return "BindHostScope::Document";
    case "universal":
      return "BindHostScope::Universal";
    default:
      throw new Error(`unknown host_scope: ${scope}`);
  }
}

/// Push the `target_policy` field for one row onto `lines`, in the rustfmt-canonical
/// form. The default (absent `policy`) is `LvalueOrFunctionPair` (single line); only
/// `bind:group` carries `IdentifierOrMemberOnly { official_code }`, whose struct-variant
/// initializer exceeds the 100-col width and so wraps to the multi-line form rustfmt
/// would itself produce (keeping the generated file fmt-clean + byte-stable).
function pushPolicyField(lines, policy) {
  switch (policy) {
    case undefined:
    case "lvalue_or_function_pair":
      lines.push("        target_policy: BindTargetPolicy::LvalueOrFunctionPair,");
      return;
    case "identifier_or_member_only":
      lines.push("        target_policy: BindTargetPolicy::IdentifierOrMemberOnly {");
      lines.push('            official_code: "bind_group_invalid_expression",');
      lines.push("        },");
      return;
    default:
      throw new Error(`unknown policy: ${policy}`);
  }
}

function officialHelperVariant(helper) {
  // The CONTRACT-ROW official helper vocabulary. The builtin form-control helpers
  // (value / select_value / checked) are NOT contract rows — they are emitted directly
  // as a RuntimeHelper in `resolve_runtime_bind`'s builtin arm — so they are absent here.
  const map = {
    group: "Group",
    current_time: "CurrentTime",
    paused: "Paused",
    played: "Played",
    element_size: "ElementSize",
    content_editable: "ContentEditable",
    property: "Property",
    this: "This",
    files: "Files",
    focused: "Focused",
    volume: "Volume",
    muted: "Muted",
    playback_rate: "PlaybackRate",
    buffered: "Buffered",
    seekable: "Seekable",
    seeking: "Seeking",
    ended: "Ended",
    ready_state: "ReadyState",
    resize_observer: "ResizeObserver",
    // Special-host helpers (5f-b): the dedicated `<svelte:window>` / `<svelte:document>`
    // bind helpers the native client runtime emits.
    window_size: "WindowSize",
    window_scroll: "WindowScroll",
    online: "Online",
    active_element: "ActiveElement",
  };
  const v = map[helper];
  if (!v) throw new Error(`unknown official_helper: ${helper}`);
  return `OfficialRuntimeHelper::${v}`;
}

function supportVariant(support) {
  switch (support) {
    case "supported":
      return "RuntimeSupport::Supported";
    case "unsupported":
      return "RuntimeSupport::Unsupported";
    default:
      throw new Error(`unknown support: ${support}`);
  }
}

function arityVariant(arity) {
  switch (arity) {
    case "get_set":
      return "HelperArity::GetSet";
    case "set_only":
      return "HelperArity::SetOnly";
    case "property":
      return "HelperArity::Property";
    default:
      throw new Error(`unknown arity: ${arity}`);
  }
}

function preludeVariant(prelude) {
  switch (prelude) {
    case "none":
      return "BindPrelude::None";
    case "remove_input_defaults":
      return "BindPrelude::RemoveInputDefaults";
    case "remove_textarea_child":
      return "BindPrelude::RemoveTextareaChild";
    default:
      throw new Error(`unknown prelude: ${prelude}`);
  }
}

function generate(root) {
  const lines = [];
  lines.push("// This file is auto-generated by scripts/generate-svelte-bind-contract.mjs");
  lines.push("// The CLOSED Svelte-5 element-binding vocabulary (F4). Do NOT hand-edit:");
  lines.push("// edit the registry in the generator and regenerate, or the freshness");
  lines.push("// gate (svelte_bind_contract_freshness.rs) fails.");
  lines.push("");
  // The `use` import is emitted in the rustfmt-canonical WRAPPED form (the flat
  // single-line form exceeds the 100-col width, so `cargo fmt --check` would demand
  // the wrap — keeping the generated file fmt-clean AND byte-stable under the
  // freshness gate).
  lines.push("use super::bind_contract::{");
  lines.push(
    "    BindContract, BindDirection, BindHostScope, BindPrelude, BindSpecial, BindTargetPolicy,",
  );
  lines.push("    HelperArity, OfficialRuntimeHelper, RuntimeSupport,");
  lines.push("};");
  lines.push("");
  lines.push("/// The complete CLOSED bind-contract table — the SHARED SOURCE OF TRUTH for");
  lines.push("/// the wide `bind:` family (IDE + runtime). Ordered as authored in the");
  lines.push("/// generator registry.");
  lines.push("pub(crate) const SVELTE_BIND_CONTRACTS: &[BindContract] = &[");
  for (const row of REGISTRY) {
    lines.push("    BindContract {");
    lines.push(`        name: ${rsStr(row.name)},`);
    lines.push(`        direction: ${directionVariant(row.direction)},`);
    lines.push(`        value_type: ${rsStr(row.value_type)},`);
    lines.push(`        tags: ${rsStr(row.tags)},`);
    lines.push(`        host_scope: ${hostScopeVariant(row.host_scope)},`);
    lines.push(`        special: ${specialVariant(row.special)},`);
    pushPolicyField(lines, row.policy);
    lines.push(`        official_helper: ${officialHelperVariant(row.official_helper)},`);
    lines.push(`        support: ${supportVariant(row.support)},`);
    lines.push(`        arity: ${arityVariant(row.arity)},`);
    lines.push(`        prop_event: ${rsStr(row.event)},`);
    lines.push(`        prelude: ${preludeVariant(row.prelude)},`);
    lines.push(`        should_proxy: ${row.should_proxy ? "true" : "false"},`);
    lines.push("    },");
  }
  lines.push("];");
  lines.push("");

  // The freshness gate redirects output to a temp file via this override so it
  // can byte-compare a regen without mutating the committed tree.
  const outPath =
    process.env.VERTER_BIND_CONTRACT_OUT ||
    path.join(root, "crates", "verter_compiler", "src", "svelte", "bind_contract_data.rs");
  fs.writeFileSync(outPath, lines.join("\n"));
  return outPath;
}

const root = path.resolve(__dirname, "..");
const out = generate(root);
console.log(`Generated ${path.relative(root, out)}`);
