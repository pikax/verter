import "svelte/internal/disclose-version";
import * as $ from "svelte/internal/client";

var root = $.from_html(`<p>shown</p>`);

export default function App($$anchor) {
  let show = true;
  var fragment = $.comment();
  var node = $.first_child(fragment);
  {
    var consequent = ($$anchor) => {
      var p = root();
      $.append($$anchor, p);
    };
    $.if(node, ($$render) => {
      if (show) $$render(consequent);
    });
  }
  $.append($$anchor, fragment);
}
