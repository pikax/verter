import "svelte/internal/disclose-version";
import * as $ from "svelte/internal/client";

var root = $.from_html(`<select><option>a</option><option>b</option></select> <p> </p>`, 1);

export default function App($$anchor) {
  let v = $.state("a");
  var fragment = root();
  var select = $.first_child(fragment);
  var p = $.sibling(select, 2);
  var text = $.child(p, true);
  $.reset(p);
  $.template_effect(() => $.set_text(text, $.get(v)));
  $.bind_select_value(
    select,
    () => $.get(v),
    ($$value) => $.set(v, $$value),
  );
  $.append($$anchor, fragment);
}
