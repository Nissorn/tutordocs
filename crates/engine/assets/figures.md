# Draw a chapter figure

Read this before you draw any figure. A drawn figure is one static SVG file under `CHAPTER_DIR/visuals/`. The reader shows that file inside an image, so nothing in the file runs. Steps, a replay, a slider, or a 3D view go to the interactive visual contract at the end of this file.

## What you write

- One SVG per figure, at `CHAPTER_DIR/visuals/<name>.svg`. `<name>` is one plain file name.
- The root `svg` element has a `viewBox` and no `width` or `height`. The reader scales the drawing.
- Colours are custom properties in a `<style>` block in that SVG. The dark values sit in `@media (prefers-color-scheme: dark)`.
- Text uses the system font stack: `-apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif`.
- No `script` element, no `animate` element, no external `href`, no `@import`, no web font, and no embedded raster. That bars an `image` element and a `data:` URL.
- Each label fits its box. The label budget says how many characters fit on a line. Wrap with `tspan`.
- Every arrow carries a verb.
- Embed the file as a markdown image. The title is the claim, one sentence: `![Cache miss path](visuals/cache-miss.svg "A cache miss reads the database.")`. The alt names the picture. The title states the claim.
- A PNG under `visuals/` is a picture you did not draw. A diagram is an SVG. The PNG still uses a title that states the claim.

## One claim per figure

Each figure argues one sentence, and that sentence is the markdown title. A second sentence is a second figure. Remove any mark that does not serve the claim.

## Draw the mechanism

Draw what happens. A request that misses a cache and then reads a database is those two steps, with the read drawn on the path. A box with the word "cache" on it is a label, not a figure.

Sketch the columns and rows in text, then turn them into coordinates. Leave at least 40 units between boxes. Draw each edge as orthogonal segments, `M x y H x` and `V y`. Keep a label about 9 units off its line so the stroke does not cut the word.

Past about 12 nodes, draw an overview of 5 to 8 nodes and put the rest in a second figure.

The shape says what a thing is.

- A rectangle is a service or a step.
- A cylinder, a rectangle with an ellipse on the top and the bottom, is a store.
- A rectangle with two or three short dividers is a queue.
- A dashed rectangle is optional, empty, or down.

## A verb on every arrow

Every edge has a verb the reader can check against the picture: `sends`, `reads`, `misses`, `polls 30s`. An arrow with no verb does not ship. The verb sits in the gap beside the line and obeys the label budget, using that gap as the width.

A solid edge is a sync call. A dashed edge is async, optional, or a check that failed. A thicker edge is the path this figure is about. The arrowhead shows direction. The edge does not move.

## State in form and in colour

Show each state in the shape, the stroke, or the position, and also in colour. Colour on its own does not carry the state. A miss is a dashed edge into a dashed empty box. The path the claim is about is thicker and uses the accent. A removed part is dashed. An added part is a solid accent stroke.

A before and an after are two panels in the same coordinates and the same scale. The file has no toggle, because it cannot run a script.

## Every number gets a picture

A number that measures a span sits on that span. A latency, a size, or a window gets a dimension line: extension lines from the two measured points, a line between them, a short 45 degree tick at each end, and the value centered about 6 units off the line. The value never sits on the line. Spans that add up share one rail, and the total sits on the next rail out. The dimension that carries the claim uses the accent. One accent dimension per figure.

A number that does not measure a span gets a small chart beside it. A count or a rank is a bar that starts at zero. A share of a whole is a row of equal cells, some filled and some empty. A trend is one polyline, about twice as wide as it is tall, with the values in an `aria-label`. Axes are hairlines. A gridline appears only when the reader must read a value off it. Sort bars by value unless the order itself means something.

## Cases become small multiples

Draw the same small diagram once per case, in the same coordinates, so only the change shows. The part that changes gets a different shape or stroke as well as the case colour. Each case has a short title and the diagram. Labels stay at 14 units or larger. Every copy uses one scale.

The example at the end is one figure, so it does not repeat itself. Use this section when the claim compares cases.

## Words

The title states the claim. "A cache miss reads the database." is a claim. "Cache path" names a topic and is the wrong title. One idea per sentence, and most sentences stay under 20 words. Active voice, present tense. One term per concept, and it is the term printed on the drawing. Spell an abbreviation the first time you use it. No idiom, no metaphor, no filler, and no hedge. Steps are a numbered list.

## Look

The ground is quiet. About nine tenths of the drawing is neutral. The accent marks the one thing the reader must see: the key node, the hot path, or the one number.

