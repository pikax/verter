import "svelte/internal/disclose-version";
import * as $ from "svelte/internal/client";

var root = $.from_html(`<button>update</button> <!>`, 1);

export default function App($$anchor, $$props) {
  let label = $.state("a");
  var fragment = root();
  var button = $.first_child(fragment);
  var node = $.sibling(button, 2);
  $.component(
    node,
    () => $$props.Child,
    ($$anchor, $$component) => {
      $$component($$anchor, {
        get text() {
          return $.get(label);
        },
      });
    },
  );
  $.delegated("click", button, () => $.set(label, $.get(label) + "!"));
  $.append($$anchor, fragment);
}

$.delegate(["click"]);
