// Copyright © 2026 Jalapeno Labs

// Markdown as pages hold it, by what makes it hard to carry through an editor unchanged. Written
// oddly on purpose: markers, padding, and escapes that a formatter would change are the point.
export const markdownCorpus = {
  empty: '',
  plain: 'One paragraph.',
  leadingBlankLines: '\n\nStarts after blank lines.',
  extraBlankLines: 'First.\n\n\n\nSecond, three blank lines down.',
  noBlankLine: '# Heading\nText right under it.\n- A list right under that',
  headings: '# One\n\n## Two ##\n\nSetext one\n===\n\nSetext two\n---\n\n###### Six',
  emphasis: '*one* _two_ **three** __four__ ***five*** ~~six~~ ~seven~',
  nestedEmphasis: '**bold with *italic* inside** and *italic with **bold** inside* and ***a** b*',
  specialCharacters: [
    'snake_case and 2 * 3 * 4 and a_b_c, ~/.config, [brackets], <angle>, and 100% & more.',
    '\\*not bold\\* and \\_not italic\\_ and \\# not a heading and 1\\. not a list.',
    '\\- not a list, a pipe | and a backtick \\` and &amp; an entity &copy; &#35; too.',
  ].join('\n\n'),
  lineStarts: '\\* star\n\n\\+ plus\n\n\\> quote\n\n\\# hash\n\n1\\. one\n\n\\---',
  softBreaks: 'A hard-wrapped paragraph\nkeeps its lines,\n  even an indented one.',
  hardBreaks: 'Two spaces  \nand a backslash\\\nboth break a line.',
  unicode: 'Résumé, naïve café, 日本語, and emoji 🎉. Nonbreaking.',
  links: [
    '[inline](https://example.com) and [with title](https://example.com "Title") and',
    '[relative](/knowledge/pages/1) and <https://auto.example.com> and https://bare.example.com',
    'and mail@example.com and [empty]() and [`code`](https://example.com) and',
    '[**bold**](https://example.com).',
  ].join('\n'),
  linkInsideBold: '**[bold outside](https://example.com)** and [**bold inside**](https://example.com).',
  referenceLinks: 'A [full][one], a [collapsed][], and a [shortcut].\n\n[one]: https://one.example.com\n'
    + '[collapsed]: https://two.example.com "Two"\n[shortcut]: <https://three.example.com>',
  images: [
    '![alt](/api/v1/image.png) and ![alt](https://example.com/a.png "Title") and ![](/empty.png)',
    '[![linked](/image.png)](https://example.com)',
    '![reference][image]',
    '[image]: /image.png',
  ].join('\n\n'),
  bulletLists: '- dash\n- dash\n\n* star\n* star\n\n+ plus\n+ plus',
  nestedLists: '- one\n  - two\n    - three\n      more of three\n- four\n\n    still four',
  looseList: '- one\n\n- two\n\n  second paragraph\n\n- three',
  orderedLists: '1. one\n2. two\n3. three\n\n3) three\n4) four\n\n1. same\n1. same\n1. same',
  orderedFromSeven: '7. seven\n8. eight\n9. nine\n10. ten',
  mixedList: '- one\n  1. two\n  2. three\n- four\n  > quoted\n- ```\n  code first\n  ```',
  taskLists: '- [ ] open\n- [x] done\n  - [ ] nested\n\n* [X] upper\n\n- [ ] task\n- plain in the same list',
  emptyItems: '-\n- has text\n-\n\n1.\n2. has text',
  indentedList: ' - one space\n - one space\n\n   - three spaces',
  tableAligned: [
    '| Left | Center | Right | None |',
    '|:-----|:------:|------:|------|',
    '| a    | b      | c     | d    |',
    '| long | longer | longest text | e |',
  ].join('\n'),
  tableBare: 'Name | Role\n--- | ---\nAlice | Lead\nBob | Dev',
  tableEscapes: [
    '| Pipe | Code | Marks |',
    '| --- | --- | --- |',
    '| a \\| b | `x \\| y` | **bold** and [link](/x) |',
    '|  | empty before | <br> html |',
  ].join('\n'),
  tableShortRow: '| a | b | c |\n|---|---|---|\n| 1 |\n| 1 | 2 | 3 | 4 |',
  tableHeaderOnly: '| a | b |\n| - | - |',
  codeFences: [
    '```ts',
    'const value = `template`',
    '```',
    '',
    '```python title="x.py" linenos',
    'print("hi")',
    '```',
    '',
    '~~~',
    'tildes',
    '~~~',
    '',
    '````md',
    '```js',
    'nested()',
    '```',
    '````',
    '',
    '```',
    '```',
    '',
    '```',
    '  keeps its indent \\ and * and _',
    '',
    '',
    'and blank lines',
    '```',
  ].join('\n'),
  indentedCode: 'Before.\n\n    indented code\n      more\n\nAfter.',
  unclosedFence: 'Before.\n\n```ts\nnever closed',
  inlineCode: 'Some `code`, ``with ` tick``, `` `edges` ``, and `*not emphasis*`.',
  blockquotes: '> one\n> two\n\n> # heading\n>\n> - list\n> - list\n>\n> ```\n> code\n> ```\n\n'
    + '> lazy\ncontinuation',
  nestedQuote: '> outer\n>\n> > inner\n>\n> outer again\n\n>no space',
  thematicBreaks: '---\n\n***\n\n___\n\n- - -',
  rawHtml: [
    '<div class="x">',
    'block html',
    '</div>',
    '',
    'Inline <b>html</b> and a Vec<T> and sessions/<id> too.',
    '',
    '<!-- a comment -->',
    '',
    '<script>alert(1)</script>',
    '',
    '<details>',
    '<summary>More</summary>',
    '',
    'Inside.',
    '',
    '</details>',
  ].join('\n'),
  footnotes: 'A claim.[^1] Another.[^long]\n\n[^1]: The note.\n\n[^long]: First paragraph.\n\n    Second paragraph.',
  dangerousAddresses: '[run](javascript:alert(1)) and ![pixel](data:image/png;base64,AAAA) and [mail](mailto:a@b.c)',

  // The company doc, whose seven sections take these shapes (docs/knowledge.md, Sections).
  companyDocCompany: 'Acme Corp makes verification tools for chip designers, from Austin, TX.\n\n'
    + 'Founded in 2021; about 40 people.',
  companyDocLeadership: '- **Priya Raman**, CEO: strategy, fundraising\n'
    + '- **Tom Okafor**, CTO: engineering, security\n- **Lena Fischer**, COO: operations',
  companyDocPeople: '- Priya Raman (misheard: "Maya Ramen"): CEO\n- Tom Okafor (misheard: "Tom O\'Connor"): CTO\n'
    + '- Jürgen Weiß: firmware, "Yergen"',
  companyDocStakeholders: '## Clients\n\n- Northwind: Sara Lee speaks for them\n\n## Partners\n\n'
    + '| Partner | Contact | Since |\n|---------|---------|-------|\n| Contoso | Raj | 2023 |',
  companyDocProducts: '### Lint\n\nA static checker. Sold per seat.\n\n### Formal\n\n'
    + 'A bounded prover, `formal-cli` on the command line.',
  companyDocProjects: '- **Falcon**: tape-out by Q3; Tom, Jürgen\n- **Harbor**: SOC 2 audit; Lena',
  companyDocGlossary: '- AoU: assumption of use\n- CDC (misheard "CDs"): clock domain crossing\n'
    + '- UCIe: Universal Chiplet Interconnect Express, *not* "you see"',

  // A learning's body and its evidence.
  learningBody: 'Run the linter before the build.\n\n'
    + 'It fails faster, and `yarn lint` is 10x quicker than `yarn build`.',
  learningEvidence: 'Caught on [WAY-123](https://youtrack.example.com/issue/WAY-123):\n\n'
    + '```text\nerror: unused import\n```\n\n- 3 of 4 failures in October\n- see `ci/run_17.log`',
} as const satisfies Record<string, string>
