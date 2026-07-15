import "svelte/internal/disclose-version";
import * as $ from "svelte/internal/client";

var root = $.from_html(`<input/> <button>toggle</button>`, 1);

export default function App($$anchor) {
  let off = $.state(false);
  var fragment = root();
  var input = $.first_child(fragment);
  var button = $.sibling(input, 2);
  $.template_effect(() => (input.readOnly = $.get(off)));
  $.delegated("click", button, () => $.set(off, !$.get(off)));
  $.append($$anchor, fragment);
}

$.delegate(["click"]);
