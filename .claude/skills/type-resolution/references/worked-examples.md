## Worked Examples

Normative fixtures. Any solver / dispatch change that breaks an expected behaviour below breaks the contract.

### Example A — basic conditional

```ts
type MyType<T> = T extends string ? StringType : NotStringType;
```

- **Navigate(`MyType`)**: decl identity, parameters `[T]`, body shape `Conditional(check=T, extends=string)`. Do NOT descend into `StringType` / `NotStringType`. Stop at the open conditional shell.
- **Expand(`MyType`)** (T unbound): return a `Conditional` graph with both branches materialized. Shape retains `T`, `string`, `StringType`, `NotStringType`. Origin layer carries `Instantiate(MyType, [T])` and structural links into each branch.
- **Expand(`MyType<string>`)**: conditional is closed. Return only `StringType`. Origin layer carries `Instantiate(MyType, [string])`, `SubstituteTypeParam(T -> string)`, `ConditionalSelect(check=string extends string, branch=True)`. `NotStringType` is NOT walked and does NOT appear in the result.

### Example B — intersection with mixed static and generic members

```ts
type Foo = { foo: number };
type StringType = { a: Foo };
type NotStringType = { a: Foo[] };
type OtherType<T> = { a: Foo, b: T } & (T extends string ? StringType : NotStringType);
```

- **Navigate(`OtherType`)**: decl identity, parameters `[T]`, intersection shell with two arms: object `{ a, b }` and conditional `(T extends string ? … : …)`. Members `a` and `b` visible; `b` marked as a generic-parameter reference. Conditional arm stays a symbolic shell.
- **Expand(`OtherType`)** (T unbound): object arm expands — `a: Foo` fully resolved, `b: T` preserves `T` as a parameter reference (origin links to the `[T]` parameter list). Conditional arm stays `Conditional` with both branches materialized. Intersection is NOT collapsed because the conditional remains open.
- **Expand(`OtherType<string>`)**: object arm expands — `a: Foo`, `b: string` with `SubstituteTypeParam(T -> string)` origin. Conditional arm closes to `StringType`, which expands to `{ a: Foo }`. Intersection collapses to `{ a: Foo, b: string } & { a: Foo }` and normalizes; origin records `Normalize` over both contributing members.

### Example C — `infer` inside a decidable conditional

```ts
type Foo = { a: number };
type OtherFoo = Foo extends { a: infer T } ? T : never;
```

- The conditional is closed: `Foo` is concrete, `{ a: infer T }` is the extends pattern, no free parameters surround the check.
- **Expand(`OtherFoo`)**: the relation engine matches `Foo` against `{ a: infer T }`, binds `T = number` via `InferBind`, selects the True branch, returns `number`. Origin: `ConditionalSelect(True)` + `InferBind(T = number)`.
- **Navigate(`OtherFoo`)** follows the same reduction because `Navigate` reduces closed conditionals. The navigate result is the same `number` node. `infer` does NOT force a stop.

### Example D — path-precise projection

```ts
type Foo = { foo: number };
type StringType = { a: Foo };
type NotStringType = { a: Foo };
type OtherType<T> = { a: Foo, b: T } & (T extends string ? StringType : NotStringType);
```

- **`OtherType['a']['foo']`** (T unbound): project path `['a', 'foo']` through the intersection.
  - Object arm contributes `a: Foo`; project `Foo['foo']` → `number`.
  - Conditional arm is open; distribute `['a', 'foo']` into both branches.
    - True: `StringType['a']['foo']` → `Foo['foo']` → `number`.
    - False: `NotStringType['a']['foo']` → `Foo['foo']` → `number`.
  - Intersect contributing arms. Final: `number`, with origin edges for each contributing path. The `b` field is NEVER touched. `NotStringType`'s non-`a` members are NEVER walked.
- **`OtherType<string>['a']['foo']`**: conditional is closed.
  - Object arm: `Foo['foo']` → `number`.
  - Conditional arm: `StringType['a']['foo']` → `number`. `NotStringType` is NEVER touched.
  - Intersection collapses to `number`.

This is path-precise projection, not whole-branch expansion. Sibling members and unrelated branches are not materialized.

### Example E — nested open conditionals

```ts
type Deep<T> =
  T extends string
    ? (T extends "ab" | "cd" ? { kind: "pair"; value: T } : { kind: "str"; value: T })
    : { kind: "other"; value: T };
```

- **Expand(`Deep`)** (T unbound): outer check is open. Keep outer `Conditional` shell. In the True branch, inner check is also open; keep inner `Conditional` shell. In the False branch, materialize `{ kind: "other"; value: T }` with `T` preserved.
- **Expand(`Deep<"ab">`)**: outer check `"ab" extends string` closed → True. Inner check `"ab" extends "ab" | "cd"` closed → True. Return `{ kind: "pair"; value: "ab" }`. Origin chain: `Instantiate(Deep, ["ab"])` → `SubstituteTypeParam(T -> "ab")` → `ConditionalSelect(outer=True)` → `ConditionalSelect(inner=True)`.
- **Expand(`Deep<"xx">`)**: outer closed → True. Inner `"xx" extends "ab" | "cd"` closed → False. Return `{ kind: "str"; value: "xx" }`. Origin: same outer chain, `ConditionalSelect(inner=False)`.

> Template-literal pattern matching (e.g. `T extends \`${infer _}${infer _}\``) is a future relation-engine extension and is **not** part of this contract's normative fixtures. The nested-conditional semantics above apply uniformly once template-literal infer support lands — adding it does not require a contract revision.

### Example F — contributors-only union / intersection combining

```ts
type A = { a: number; x: string };
type B = { a: string; y: boolean };
type C = { z: number };
type AB = A | B | C;
```

- **`AB['a']`** projection: `A` contributes `number`, `B` contributes `string`, `C` does NOT contribute (no `a`). For a union, every member must contribute to the path; `C`'s miss makes the union projection a miss as a whole. (Contrast: if `AB` were `A | B` only, the result would be `number | string`.)
- For intersection analogs (`A & B & C`): `A` contributes `number`, `B` contributes `string`, `C` does not contribute. Contributing arms' projection: `number & string` → `never` via intersection normalization. `C` is ignored for the path; the `never` arises from the contributors' intersection semantics, not from `C` being rewritten.

