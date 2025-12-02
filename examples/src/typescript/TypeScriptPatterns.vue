<script setup lang="ts">
// TypeScript-specific syntax patterns for parser testing

import { ref, reactive, type Ref } from "vue";

// Type annotations
const count: number = 0;
const name: string = "John";
const active: boolean = true;
const items: string[] = [];
const tuple: [string, number] = ["hello", 42];
const record: Record<string, number> = { a: 1, b: 2 };
const map: Map<string, number> = new Map();
const set: Set<number> = new Set();

// Union types
const status: "pending" | "active" | "completed" = "active";
const value: string | number = "hello";
const nullable: string | null = null;
const optional: string | undefined = undefined;

// Intersection types
interface A { a: string }
interface B { b: number }
type AB = A & B;
const ab: AB = { a: "hello", b: 42 };

// Type assertions
const input = document.querySelector("input") as HTMLInputElement;
const element = <HTMLDivElement>document.createElement("div");

// Non-null assertion
const definitelyExists = document.querySelector(".exists")!;

// Satisfies operator (TS 4.9+)
const config = {
  theme: "dark",
  fontSize: 14,
} satisfies Record<string, string | number>;

// Const assertions
const directions = ["up", "down", "left", "right"] as const;
type Direction = typeof directions[number];

// Template literal types (in type position)
type EventName = `on${Capitalize<"click" | "focus" | "blur">}`;

// Generics
function identity<T>(value: T): T {
  return value;
}

function merge<T extends object, U extends object>(a: T, b: U): T & U {
  return { ...a, ...b };
}

// Generic arrow function
const wrap = <T,>(value: T): { wrapped: T } => ({ wrapped: value });

// Generic class (for reference)
class Container<T> {
  constructor(public value: T) {}
  map<U>(fn: (val: T) => U): Container<U> {
    return new Container(fn(this.value));
  }
}

// Conditional types
type IsArray<T> = T extends any[] ? true : false;
type ArrayElement<T> = T extends (infer E)[] ? E : never;

// Mapped types
type Readonly<T> = { readonly [K in keyof T]: T[K] };
type Optional<T> = { [K in keyof T]?: T[K] };

// Utility types usage
type PartialUser = Partial<{ name: string; age: number }>;
type RequiredConfig = Required<{ theme?: string; lang?: string }>;
type PickedProps = Pick<{ a: 1; b: 2; c: 3 }, "a" | "b">;
type OmittedProps = Omit<{ a: 1; b: 2; c: 3 }, "c">;
type ExtractedUnion = Extract<"a" | "b" | "c", "a" | "d">;
type ExcludedUnion = Exclude<"a" | "b" | "c", "a">;
type NonNullableType = NonNullable<string | null | undefined>;
type ReturnTypeOf = ReturnType<() => string>;
type ParametersOf = Parameters<(a: string, b: number) => void>;

// Indexed access types
interface Person {
  name: string;
  age: number;
  address: {
    city: string;
    zip: string;
  };
}
type PersonName = Person["name"];
type PersonAddress = Person["address"];
type PersonKeys = keyof Person;

// typeof operator in type position
const user = { name: "John", age: 30 };
type UserType = typeof user;

// Decorators (when enabled)
// @Component
// class MyComponent {}

// Enums
enum Status {
  Pending,
  Active,
  Completed,
}

const enum Direction2 {
  Up = "UP",
  Down = "DOWN",
}

// Namespace (for type organization)
namespace Models {
  export interface User {
    id: number;
    name: string;
  }
  export type UserId = number;
}

// Type guards
function isString(value: unknown): value is string {
  return typeof value === "string";
}

function assertDefined<T>(value: T | null | undefined): asserts value is T {
  if (value == null) throw new Error("Value is not defined");
}

// Overloads
function process(value: string): string;
function process(value: number): number;
function process(value: string | number): string | number {
  return value;
}

// Abstract class (for reference)
abstract class BaseComponent {
  abstract render(): void;
  log(message: string): void {
    console.log(message);
  }
}

// Readonly tuple
const point: readonly [number, number] = [10, 20];

// Variadic tuple types
type Concat<T extends unknown[], U extends unknown[]> = [...T, ...U];

// Using in ref
const typedRef = ref<string | null>(null);
const reactiveState = reactive<{ count: number; items: string[] }>({
  count: 0,
  items: [],
});
</script>

<template>
  <div>
    <h2>TypeScript Patterns</h2>
    <p>Count: {{ count }}</p>
    <p>Name: {{ name }}</p>
    <p>Status: {{ status }}</p>
    <p>Enum: {{ Status.Active }}</p>
  </div>
</template>
