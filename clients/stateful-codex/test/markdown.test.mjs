import assert from "node:assert/strict";
import test from "node:test";

import { renderMarkdown } from "../public/markdown.mjs";

test("agent prose renders as formatted, escaped Markdown", () => {
  const source = [
    "## What changed",
    "",
    "Added **strict** `--month YYYY-MM` filtering in [cli.py](C:/work/textkit/cli.py) and *tests*.",
    "Second line of the same paragraph.",
    "",
    "- first <item>",
    "- second",
    "",
    "1. run tests",
    "2. commit",
    "",
    "```python",
    "print('<b>')",
    "```",
    "",
    "| Check | Result |",
    "| --- | --- |",
    "| tests | 18 passed |",
    "",
    "See [docs](https://example.com/a?b=1&c=2).",
  ].join("\n");

  assert.equal(
    renderMarkdown(source),
    [
      '<h4 class="md-heading">What changed</h4>',
      '<p>Added <strong>strict</strong> <code>--month YYYY-MM</code> filtering in <code class="md-path" title="C:/work/textkit/cli.py">cli.py</code> and <em>tests</em>.<br/>Second line of the same paragraph.</p>',
      "<ul><li>first &lt;item&gt;</li><li>second</li></ul>",
      "<ol><li>run tests</li><li>commit</li></ol>",
      '<pre class="md-code" data-language="python"><code>print(&#39;&lt;b&gt;&#39;)</code></pre>',
      '<div class="md-table"><table><thead><tr><th>Check</th><th>Result</th></tr></thead><tbody><tr><td>tests</td><td>18 passed</td></tr></tbody></table></div>',
      '<p>See <a href="https://example.com/a?b=1&amp;c=2" target="_blank" rel="noopener noreferrer">docs</a>.</p>',
    ].join(""),
  );
});

test("markup in prose and link targets can never become live HTML", () => {
  const html = renderMarkdown(
    '<script>alert(1)</script> [x](javascript:alert) [y](" onmouseover="z) **<img src=x>**',
  );

  assert.doesNotMatch(html, /<script|<img|href="javascript|onmouseover="/);
  assert.match(html, /&lt;script&gt;/);
  assert.match(html, /<code class="md-path" title="javascript:alert">x<\/code>/);
});

test("code spans keep their content literal and identifiers keep their underscores", () => {
  assert.equal(
    renderMarkdown("Use `**not bold**` in parse_iso_date and snake_case_name."),
    "<p>Use <code>**not bold**</code> in parse_iso_date and snake_case_name.</p>",
  );
});
