// JSX/TSX patterns for parser testing

import { ref, defineComponent, h, type VNode, Transition } from "vue";

// Render function returning JSX
export function renderButton(label: string, onClick: () => void): VNode {
  return <button onClick={onClick}>{label}</button>;
}

// JSX with expressions
export function renderList(items: string[]): VNode {
  return (
    <ul>
      {items.map((item, index) => (
        <li key={index}>{item}</li>
      ))}
    </ul>
  );
}

// Conditional JSX
export function renderConditional(show: boolean): VNode | null {
  return show ? <div>Visible</div> : null;
}

// JSX with spread
interface ButtonProps {
  label: string;
  disabled?: boolean;
  onClick?: () => void;
}

export function renderWithSpread(props: ButtonProps): VNode {
  return <button {...props}>{props.label}</button>;
}

// Fragment
export function renderFragment(): VNode {
  return (
    <>
      <span>First</span>
      <span>Second</span>
    </>
  );
}

// Component using render with JSX
export const JsxComponent = defineComponent({
  props: {
    title: { type: String, required: true },
  },
  setup(props) {
    return () => (
      <div class="jsx-component">
        <h3>{props.title}</h3>
        <p>JSX in setup render</p>
      </div>
    );
  },
});

// Using h() alongside JSX
export function renderWithH(): VNode {
  return h("div", { class: "h-rendered" }, [
    h("span", "Using h()"),
    <span>Using JSX</span>,
  ]);
}

// Slots in JSX
export const SlottedComponent = defineComponent({
  setup(_, { slots }) {
    return () => (
      <div class="slotted">
        <header>{slots.header?.()}</header>
        <main>{slots.default?.()}</main>
        <footer>{slots.footer?.()}</footer>
      </div>
    );
  },
});

// Using scoped slots in JSX
export function renderWithScopedSlot(): VNode {
  return (
    <SlottedComponent>
      {{
        header: () => <h1>Header Slot</h1>,
        default: () => <p>Default Slot</p>,
        footer: () => <small>Footer Slot</small>,
      }}
    </SlottedComponent>
  );
}

// v-model in JSX (manual implementation)
export function renderInput(): VNode {
  const value = ref("");
  return (
    <input
      value={value.value}
      onInput={(e: Event) => {
        value.value = (e.target as HTMLInputElement).value;
      }}
    />
  );
}

// Event handling in JSX
export function renderWithEvents(): VNode {
  const handleClick = (e: MouseEvent) => {
    e.stopPropagation();
    e.preventDefault();
    console.log("Clicked");
  };
  
  const handleInput = (e: Event) => {
    console.log((e.target as HTMLInputElement).value);
  };
  
  return (
    <div>
      <button onClick={handleClick}>Click</button>
      <input onInput={handleInput} />
    </div>
  );
}

// Dynamic component in JSX
export function renderDynamic(component: any): VNode {
  const Comp = component;
  return <Comp prop="value" />;
}

// Transition in JSX
export function renderTransition(show: boolean): VNode {
  return (
    <Transition name="fade">
      {show && <div>Transitioning</div>}
    </Transition>
  );
}

// Class and style bindings in JSX
export function renderWithBindings(): VNode {
  const isActive = true;
  const color = "red";
  
  return (
    <div
      class={["base", { active: isActive, disabled: false }]}
      style={{ color, fontSize: "14px" }}
    >
      Styled content
    </div>
  );
}

// Refs in JSX
export const RefComponent = defineComponent({
  setup() {
    const inputRef = ref<HTMLInputElement | null>(null);
    
    return () => (
      <div>
        <input ref={inputRef} type="text" />
        <button onClick={() => inputRef.value?.focus()}>Focus</button>
      </div>
    );
  },
});

// v-show equivalent in JSX
export function renderVShow(visible: boolean): VNode {
  return (
    <div style={{ display: visible ? undefined : "none" }}>
      Conditionally visible
    </div>
  );
}

// Key prop in JSX
export function renderWithKey(items: Array<{ id: number; name: string }>): VNode {
  return (
    <ul>
      {items.map((item) => (
        <li key={item.id}>{item.name}</li>
      ))}
    </ul>
  );
}

// Directive equivalent (v-model) in JSX component
export const VModelComponent = defineComponent({
  props: {
    modelValue: { type: String, required: true },
  },
  emits: ["update:modelValue"],
  setup(props, { emit }) {
    return () => (
      <input
        value={props.modelValue}
        onInput={(e: Event) => {
          emit("update:modelValue", (e.target as HTMLInputElement).value);
        }}
      />
    );
  },
});
