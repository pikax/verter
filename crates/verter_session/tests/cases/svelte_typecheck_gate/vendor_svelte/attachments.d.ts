// Vendored minimal `svelte/attachments` (5.29) — the `Attachment<E>` type the
// `{@attach}` projection checker (`__verter_attach`) consumes. An attachment is
// a function taking the attached element; a mistyped element (an
// `Attachment<HTMLInputElement>` on a `<canvas>`) fails the relation.

export interface Attachment<E extends EventTarget = Element> {
  (element: E): void | (() => void);
}
