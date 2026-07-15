import "svelte/internal/disclose-version";
import * as $ from "svelte/internal/client";

var root = $.from_html(`<details></details> <p> </p>`, 1);

export default function App($$anchor) {
  let o = $.state(false);
  var fragment = root();
  var details = $.first_child(fragment);
  var p = $.sibling(details, 2);
  var text = $.child(p, true);
  $.reset(p);
  $.template_effect(() => $.set_text(text, $.get(o)));
  $.bind_property(
    "open",
    "toggle",
    details,
    ($$value) => $.set(o, $$value),
    () => $.get(o),
  );
  $.append($$anchor, fragment);
}
