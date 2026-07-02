import "svelte/internal/disclose-version";
import * as $ from "svelte/internal/client";

var root = $.from_html(`<div><button>btn</button></div> <p> </p>`, 1);

export default function App($$anchor) {
  let inner = $.state(0);
  let outer = $.state(0);
  var fragment = root();
  var div = $.first_child(fragment);
  var button = $.child(div);
  $.reset(div);
  var p = $.sibling(div, 2);
  var text = $.child(p);
  $.reset(p);
  $.template_effect(() => $.set_text(text, `${$.get(inner) ?? ""}-${$.get(outer) ?? ""}`));
  $.event(
    "click",
    button,
    $.stopPropagation(() => $.update(inner)),
  );
  $.event("click", div, () => $.update(outer));
  $.append($$anchor, fragment);
}
