export interface CardInstance {
  readonly $props: { kind: "card"; label: string };
  readonly label: string;
}

export interface IconInstance {
  readonly $props: { kind: "icon"; size: number };
  readonly size: number;
}

export declare const Local: new (props: CardInstance["$props"]) => CardInstance;
export declare const Icon: new (props: IconInstance["$props"]) => IconInstance;
