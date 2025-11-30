import { GetVueComponent } from "../components";

/**
 * Resolves a component from a components map or native element registry.
 *
 * This function has two overloads:
 *
 * **Overload 1: String-based resolution (for components map and native elements)**
 * - If `is` is a key in the components map `C`, returns `C[is]`
 * - If `is` is a native HTML element name, returns a functional component with the element's props
 * - If `is` is neither, the call is rejected at compile-time (argument not assignable to `never`)
 *
 * **Overload 2: Direct component reference**
 * - If `is` is already a Vue component (detected via `GetVueComponent`), returns it as-is
 * - Useful for dynamic components where you have a direct component reference
 *
 * @example
 * ```tsx
 * // Component map resolution
 * const components = { MyButton: ButtonComponent };
 * const Btn = toComponentRender("MyButton", components);
 * <Btn onClick={() => {}} />
 *
 * // Native element resolution
 * const Div = toComponentRender("div", {});
 * <Div class="container" />
 *
 * // Direct component reference
 * const Comp = toComponentRender(SomeComponent, {});
 * <Comp />
 *
 * // Invalid keys are rejected at compile-time:
 * // toComponentRender("NonExistent", components); // Error: Argument not assignable to 'never'
 * ```
 *
 * @remarks
 * - Invalid component names cause a compile-time error rather than returning `never`,
 *   providing better type safety by preventing invalid calls entirely.
 */
export declare function toComponentRender<
  T extends string,
  C extends Record<string, any>
>(
  is: T extends keyof C | keyof import("vue").NativeElements ? T : never,
  components: C
): T extends keyof C
  ? C[T]
  : T extends keyof import("vue").NativeElements
  ? (props: import("vue").NativeElements[T]) => JSX.Element
  : never;

/**
 * @see {@link toComponentRender} for full documentation
 */
export declare function toComponentRender<T, C extends Record<string, any>>(
  is: GetVueComponent<T> extends never ? never : T,
  components: C
): T;
