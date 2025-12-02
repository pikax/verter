// Provide/Inject type definitions

import type { InjectionKey, Ref } from "vue";

// Simple typed injection key
export const ThemeKey: InjectionKey<string> = Symbol("theme");

// Complex object injection key
export interface UserContext {
  id: number;
  name: string;
  email: string;
  isAdmin: boolean;
}
export const UserKey: InjectionKey<UserContext> = Symbol("user");

// Reactive injection key
export const CounterKey: InjectionKey<Ref<number>> = Symbol("counter");

// Function injection key
export type NotifyFn = (message: string, type: "info" | "error" | "success") => void;
export const NotifyKey: InjectionKey<NotifyFn> = Symbol("notify");

// Object with methods injection key
export interface ApiService {
  get<T>(url: string): Promise<T>;
  post<T>(url: string, data: unknown): Promise<T>;
  put<T>(url: string, data: unknown): Promise<T>;
  delete(url: string): Promise<void>;
}
export const ApiKey: InjectionKey<ApiService> = Symbol("api");

// Optional injection with default
export interface ConfigContext {
  apiUrl: string;
  debug: boolean;
  version: string;
}
export const ConfigKey: InjectionKey<ConfigContext> = Symbol("config");
export const defaultConfig: ConfigContext = {
  apiUrl: "/api",
  debug: false,
  version: "1.0.0",
};
