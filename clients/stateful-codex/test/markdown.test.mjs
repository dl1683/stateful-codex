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

test("nested lists, escaped table pipes and angle-bracket links keep their meaning", () => {
  assert.equal(
    renderMarkdown(["1. Build", "   - compile", "   - link", "2. Test", "   still step two"].join("\n")),
    "<ol><li>Build<ul><li>compile</li><li>link</li></ul></li><li>Test<br/>still step two</li></ol>",
  );
  assert.equal(
    renderMarkdown(["| Expr | Result |", "| --- | --- |", "| `a \| b` | pass |"].join("\n")),
    '<div class="md-table"><table><thead><tr><th>Expr</th><th>Result</th></tr></thead><tbody><tr><td><code>a | b</code></td><td>pass</td></tr></tbody></table></div>',
  );
  assert.equal(
    renderMarkdown("[Report](<C:/My Project/report.md>) and [f](C:/a_(1)/f.md)"),
    '<p><code class="md-path" title="C:/My Project/report.md">Report</code> and <code class="md-path" title="C:/a_(1)/f.md">f</code></p>',
  );
});

test("adversarial agent output renders quickly, without unbounded output or recursion", () => {
  const cases = [
    "# before\u2028after",
    "x\u2029# heading",
    "[".repeat(65_536),
    "[a](".repeat(16_384),
    `| a |\n|${" ".repeat(65_536)}X`,
    `${">".repeat(10_000)}x`,
    "**a ".repeat(16_384),
    "*a ".repeat(21_845),
    `${"`".repeat(300)}${"x`".repeat(30_000)}`,
    `${"- a\n\n".repeat(10_000)}`,
    `${"  ".repeat(5_000)}- deep`,
    `# x${" ".repeat(65_536)}X`,
    Array.from({ length: 700 }, (_, index) => `${"`".repeat(700 - index)}a`).join(""),
    "[a](<".repeat(60_000),
    '[a](b "'.repeat(40_000),
    "*a _b ".repeat(15_000),
  ];
  for (const source of cases) {
    const started = performance.now();
    const html = renderMarkdown(source);
    const elapsed = performance.now() - started;
    assert.ok(elapsed < 500, `${JSON.stringify(source.slice(0, 20))} took ${elapsed} ms`);
    assert.ok(html.length < source.length * 12 + 1_000);
  }

  const header = `|${" h |".repeat(300)}`;
  const separator = `|${" --- |".repeat(300)}`;
  const source = [header, separator, ...Array(5_000).fill("|")].join("\n");
  const table = renderMarkdown(source);
  // Rows beyond the bound stay visible as text; the output stays proportional to the input.
  assert.ok((table.match(/<td>/g)?.length ?? 0) <= 2_000);
  assert.ok(table.length < source.length * 12 + 100_000);
  assert.match(renderMarkdown("# before\u2028after"), /^<h3 class="md-heading">before<\/h3><p>after<\/p>$/);
});

test("emphasis nests, underscores respect word boundaries, and code spans need equal runs", () => {
  assert.equal(
    renderMarkdown("*outer **inner** outer* and __bold__ _it_ in parse_iso_date"),
    "<p><em>outer <strong>inner</strong> outer</em> and <strong>bold</strong> <em>it</em> in parse_iso_date</p>",
  );
  assert.equal(renderMarkdown("Use `a``b`."), "<p>Use <code>a``b</code>.</p>");
  assert.equal(
    renderMarkdown("- parent\n  - child\n\n  after child"),
    "<ul><li>parent<ul><li>child</li></ul>after child</li></ul>",
  );
});

test("one long opener matched by many closers stays linear", () => {
  const source = `x ${"*".repeat(20_000)}${"a* ".repeat(20_000)}`;
  const started = performance.now();
  renderMarkdown(source);
  assert.ok(performance.now() - started < 500);
});
