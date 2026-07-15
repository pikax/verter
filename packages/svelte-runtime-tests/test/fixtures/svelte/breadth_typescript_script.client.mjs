import "svelte/internal/disclose-version";
import * as $ from "svelte/internal/client";

var root = $.from_html(`<button> </button>`);

export default function App($$anchor, $$props) {
  $.push($$props, true);

  let count = $.state(0);
  const STEP = 2;
  function advance(delta = STEP) {
    $.set(count, $.get(count) + delta);
  }
  class Marker {
    value = 3;
  }
  if (new Marker().value < 0) advance(0);
  var button = root();
  var text = $.child(button, true);
  $.reset(button);
  $.template_effect(() => $.set_text(text, $.get(count)));
  $.delegated("click", button, () => $.set(count, $.get(count) + STEP));
  $.append($$anchor, button);
  $.pop();
}

$.delegate(["click"]);
