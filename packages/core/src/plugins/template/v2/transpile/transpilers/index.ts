import { reducePlugins } from "../utils.js";

import Comment from "./comment/index.js";
import Element from "./element/index.js";
import Interpolation from "./interpolation/index.js";
// import Root from "./root/index.js";
import Text from "./text/index.js";

export default reducePlugins([Element, Comment, Interpolation, /*Root,*/ Text]);
