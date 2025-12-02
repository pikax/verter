// Type definitions for import testing

export interface User {
  id: number;
  name: string;
  email: string;
  role: "admin" | "user" | "guest";
}

export interface Config {
  theme: "light" | "dark";
  language: string;
  features: string[];
}

// Type exported from types.ts (moved from MixedExport.vue for compatibility)
export interface SomeType {
  value: string;
}

export type UserId = number;

export type UserWithoutEmail = Omit<User, "email">;

export type PartialUser = Partial<User>;

export type RequiredUser = Required<User>;

// Generic types
export interface ApiResponse<T> {
  data: T;
  status: number;
  message: string;
}

export type UserResponse = ApiResponse<User>;
export type UsersResponse = ApiResponse<User[]>;

// Utility types
export type Nullable<T> = T | null;
export type Optional<T> = T | undefined;
export type Maybe<T> = T | null | undefined;

// Function types
export type EventHandler<T = Event> = (event: T) => void;
export type AsyncHandler<T, R> = (arg: T) => Promise<R>;

// Mapped types
export type ReadonlyUser = Readonly<User>;
export type UserKeys = keyof User;
export type UserValues = User[keyof User];

// Conditional types
export type IsString<T> = T extends string ? true : false;
export type ExtractStrings<T> = T extends string ? T : never;

// Template literal types
export type EventName = `on${Capitalize<string>}`;
export type UserEventName = `user:${keyof User}`;
