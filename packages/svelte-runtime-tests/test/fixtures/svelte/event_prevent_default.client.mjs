import "svelte/internal/disclose-version";
import * as $ from "svelte/internal/client";

var root = $.from_html(`<button>btn</button> <p> </p>`, 1);

export default function App($$anchor) {
  let hits = $.state(0);
  var fragment = root();
  var button = $.first_child(fragment);
  var p = $.sibling(button, 2);
  var text = $.child(p, true);
  $.reset(p);
  $.template_effect(() => $.set_text(text, $.get(hits)));
  $.event(
    "click",
    button,
    $.preventDefault(() => $.update(hits)),
  );
  $.append($$anchor, fragment);
}
