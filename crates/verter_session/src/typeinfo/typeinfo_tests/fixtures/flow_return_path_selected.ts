// @ai-generated - Synthetic selected branch for flow-return path coverage.

export type SelectedEnvelope = {
  profile: {
    name: string;
    nested: {
      id: string;
    };
  };
  flags: {
    ready: true;
  };
};

export type SelectedDeep = {
  selected: SelectedEnvelope;
  local: {
    visible: boolean;
  };
};

export function makeSelected(name = "selected"): SelectedEnvelope {
  return {
    profile: {
      name,
      nested: {
        id: `${name}:id`,
      },
    },
    flags: {
      ready: true,
    },
  };
}

export function isSelectedReady(input: unknown): input is SelectedEnvelope {
  return typeof input === "object" && input !== null && "profile" in input && "flags" in input;
}

export function assertSelectedRecord(input: unknown): asserts input is SelectedEnvelope {
  if (!isSelectedReady(input)) throw new Error("not selected");
}

export function selectedMap<T, R>(value: T, map: (value: T) => R): R {
  return map(value);
}
