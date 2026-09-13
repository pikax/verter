import "svelte/internal/disclose-version";
import * as $ from "svelte/internal/client";

var root = $.from_html(`<input type="checkbox"/> <p> </p> <button>toggle</button>`, 1);

export default function App($$anchor) {
  let c = $.state(false);
  var fragment = root();
  var input = $.first_child(fragment);
  $.remove_input_defaults(input);
  var p = $.sibling(input, 2);
  var text = $.child(p, true);
  $.reset(p);
  var button = $.sibling(p, 2);
  $.template_effect(() => $.set_text(text, $.get(c)));
  $.bind_checked(
    input,
    () => $.get(c),
    ($$value) => $.set(c, $$value),
  );
  $.delegated("click", button, () => $.set(c, !$.get(c)));
  $.append($$anchor, fragment);
}

$.delegate(["click"]);
