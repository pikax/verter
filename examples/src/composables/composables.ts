// Composables - reusable stateful logic

import { ref, onMounted, onUnmounted, type Ref } from "vue";

/**
 * Composable for tracking mouse position
 */
export function useMouse() {
  const x = ref(0);
  const y = ref(0);

  function update(event: MouseEvent) {
    x.value = event.pageX;
    y.value = event.pageY;
  }

  onMounted(() => window.addEventListener("mousemove", update));
  onUnmounted(() => window.removeEventListener("mousemove", update));

  return { x, y };
}

/**
 * Composable for counter logic with typed options
 */
export interface UseCounterOptions {
  initialValue?: number;
  min?: number;
  max?: number;
}

export function useCounter(options: UseCounterOptions = {}) {
  const { initialValue = 0, min = -Infinity, max = Infinity } = options;

  const count = ref(initialValue);

  function increment() {
    if (count.value < max) {
      count.value++;
    }
  }

  function decrement() {
    if (count.value > min) {
      count.value--;
    }
  }

  function reset() {
    count.value = initialValue;
  }

  function set(value: number) {
    count.value = Math.min(Math.max(value, min), max);
  }

  return {
    count,
    increment,
    decrement,
    reset,
    set,
  };
}

/**
 * Composable for toggling boolean state
 */
export function useToggle(initialValue = false) {
  const state = ref(initialValue);

  function toggle() {
    state.value = !state.value;
  }

  function setTrue() {
    state.value = true;
  }

  function setFalse() {
    state.value = false;
  }

  return {
    state,
    toggle,
    setTrue,
    setFalse,
  };
}

/**
 * Composable for async data fetching with loading/error states
 */
export interface UseFetchOptions<T> {
  immediate?: boolean;
  initialData?: T;
}

export interface UseFetchReturn<T> {
  data: Ref<T | null>;
  error: Ref<Error | null>;
  isLoading: Ref<boolean>;
  execute: () => Promise<void>;
}

export function useFetch<T>(
  url: string | Ref<string>,
  options: UseFetchOptions<T> = {}
): UseFetchReturn<T> {
  const { immediate = true, initialData = null } = options;

  const data = ref<T | null>(initialData) as Ref<T | null>;
  const error = ref<Error | null>(null);
  const isLoading = ref(false);

  async function execute() {
    isLoading.value = true;
    error.value = null;

    try {
      const urlValue = typeof url === "string" ? url : url.value;
      const response = await fetch(urlValue);

      if (!response.ok) {
        throw new Error(`HTTP error! status: ${response.status}`);
      }

      data.value = await response.json();
    } catch (e) {
      error.value = e instanceof Error ? e : new Error(String(e));
    } finally {
      isLoading.value = false;
    }
  }

  if (immediate) {
    execute();
  }

  return {
    data,
    error,
    isLoading,
    execute,
  };
}

/**
 * Composable for localStorage with reactive state
 */
export function useLocalStorage<T>(key: string, defaultValue: T) {
  const storedValue = localStorage.getItem(key);
  const data = ref<T>(
    storedValue !== null ? JSON.parse(storedValue) : defaultValue
  ) as Ref<T>;

  function set(value: T) {
    data.value = value;
    localStorage.setItem(key, JSON.stringify(value));
  }

  function remove() {
    data.value = defaultValue;
    localStorage.removeItem(key);
  }

  return {
    data,
    set,
    remove,
  };
}

/**
 * Composable for debounced ref
 */
export function useDebouncedRef<T>(value: T, delay = 200) {
  let timeout: ReturnType<typeof setTimeout>;
  const debouncedValue = ref(value) as Ref<T>;

  function update(newValue: T) {
    clearTimeout(timeout);
    timeout = setTimeout(() => {
      debouncedValue.value = newValue;
    }, delay);
  }

  return {
    value: debouncedValue,
    update,
  };
}
