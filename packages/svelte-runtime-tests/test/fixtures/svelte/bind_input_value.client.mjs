import "svelte/internal/disclose-version";
import * as $ from "svelte/internal/client";

var root = $.from_html(`<input/> <p> </p> <button>clear</button>`, 1);

export default function App($$anchor) {
  let v = $.state("init");
  var fragment = root();
  var input = $.first_child(fragment);
  $.remove_input_defaults(input);
  var p = $.sibling(input, 2);
  var text = $.child(p, true);
  $.reset(p);
  var button = $.sibling(p, 2);
  $.template_effect(() => $.set_text(text, $.get(v)));
  $.bind_value(
    input,
    () => $.get(v),
    ($$value) => $.set(v, $$value),
  );
  $.delegated("click", button, () => $.set(v, ""));
  $.append($$anchor, fragment);
}

$.delegate(["click"]);
