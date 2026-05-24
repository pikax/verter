// @ai-generated - Synthetic TS7 (TC39 stage 3) decorators typeinfo fixture.
//
// Covers identity-shaped class / method / field / accessor decorators, a
// decorator factory (a function returning a decorator), and a decorator
// that reads `ctx.metadata` (`Symbol.metadata`). All decorators below are
// identity in their effect — they return the original target — so the
// post-decoration class shape stays equal to the pre-decoration class
// shape. The tests assert that the type system preserves the published
// shape (including return-type inference for methods) across the
// decoration.

// 1. Class decorator: identity. Receives the constructor + ClassDecoratorContext,
// returns the constructor unchanged.
export function logged<T extends new (...args: any[]) => any>(
  ctor: T,
  _ctx: ClassDecoratorContext,
): T {
  return ctor;
}

@logged
export class LoggedItem {
  id: string = "";
  label(): string {
    return "label";
  }
}

export type LoggedItemInstance = InstanceType<typeof LoggedItem>;

// 2. Method decorator: identity. Receives the method + ClassMethodDecoratorContext.
export function bound<T extends (this: any, ...args: any[]) => any>(
  method: T,
  _ctx: ClassMethodDecoratorContext,
): T {
  return method;
}

export class MethodHost {
  @bound
  tag(): "tag" {
    return "tag";
  }
}

export type MethodHostInstance = InstanceType<typeof MethodHost>;
export type MethodHostTagReturn = ReturnType<MethodHost["tag"]>;

// 3. Field decorator with initializer. Receives `undefined` + ClassFieldDecoratorContext,
// returns an initializer function `(initial) => transformed`. Identity here:
// the returned function passes the initial value through unchanged.
export function tracked<This, Value>(
  _value: undefined,
  _ctx: ClassFieldDecoratorContext<This, Value>,
): (this: This, initial: Value) => Value {
  return function (this: This, initial: Value): Value {
    return initial;
  };
}

export class FieldHost {
  @tracked
  count: number = 0;
}

export type FieldHostInstance = InstanceType<typeof FieldHost>;

// 4. Accessor decorator: identity. The `accessor` keyword produces a synthesised
// pair of getter/setter; the decorator receives that pair via
// ClassAccessorDecoratorContext and returns it unchanged.
export function readonlyGet<This, Value>(
  target: ClassAccessorDecoratorTarget<This, Value>,
  _ctx: ClassAccessorDecoratorContext<This, Value>,
): ClassAccessorDecoratorTarget<This, Value> {
  return target;
}

export class AccessorHost {
  @readonlyGet
  accessor visible: string = "";
}

export type AccessorHostInstance = InstanceType<typeof AccessorHost>;

// 5. Decorator factory: a function `withTag` that captures its `tag` argument
// in a closure and returns a class decorator. The factory's parameter `tag`
// must stay the literal type "v1" passed at the call site (no widening).
export function withTag(tag: string) {
  return function <T extends new (...args: any[]) => any>(ctor: T, _ctx: ClassDecoratorContext): T {
    void tag;
    return ctor;
  };
}

@withTag("v1")
export class FactoryDecorated {
  payload: string = "";
}

export type FactoryDecoratedInstance = InstanceType<typeof FactoryDecorated>;

// 6. Decorator that reads `ctx.metadata` (`Symbol.metadata`).
// Reading the metadata bag inside the decorator must not change the class's
// public structural shape — the published instance type remains equal to
// what it would be without the decorator.
export function metadataReader<T extends new (...args: any[]) => any>(
  ctor: T,
  ctx: ClassDecoratorContext,
): T {
  // Touch ctx.metadata so the property is statically referenced. The decorator
  // does not mutate `ctor`, so the class's instance type is unchanged.
  void ctx.metadata;
  return ctor;
}

@metadataReader
export class MetadataAware {
  ready: boolean = false;
  describe(): "ready" | "pending" {
    return this.ready ? "ready" : "pending";
  }
}

export type MetadataAwareInstance = InstanceType<typeof MetadataAware>;
export type MetadataAwareDescribeReturn = ReturnType<MetadataAware["describe"]>;

// 7. Decorator factory with a `const`-modified type parameter capturing a
// literal at the call site. `withConstTag<const Tag extends string>(_tag)`
// infers `Tag = "v2"` from the call `@withConstTag("v2")`. The returned
// decorator is structurally identity (it ignores the captured `Tag` from
// the closure for type-shape purposes), so the decorated class's instance
// shape equals the bare class shape `{ visible: string }`.
export function withConstTag<const Tag extends string>(_tag: Tag) {
  return function <T extends new (...args: any[]) => any>(ctor: T, _ctx: ClassDecoratorContext): T {
    return ctor;
  };
}

@withConstTag("v2")
export class ConstTaggedClass {
  public visible: string = "";
}

export type ConstTaggedInstance = InstanceType<typeof ConstTaggedClass>;

// 8. Method decorator that invokes `ctx.addInitializer` (a `ClassMethodDecoratorContext`
// hook scheduled to run once per instance at construction). The hook is a
// runtime side-effect — it does NOT modify the method's type. The decorator
// otherwise returns the method unchanged. The published instance shape is
// therefore `{ ping(): "pong" }`.
export function methodWithInit<T extends (this: any, ...args: any[]) => any>(
  method: T,
  ctx: ClassMethodDecoratorContext,
): T {
  ctx.addInitializer(function () {
    // identity initializer
  });
  return method;
}

export class InitDecoratedClass {
  @methodWithInit
  ping() {
    return "pong" as const;
  }
}

export type InitDecoratedInstance = InstanceType<typeof InitDecoratedClass>;

// 9. Accessor decorator that returns a `ClassAccessorDecoratorTarget<unknown, T>`
// (the same shape it received). `accessor count: number` synthesises a
// getter/setter pair whose public surface is the property `count: number`.
// The decorator is type-level identity, so the published instance type
// remains `{ count: number }`.
export function trackedAccessor<T>(
  target: ClassAccessorDecoratorTarget<unknown, T>,
  _ctx: ClassAccessorDecoratorContext<unknown, T>,
): ClassAccessorDecoratorTarget<unknown, T> {
  return target;
}

export class AccessorTransformedClass {
  @trackedAccessor accessor count: number = 0;
}

export type AccessorTransformedInstance = InstanceType<typeof AccessorTransformedClass>;
