export type Fn = (value: string) => void;

export interface MemberValueProps {
  onClick: () => void;
  handlers: Fn | Fn[];
  config: { nested: number };
}
