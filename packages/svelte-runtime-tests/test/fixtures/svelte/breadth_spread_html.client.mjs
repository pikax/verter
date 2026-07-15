import "svelte/internal/disclose-version";
import * as $ from "svelte/internal/client";

var root = $.from_html(`<div></div> <button>title</button> <button>html</button>`, 1);

export default function App($$anchor) {
  let title = $.state("a");
  let html = $.state("<strong>one</strong>");
  var fragment = root();
  var div = $.first_child(fragment);
  $.attribute_effect(div, () => ({ ...{ title: $.get(title) } }));
  $.html(div, () => $.get(html), true);
  $.reset(div);
  var button = $.sibling(div, 2);
  var button_1 = $.sibling(button, 2);
  $.delegated("click", button, () => $.set(title, $.get(title) + "!"));
  $.delegated("click", button_1, () => $.set(html, "<em>two</em>"));
  $.append($$anchor, fragment);
}

$.delegate(["click"]);
