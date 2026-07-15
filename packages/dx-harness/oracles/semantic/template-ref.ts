// Curated semantic oracle - template-ref unwrapping.
//
// The intended Vue semantics of a template ref
// (`const inputRef = useTemplateRef<HTMLInputElement>('input')`, or
// `ref<HTMLInputElement | null>(null)`): `.value` unwraps to the element OR `null`
// before mount - the `| null` is the part a wrong unwrapping most often drops.
//
// Each anchor's query target is the LAST identifier on its line.

interface TemplateRef<T> {
  value: T | null;
}

declare const inputRef: TemplateRef<HTMLInputElement>;

const el = inputRef.value; // @dx-anchor ref.value

export { el };
