import "svelte/internal/disclose-version";
import * as $ from "svelte/internal/client";

var root = $.from_html(`<p> </p>`);
var root_1 = $.from_html(`<button>swap</button> <!>`, 1);

export default function App($$anchor, $$props) {
  let flipped = $.state(false);
  var fragment = root_1();
  var button = $.first_child(fragment);
  var node = $.sibling(button, 2);
  $.each(
    node,
    25,
    () => ($.get(flipped) ? ["b", "a"] : ["a", "b"]),
    (item) => item,
    ($$anchor, item) => {
      var p = root();
      var text = $.child(p, true);
      $.reset(p);
      $.template_effect(() => $.set_text(text, $.get(item)));
      $.animation(p, () => $$props.fx, null);
      $.append($$anchor, p);
    },
  );
  $.delegated("click", button, () => $.set(flipped, !$.get(flipped)));
  $.append($$anchor, fragment);
}

$.delegate(["click"]);
