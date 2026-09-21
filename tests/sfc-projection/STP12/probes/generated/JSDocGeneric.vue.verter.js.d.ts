import { defineComponent } from "vue"
type __Verter_RootElementAttrs<Tag extends string> = Tag extends keyof import("vue").IntrinsicElementAttributes ? import("vue").IntrinsicElementAttributes[Tag] : {}
declare module "vue" {
  interface IntrinsicElementAttributes {}
}
type __Verter_DataAttrs = { [Key in `data-${string}`]?: unknown }
type __OmitNew<T> = { [K in keyof T]: T[K] }

const __comp = defineComponent({
})

declare const JSDocGeneric: __OmitNew<typeof __comp> & {
  new(props?: import("vue").PublicProps & Omit<__Verter_RootElementAttrs<"div">, "class" | "style" | keyof import("vue").PublicProps> & __Verter_DataAttrs): {
    $props: import("vue").PublicProps & Omit<__Verter_RootElementAttrs<"div">, "class" | "style" | keyof import("vue").PublicProps> & __Verter_DataAttrs,
    $emit: (event: string, ...args: unknown[]) => void,
    $data: {},
    $attrs: import("vue").HTMLAttributes,
    $refs: {},
  }
}
export default JSDocGeneric
//# sourceMappingURL=data:application/json;base64,eyJ2ZXJzaW9uIjozLCJuYW1lcyI6W10sInNvdXJjZXMiOlsiL3NyYy9KU0RvY0dlbmVyaWMudnVlIl0sInNvdXJjZXNDb250ZW50IjpbIjxzY3JpcHQgc2V0dXA+XG4vKiogQHRlbXBsYXRlIFQgQHBhcmFtIHtUfSB2YWx1ZSBAcmV0dXJucyB7VH0gKi9cbmNvbnN0IGlkZW50aXR5ID0gKHZhbHVlKSA9PiB2YWx1ZVxuY29uc3QgbGFiZWwgPSBpZGVudGl0eSgnbGFiZWwnKVxuPC9zY3JpcHQ+XG48dGVtcGxhdGU+PGRpdj57eyBsYWJlbCB9fTwvZGl2PjwvdGVtcGxhdGU+XG4iXSwibWFwcGluZ3MiOiJBO0E7QTs7O0E7QTtBO0E7QTs7QTtBLDBDLGdIO0Esd0MsZ0g7QSxXLDJDO0E7QTtBO0E7QTtBIn0=
