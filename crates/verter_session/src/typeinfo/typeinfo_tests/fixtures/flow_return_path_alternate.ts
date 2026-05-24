// @ai-generated - Synthetic alternate branch for flow-return path coverage.

export type AlternateEnvelope = {
  stats: {
    count: number;
    nested: {
      code: number;
    };
  };
  flags: {
    ready: false;
  };
};

export type AlternateDeep = {
  alternate: AlternateEnvelope;
  local: {
    visible: boolean;
  };
};

export function makeAlternate(count = 1): AlternateEnvelope {
  return {
    stats: {
      count,
      nested: {
        code: count,
      },
    },
    flags: {
      ready: false,
    },
  };
}
