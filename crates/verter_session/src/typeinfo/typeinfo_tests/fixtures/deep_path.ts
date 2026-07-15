// @ai-generated - Synthetic deep indexed-access typeinfo fixture.

export type TerminalPayload = {
  id: string;
  priority: 1 | 2 | 3;
};

export type HeavySibling00 = {
  ignored00: {
    label: string;
    values: Array<{ id: string; score: number; nested: Record<string, string[]> }>;
  };
};

export type HeavySibling01 = {
  ignored01: {
    label: string;
    values: Array<{ id: string; score: number; nested: Record<string, string[]> }>;
  };
};

export type HeavySibling02 = {
  ignored02: {
    label: string;
    values: Array<{ id: string; score: number; nested: Record<string, string[]> }>;
  };
};

export type HeavySibling03 = {
  ignored03: {
    label: string;
    values: Array<{ id: string; score: number; nested: Record<string, string[]> }>;
  };
};

export type HeavySibling04 = {
  ignored04: {
    label: string;
    values: Array<{ id: string; score: number; nested: Record<string, string[]> }>;
  };
};

export type HeavySibling05 = {
  ignored05: {
    label: string;
    values: Array<{ id: string; score: number; nested: Record<string, string[]> }>;
  };
};

export type HeavySibling06 = {
  ignored06: {
    label: string;
    values: Array<{ id: string; score: number; nested: Record<string, string[]> }>;
  };
};

export type HeavySibling07 = {
  ignored07: {
    label: string;
    values: Array<{ id: string; score: number; nested: Record<string, string[]> }>;
  };
};

export type HeavySibling08 = {
  ignored08: {
    label: string;
    values: Array<{ id: string; score: number; nested: Record<string, string[]> }>;
  };
};

export type HeavySibling09 = {
  ignored09: {
    label: string;
    values: Array<{ id: string; score: number; nested: Record<string, string[]> }>;
  };
};

export type HeavySibling10 = {
  ignored10: {
    label: string;
    values: Array<{ id: string; score: number; nested: Record<string, string[]> }>;
  };
};

export type HeavySibling11 = {
  ignored11: {
    label: string;
    values: Array<{ id: string; score: number; nested: Record<string, string[]> }>;
  };
};

export type HeavySibling12 = {
  ignored12: {
    label: string;
    values: Array<{ id: string; score: number; nested: Record<string, string[]> }>;
  };
};

export type HeavySibling13 = {
  ignored13: {
    label: string;
    values: Array<{ id: string; score: number; nested: Record<string, string[]> }>;
  };
};

export type HeavySibling14 = {
  ignored14: {
    label: string;
    values: Array<{ id: string; score: number; nested: Record<string, string[]> }>;
  };
};

export type HeavySibling15 = {
  ignored15: {
    label: string;
    values: Array<{ id: string; score: number; nested: Record<string, string[]> }>;
  };
};

export type Layer00<T> = { target: T; sibling00?: HeavySibling00 };
export type Layer01<T> = { level00: Layer00<T>; sibling01?: HeavySibling01 };
export type Layer02<T> = { level01: Layer01<T>; sibling02?: HeavySibling02 };
export type Layer03<T> = { level02: Layer02<T>; sibling03?: HeavySibling03 };
export type Layer04<T> = { level03: Layer03<T>; sibling04?: HeavySibling04 };
export type Layer05<T> = { level04: Layer04<T>; sibling05?: HeavySibling05 };
export type Layer06<T> = { level05: Layer05<T>; sibling06?: HeavySibling06 };
export type Layer07<T> = { level06: Layer06<T>; sibling07?: HeavySibling07 };
export type Layer08<T> = { level07: Layer07<T>; sibling08?: HeavySibling08 };
export type Layer09<T> = { level08: Layer08<T>; sibling09?: HeavySibling09 };
export type Layer10<T> = { level09: Layer09<T>; sibling10?: HeavySibling10 };
export type Layer11<T> = { level10: Layer10<T>; sibling11?: HeavySibling11 };
export type Layer12<T> = { level11: Layer11<T>; sibling12?: HeavySibling12 };
export type Layer13<T> = { level12: Layer12<T>; sibling13?: HeavySibling13 };
export type Layer14<T> = { level13: Layer13<T>; sibling14?: HeavySibling14 };
export type Layer15<T> = { level14: Layer14<T>; sibling15?: HeavySibling15 };
export type DeepRoot = Layer15<TerminalPayload>;
export type DeepProjectedTarget =
  DeepRoot["level14"]["level13"]["level12"]["level11"]["level10"]["level09"]["level08"]["level07"]["level06"]["level05"]["level04"]["level03"]["level02"]["level01"]["level00"]["target"];