Every mark carries data or guides the eye. A chart has no frame, no filled plot background, no heavy grid, and no 3D bar. Build the drawing in layers, quiet first: ground, structure, data, then the accent. Two lines that sit close together form a third shape. Add space, or merge them.

A label sits on the data. It is horizontal, it includes the unit, and a long label wraps inside its box. Two sizes are enough: small and dim for a verb, larger and bright for a node name. Skip a legend under five series.

Use line weight 1 for structure, 1.5 for a box and an ordinary edge, and 2.5 for the signal.

One subject accent is enough. Status colours stay status colours: green for ok, red for risk. A dim label uses the muted token. Do not fade a label with opacity.

Use space before a box. Related marks sit closer than unrelated marks. Do not put a card inside a card. The SVG background is the ground token, and the dark set replaces it. Do not draw a white card behind the figure.

## Never

- Do not load a font. No Inter, no Roboto, no Arial file, no Google Fonts, and no CDN. Use the system stack in "What you write".
- Do not use a violet or fuchsia accent, neon, glass, a gradient fill, gradient text, or a blurred blob.
- Do not use clip art, a mascot, or an emoji as a marker.
- Do not center the drawing. A label starts inset from the left edge of its box.
- Do not put an accent bar on a rounded card.
- Do not number items `01`, `02`, `03` when the order does not matter.
- Do not use Mermaid, an HTML page, a Node script, or a render step. You write the SVG.
- Do not let a label run past its box.

## Label budget

SVG text does not wrap. You wrap it. A line holds at most `floor(box width / (0.6 × font size))` characters. `0.6 × font size` is the width of one character. Count every character, including spaces. A longer line breaks on a word boundary into another `tspan`. Each `tspan` repeats the text element's `x`. Each line after the first sets `dy="1.2em"`.

Worked example. The box is 120 units wide and the font size is 14. One character is `0.6 × 14 = 8.4` units. The budget is `floor(120 / 8.4) = floor(14.28) = 14` characters.

"reads the row" is 13 characters, so it fits on one line. "reads the row now" is 17 characters, so it does not fit. Break it after "row".

```xml
<text x="48" y="70" font-size="14">
  <tspan x="48">reads the row</tspan>
  <tspan x="48" dy="1.2em">now</tspan>
</text>
```

"reads the row" is 13, inside the budget of 14. "now" is 3. A verb uses the same formula, with the gap between the boxes as the width. If the verb does not fit in the gap, move the boxes apart. Do not set a font size below 12.

## Colour

Set these custom properties on the `svg` element. Light values are the default. Dark values live in `@media (prefers-color-scheme: dark)`. Paint text and strokes with the tokens.

| Token | Light | Dark | Use |
|---|---|---|---|
| `--bg` | `#f6f5f2` | `#1a1917` | ground |
| `--surface` | `#fffcf7` | `#262420` | box fill |
| `--ink` | `#241f1a` | `#f3efe6` | text and an ordinary stroke |
| `--muted` | `#5c564c` | `#b7b0a4` | a verb, an empty box, a dashed edge |
| `--line` | `#cfc6b8` | `#4d473f` | a hairline or an axis |
| `--accent` | `#1d6b45` | `#8ed4ae` | the one signal |
| `--store` | `#8a5520` | `#e2b58a` | a store's stroke |

The example below puts this palette in a `<style>` block. Copy that block.

## A complete example

The claim is "A cache miss reads the database." The cache is dashed because the lookup found it empty, and the read is a thicker accent edge with the 30 ms it costs. `Client`, `API`, `DB`, and `Cache` each fit their box: the budget is `floor(116 / (0.6 × 14)) = 13`, and the longest of those names is 6 characters. `sends`, `reads`, `misses`, and `30 ms` each fit their gap: the budget is `floor(72 / (0.6 × 12)) = 10`, and the longest of those labels is 6 characters.

```markdown
![Cache miss path](visuals/cache-miss.svg "A cache miss reads the database.")
```

