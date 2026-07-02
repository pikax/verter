import "svelte/internal/disclose-version";
import * as $ from "svelte/internal/client";

var root = $.from_html(`<div>x</div>`);
var root_1 = $.from_html(`<button>t</button> <!>`, 1);

export default function App($$anchor, $$props) {
  let show = $.state(false);
  var fragment = root_1();
  var button = $.first_child(fragment);
  var node = $.sibling(button, 2);
  {
    var consequent = ($$anchor) => {
      var div = root();
      $.transition(3, div, () => $$props.fx);
      $.append($$anchor, div);
    };
    $.if(node, ($$render) => {
      if ($.get(show)) $$render(consequent);
    });
  }
  $.delegated("click", button, () => $.set(show, !$.get(show)));
  $.append($$anchor, fragment);
}

$.delegate(["click"]);
