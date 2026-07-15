import "svelte/internal/disclose-version";
import * as $ from "svelte/internal/client";

var root = $.from_html(`<button> </button>`);

export default function App($$anchor, $$props) {
  $.push($$props, true);
  let count = $.state(0);
  $.user_effect(() => console.log("breadth-effect", $.get(count)));
  var button = root();
  var text = $.child(button, true);
  $.reset(button);
  $.template_effect(() => $.set_text(text, $.get(count)));
  $.delegated("click", button, () => $.update(count));
  $.append($$anchor, button);
  $.pop();
}

$.delegate(["click"]);
