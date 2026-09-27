export type EmitterProps = {
  readonly onSaveItem?: (value: number) => void;
  readonly "onUpdate:modelValue"?: (value: string) => void;
  readonly onSaveOnceCapturePassive?: (value: number) => void;
  readonly onCancel?: (value: boolean) => void;
};

export declare class Emitter {
  constructor(props?: EmitterProps);
  readonly $props: EmitterProps;
  readonly count: number;
}

export default Emitter;
