import type { FunctionalComponent, PropType } from "vue";

// Functional component with props type
interface ButtonProps {
  label: string;
  disabled?: boolean;
  variant?: "primary" | "secondary" | "danger";
}

// Using FunctionalComponent type
export const FunctionalButton: FunctionalComponent<ButtonProps> = (props, { emit, slots }) => {
  return (
    <button
      disabled={props.disabled}
      class={`btn btn-${props.variant || "primary"}`}
      onClick={() => emit("click")}
    >
      {props.label}
      {slots.default?.()}
    </button>
  );
};
FunctionalButton.props = {
  label: { type: String, required: true },
  disabled: Boolean,
  variant: String as PropType<"primary" | "secondary" | "danger">,
};
FunctionalButton.emits = ["click"];

// Functional component with slots typing
interface CardProps {
  title: string;
}

interface CardSlots {
  default?: () => any;
  header?: (props: { title: string }) => any;
  footer?: () => any;
}

export const FunctionalCard: FunctionalComponent<CardProps, {}, CardSlots> = (
  props,
  { slots }
) => {
  return (
    <div class="card">
      <header>{slots.header?.({ title: props.title }) ?? props.title}</header>
      <main>{slots.default?.()}</main>
      <footer>{slots.footer?.()}</footer>
    </div>
  );
};
FunctionalCard.props = {
  title: { type: String, required: true },
};

// Generic functional component
interface ListProps<T> {
  items: T[];
  keyFn: (item: T) => string | number;
}

export function createListComponent<T>(): FunctionalComponent<ListProps<T>> {
  return (props, { slots }) => {
    return (
      <ul>
        {props.items.map((item) => (
          <li key={props.keyFn(item)}>{slots.default?.({ item })}</li>
        ))}
      </ul>
    );
  };
}

// Stateless functional component - just a render function
export const Divider: FunctionalComponent = () => <hr class="divider" />;

// With inheritAttrs
export const TransparentWrapper: FunctionalComponent = (_, { attrs, slots }) => {
  return <div {...attrs}>{slots.default?.()}</div>;
};
TransparentWrapper.inheritAttrs = false;
