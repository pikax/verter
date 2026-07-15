import "svelte/internal/disclose-version";
import * as $ from "svelte/internal/client";

var root = $.from_html(`<div><button>child</button></div> <p> </p>`, 1);

export default function App($$anchor) {
  let count = $.state(0);
  var fragment = root();
  var div = $.first_child(fragment);
  var p = $.sibling(div, 2);
  var text = $.child(p, true);
  $.reset(p);
  $.template_effect(() => $.set_text(text, $.get(count)));
  $.event(
    "click",
    div,
    $.self(() => $.update(count)),
  );
  $.append($$anchor, fragment);
}
