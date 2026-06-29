import "svelte/internal/disclose-version";
import * as $ from "svelte/internal/client";

var root = $.from_html(`<p> </p>`);
var root_1 = $.from_html(`<button>inc</button> <!>`, 1);

export default function App($$anchor) {
  let selected = 0;
  let count = $.state(5);
  var fragment = root_1();
  var button = $.first_child(fragment);
  var node = $.sibling(button, 2);
  $.key(
    node,
    () => selected,
    ($$anchor) => {
      var p = root();
      var text = $.child(p, true);
      $.reset(p);
      $.template_effect(() => $.set_text(text, $.get(count)));
      $.append($$anchor, p);
    },
  );
  $.delegated("click", button, () => $.update(count));
  $.append($$anchor, fragment);
}

$.delegate(["click"]);