```svg
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 520 228">
  <title>A cache miss reads the database.</title>
  <style>
    svg {
      background: var(--bg);
      --bg: #f6f5f2;
      --surface: #fffcf7;
      --ink: #241f1a;
      --muted: #5c564c;
      --line: #cfc6b8;
      --accent: #1d6b45;
      --store: #8a5520;
    }
    @media (prefers-color-scheme: dark) {
      svg {
        --bg: #1a1917;
        --surface: #262420;
        --ink: #f3efe6;
        --muted: #b7b0a4;
        --line: #4d473f;
        --accent: #8ed4ae;
        --store: #e2b58a;
      }
    }
    text {
      font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif;
      fill: var(--ink);
      stroke: none;
    }
    .box { fill: var(--surface); stroke: var(--ink); stroke-width: 1.5; }
    .empty { fill: var(--surface); stroke: var(--muted); stroke-width: 1.5; stroke-dasharray: 5 4; }
    .store { fill: var(--surface); stroke: var(--store); stroke-width: 1.5; }
    .edge { fill: none; stroke: var(--ink); stroke-width: 1.5; }
    .hot { fill: none; stroke: var(--accent); stroke-width: 2.5; }
    .miss { fill: none; stroke: var(--muted); stroke-width: 1.5; stroke-dasharray: 5 4; }
    .verb { fill: var(--muted); }
    .dim { fill: none; stroke: var(--accent); stroke-width: 1; }
    .measure { fill: var(--accent); }
  </style>
  <defs>
    <marker id="head" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="8" markerHeight="8" orient="auto">
      <path d="M0 0 L10 5 L0 10 Z" fill="var(--ink)"/>
    </marker>
    <marker id="hot-head" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="8" markerHeight="8" orient="auto">
      <path d="M0 0 L10 5 L0 10 Z" fill="var(--accent)"/>
    </marker>
    <marker id="miss-head" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="8" markerHeight="8" orient="auto">
      <path d="M0 0 L10 5 L0 10 Z" fill="var(--muted)"/>
    </marker>
  </defs>
  <rect class="box" x="24" y="28" width="116" height="56"/>
  <text x="36" y="61" font-size="14">Client</text>
  <rect class="box" x="212" y="28" width="116" height="56"/>
  <text x="224" y="61" font-size="14">API</text>
  <path class="edge" d="M140 56 H204" marker-end="url(#head)"/>
  <text class="verb" x="148" y="46" font-size="12">sends</text>
  <path class="hot" d="M328 56 H392" marker-end="url(#hot-head)"/>
  <text class="verb" x="336" y="46" font-size="12">reads</text>
  <g class="dim">
    <path d="M336 64 V90 M392 64 V90 M336 90 H392"/>
    <path d="M336 90 l5 -4 M336 90 l5 4 M392 90 l-5 -4 M392 90 l-5 4"/>
    <text class="measure" x="346" y="84" font-size="12">30 ms</text>
  </g>
  <rect class="store" x="400" y="44" width="80" height="36"/>
  <ellipse class="store" cx="440" cy="80" rx="40" ry="11"/>
  <ellipse class="store" cx="440" cy="44" rx="40" ry="11"/>
  <text x="412" y="70" font-size="14">DB</text>
  <rect class="empty" x="212" y="156" width="116" height="52"/>
  <text x="224" y="186" font-size="14">Cache</text>
  <path class="miss" d="M270 84 V148" marker-end="url(#miss-head)"/>
  <text class="verb" x="280" y="122" font-size="12">misses</text>
</svg>
```

## When the picture must move

A figure that needs steps, a replay, a control, or depth is an interactive visual. Write a fenced block with the info string `visual <id>`. Its body is the markdown the reader shows when the visual does not start.

````markdown
```visual cache-miss
A cache miss reads the database.
```
````

The files live in `CHAPTER_DIR/visuals/<id>/` and include `index.html` and `manifest.json`. CSS, script, and SVG in that page are inline. The host inlines the vendored three.js build. That is the only outside code. No CDN, no Google Fonts, no remote fetch, and no remote schema.

### Interactive visual manifest contract

The reader validates `CHAPTER_DIR/visuals/<id>/manifest.json` before displaying the artifact.

Required fields:

- `schemaVersion`: 1 (number). Must be 1.
- `id`: string. Must match the visual directory name `<id>`.
- `kind`: `"svg"`, `"canvas"`, or `"three"`.
- `title`: string. A concise title describing the visual.

Optional fields:

- `entry`: `"index.html"` (string, default `"index.html"`). If present, it must be `"index.html"`.
- `vendor`: array of strings (default `[]`). When `kind` is `"three"`, only `"three@0.169.0"` is permitted.
- `estimatedHeight`: height in pixels (number, default 420).

Do not invent fields. Any manifest missing required fields is rejected.

Complete valid example:

```json
{
  "schemaVersion": 1,
  "id": "cache-miss",
  "kind": "canvas",
  "title": "Cache miss walkthrough"
}
```

If you cannot meet this contract, do not emit an interactive visual.

## Tables

A table is a markdown pipe table. Do not draw a table as text in a code fence. Draw an SVG for a table only when the claim is about a spatial layout.

## Notice

This guide is adapted from visual-explainer at 9d5f880.

MIT License

Copyright (c) 2025 Nico Bailon

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
