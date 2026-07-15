import "svelte/internal/disclose-version";
import * as $ from "svelte/internal/client";

var root = $.from_html(`<h1> </h1> <input/> <button> </button>`, 1);

export default function App($$anchor) {
  let name = $.state("world");
  let count = $.state(0);
  var fragment = root();
  var h1 = $.first_child(fragment);
  var text = $.child(h1);
  $.reset(h1);
  var input = $.sibling(h1, 2);
  $.remove_input_defaults(input);
  var button = $.sibling(input, 2);
  var text_1 = $.child(button);
  $.reset(button);
  $.template_effect(() => {
    $.set_text(text, `Hello ${$.get(name) ?? ""}!`);
    $.set_text(text_1, `clicks: ${$.get(count) ?? ""}`);
  });
  $.bind_value(
    input,
    () => $.get(name),
    ($$value) => $.set(name, $$value),
  );
  $.delegated("click", button, () => $.set(count, $.get(count) + 1));
  $.append($$anchor, fragment);
}

$.delegate(["click"]);
