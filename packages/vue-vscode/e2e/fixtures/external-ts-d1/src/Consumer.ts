// A PLAIN `.ts` file (not a carrier) importing the `.vue` component. Under the
// project-bound external-TS contract the bare `.vue` import resolves to the
// component IDE carrier and the component's public surface flows into this module —
// a hover on `Comp` here reaches the real prop type through the carrier source.
import Comp from "./Comp.vue";

export const comp = Comp;

export function useComp(): typeof Comp {
  return comp;
}
