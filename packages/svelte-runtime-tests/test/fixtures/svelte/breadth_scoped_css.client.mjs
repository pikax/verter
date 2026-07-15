import "svelte/internal/disclose-version";
import * as $ from "svelte/internal/client";

var root = $.from_html(`<button class="action svelte-n50uah"> </button>`);

export default function App($$anchor) {
  let count = $.state(0);
  var button = root();
  var text = $.child(button, true);
  $.reset(button);
  $.template_effect(() => $.set_text(text, $.get(count)));
  $.delegated("click", button, () => $.update(count));
  $.append($$anchor, button);
}

$.delegate(["click"]);
