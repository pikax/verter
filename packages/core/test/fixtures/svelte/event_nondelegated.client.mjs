import "svelte/internal/disclose-version";
import * as $ from "svelte/internal/client";

var root = $.from_html(`<input/> <p> </p>`, 1);

export default function App($$anchor) {
  let focused = $.state(false);
  var fragment = root();
  var input = $.first_child(fragment);
  var p = $.sibling(input, 2);
  var text = $.child(p, true);
  $.reset(p);
  $.template_effect(() => $.set_text(text, $.get(focused)));
  $.event("focus", input, () => $.set(focused, true));
  $.append($$anchor, fragment);
}
