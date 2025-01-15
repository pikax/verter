import { DefineProps as $V_DefineProps } from "vue";


/* __VERTER_IMPORTS__
[
  {
    "from": "vue",
    "asType": true,
    "items": [
      { "name": "DefineProps", "alias": "$V_DefineProps" }
    ]
  }
]
/__VERTER_IMPORTS__ */

// __VERTER__START__

export type $V_PartialUndefined<T> = {
  [P in keyof T]: undefined extends T[P] ? P : never;
}[keyof T] extends infer U extends keyof T
  ? Omit<T, U> & Partial<Pick<T, U>>
  : T;

export type $V_ProcessProps<T> = T extends $V_DefineProps<infer U, infer BKeys>
  ? $V_PartialUndefined<U>
  : T;
