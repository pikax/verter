export const VUE_MACRO_RUNTIME_FIXTURES = Object.freeze([
  {
    id: "primitive-and-bigint-props",
    axes: ["props", "primitive", "literal", "bigint"],
    source: `<script setup lang="ts">
defineProps<{
  text: string
  count: number
  enabled: boolean
  token: symbol
  nothing: null
  literal: 'fixed'
  huge: bigint
}>()
</script>`,
  },
  {
    id: "ordered-unions-and-skip-check",
    axes: ["props", "union-order", "dedup", "unknown", "skip-check"],
    source: `<script setup lang="ts">
defineProps<{
  ordered: string | boolean | string
  booleanUnknown: unknown | boolean
  functionUnknown: unknown | (() => void)
  numberUnknown: unknown | number
}>()
</script>`,
  },
  {
    id: "containers-callables-and-nominals",
    axes: ["props", "array", "tuple", "callable", "nominal", "structural-object"],
    source: `<script setup lang="ts">
class UserClass {}
interface UserObject { nested: { value: string } }
defineProps<{
  array: string[]
  tuple: [string, number]
  callable: () => void
  date: Date
  map: Map<string, string>
  set: Set<string>
  promise: Promise<string>
  error: Error
  userClass: UserClass
  userObject: UserObject
}>()
</script>`,
  },
  {
    id: "with-defaults",
    axes: ["props", "with-defaults", "optional"],
    source: `<script setup lang="ts">
withDefaults(defineProps<{ label?: string; count?: number }>(), {
  label: 'fallback',
})
</script>`,
  },
  {
    id: "emits-call-signature",
    axes: ["emits", "call-signature"],
    source: `<script setup lang="ts">
defineEmits<{
  (event: 'change', id: number): void
  (event: 'close'): void
}>()
</script>`,
  },
  {
    id: "emits-property-syntax",
    axes: ["emits", "property-tuple"],
    source: `<script setup lang="ts">
defineEmits<{
  save: [value: string]
  reset: []
}>()
</script>`,
  },
  {
    id: "define-model-default-and-named",
    axes: ["model", "prop", "update-event", "modifiers"],
    source: `<script setup lang="ts">
defineModel<string>()
defineModel<number>('count')
</script>`,
  },
  {
    id: "vue-ignore",
    axes: ["props", "vue-ignore", "heritage"],
    source: `<script setup lang="ts">
interface IgnoredBase { ignored: string }
interface Props extends /* @vue-ignore */ IgnoredBase { own: number }
defineProps<Props>()
</script>`,
  },
  {
    id: "imported-utility-and-indexed",
    axes: ["props", "import", "generic", "utility", "indexed-access", "structural-object"],
    filename: "/fixtures/imported.vue",
    source: `<script setup lang="ts">
import type { Props } from './types'
defineProps<Props>()
</script>`,
    supportFiles: {
      "/fixtures/types.ts": `
export interface Box<T> { deep: T }
export interface Deep { nested: { value: string } }
export interface Base { count: number; options: Box<Deep> }
export type Props = Pick<Base, 'count' | 'options'> & {
  selected: Base['options']
}
`,
    },
  },
]);
