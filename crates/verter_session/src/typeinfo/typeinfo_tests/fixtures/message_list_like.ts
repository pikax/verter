// @ai-generated - Synthetic message-list-like typeinfo fixture.

export type VNode = unknown;
export interface DataTypes {
  text?: string;
  image?: { url: string; alt?: string };
}
export interface ToolMap {
  search?: { query: string };
  code?: { language: string; source: string };
}
export interface Message<M = unknown, D = DataTypes, U = ToolMap> {
  id: string;
  role: "user" | "assistant" | "system";
  metadata: M;
  data: D;
  tools: U;
  parts?: Array<{ type: "text"; text: string } | { type: "tool"; tool: keyof U }>;
}
export interface MessageUi {
  root?: string;
  content?: string;
  avatar?: string;
  actions?: string;
}
export interface AvatarConfig {
  src?: string;
  alt?: string;
  icon?: string;
}
export interface ActionItem {
  label: string;
  icon?: string;
  onSelect?: (message: Message) => void;
}
export interface MessageProps<M, D, U> {
  message: Message<M, D, U>;
  icon?: string;
  avatar?: AvatarConfig;
  variant?: "solid" | "soft" | "naked";
  side?: "left" | "right";
  actions?: ActionItem[];
  ui?: MessageUi;
}
export type MessageBase<T extends Message[]> =
  T[number] extends Message<infer M, infer D, infer U>
    ? Message<M, D, U>
    : Message<unknown, DataTypes, ToolMap>;
export type PropsBase<T extends Message[]> =
  MessageBase<T> extends Message<infer M, infer D, infer U> ? MessageProps<M, D, U> : never;
export type MessageSlots = {
  content?: (props: { compact: boolean }) => VNode[];
  avatar?: (props: { size: "sm" | "md" }) => VNode[];
  actions?: (props: { items: ActionItem[] }) => VNode[];
};
export interface MessageListProps<T extends Message[] = Message[]> {
  messages?: T;
  user?: Pick<PropsBase<T>, "icon" | "avatar" | "variant" | "side" | "actions" | "ui">;
  assistant?: Pick<PropsBase<T>, "icon" | "avatar" | "variant" | "side" | "actions" | "ui">;
  compact?: boolean;
}
export type MessageListSlots<T extends Message[] = Message[]> = {
  default?(props?: {}): VNode[];
  viewport?(props: { onClick: () => void }): VNode[];
} & {
  [K in keyof MessageSlots]?: NonNullable<MessageSlots[K]> extends (props: infer P) => VNode[]
    ? (props: P & { message: MessageBase<T> }) => VNode[]
    : never;
};
export type ConcreteMessage = Message<
  { traceId: string },
  { text: string },
  { search: { query: string } }
>;
export type ConcreteMessageDirectUserProps = Pick<
  MessageProps<{ traceId: string }, { text: string }, { search: { query: string } }>,
  "icon" | "avatar" | "variant" | "side" | "actions" | "ui"
>;
export type ConcreteMessageDirectContentPayload = { compact: boolean } & {
  message: ConcreteMessage;
};
export type ConcreteMessageListUserProps = NonNullable<MessageListProps<ConcreteMessage[]>["user"]>;
export type ConcreteMessageContentSlotPayload = Parameters<
  NonNullable<MessageListSlots<ConcreteMessage[]>["content"]>
>[0];
