// @ai-generated - Synthetic enum typeinfo fixture.

export enum Color {
  Red,
  Green,
  Blue,
}

export enum Status {
  Idle = "idle",
  Active = "active",
  Done = "done",
}

export const enum Direction {
  Up = "UP",
  Down = "DOWN",
}

// Type aliases under test
export type ColorRed = Color.Red;
export type StatusIdle = Status.Idle;
export type StatusValueUnion = `${Status}`;
export type ColorKeyUnion = keyof typeof Color;
export type StatusKeyUnion = keyof typeof Status;
export type DirectionUp = Direction.Up;

// Enum used as discriminant
export type StatefulNode =
  | { status: Status.Idle; payload: { hint: string } }
  | { status: Status.Active; payload: { tick: number } }
  | { status: Status.Done; payload: { result: boolean } };

export type IdleNodePayload = Extract<StatefulNode, { status: Status.Idle }>["payload"];
