declare function defineProps<T>(): T;
declare function defineEmits<T>(): T;
declare function defineExpose(exposed?: unknown): void;
declare function defineOptions(options?: unknown): void;
declare function defineSlots<T>(): T;
declare function defineModel<T>(...args: unknown[]): { value: T };
declare function withDefaults<P, D>(props: P, defaults: D): P;
