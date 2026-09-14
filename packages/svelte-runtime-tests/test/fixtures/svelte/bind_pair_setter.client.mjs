import "svelte/internal/disclose-version";
import * as $ from "svelte/internal/client";

var root = $.from_html(`<input/> <p> </p>`, 1);

export default function App($$anchor, $$props) {
  $.push($$props, true);
  let v = $.state("");
  var fragment = root();
  var input = $.first_child(fragment);
  $.remove_input_defaults(input);
  var p = $.sibling(input, 2);
  var text = $.child(p, true);
  $.reset(p);
  $.template_effect(() => $.set_text(text, $.get(v)));
  $.bind_value(
    input,
    () => $.get(v),
    (next) => {
      $$props.onSet(next);
      $.set(v, next, true);
    },
  );
  $.append($$anchor, fragment);
  $.pop();
}
