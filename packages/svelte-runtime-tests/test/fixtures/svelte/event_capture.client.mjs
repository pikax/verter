import "svelte/internal/disclose-version";
import * as $ from "svelte/internal/client";

var root = $.from_html(`<div><button>btn</button></div> <p> </p>`, 1);

export default function App($$anchor) {
  let log = $.state("");
  var fragment = root();
  var div = $.first_child(fragment);
  var p = $.sibling(div, 2);
  var text = $.child(p, true);
  $.reset(p);
  $.template_effect(() => $.set_text(text, $.get(log)));
  $.event("click", div, () => $.set(log, $.get(log) + "B"));
  $.event("click", div, () => $.set(log, $.get(log) + "C"), true);
  $.append($$anchor, fragment);
}
