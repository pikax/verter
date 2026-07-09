// The Vue macro globals (ambient), so `defineProps<T>(): T` is typed — the
// deliberate TS2322 in Comp.vue depends on `props.label` being `string`.
declare function defineProps<T>(): T;
declare function defineEmits<T>(): T;
declare function defineExpose(exposed?: unknown): void;
declare function defineOptions(options?: unknown): void;
declare function defineSlots<T>(): T;
declare function defineModel<T>(...args: unknown[]): { value: T };
declare function withDefaults<P, D>(props: P, defaults: D): P;
