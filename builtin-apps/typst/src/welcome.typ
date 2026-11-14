#set page(paper: "a4", margin: (x: 2.6cm, y: 2.8cm), numbering: "1")
#set text(font: "Libertinus Serif", size: 11pt)
#set par(justify: true)
#set heading(numbering: "1.")
#show heading: set block(above: 1.6em, below: 0.8em)
#show link: set text(fill: rgb("#0f6cbd"))

#align(center)[
  #text(size: 26pt, weight: "bold")[Writing with Typst]
  #v(-0.6em)
  #text(size: 12pt, fill: luma(100))[A short tour, typeset as you type]
  #v(0.8em)
]

= Start here

This is a document. Its text is on one side, written in Typst's markup, and its pages are on the other, set again each time you pause. Change a word and watch it move.

- *Bold* is written between stars, and _emphasis_ between underscores.
- A heading starts with `=`, a list item with `-`, a numbered one with `+`.
- Whatever follows a `#` is code: `#lorem(20)` writes twenty words of filler.

A mistake is no accident to be afraid of: it is underlined where it is, and listed under the text with its line. Delete a closing bracket somewhere to see one.

= Mathematics

A formula goes between dollar signs. With a space inside each, it stands on a line of its own:

$ integral_(-oo)^oo e^(-x^2) dif x = sqrt(pi) $

and without, as in $e^(i pi) + 1 = 0$, it sits in the sentence. Fractions, sums and matrices are written the way they are read:

$ sum_(k=1)^n k = (n(n+1)) / 2, quad
  A = mat(1, 2; 3, 4), quad
  f(x) = cases(x^2 "if" x >= 0, -x "otherwise") $

= Tables and figures

#figure(
  table(
    columns: (1fr, auto, auto),
    align: (left, right, right),
    stroke: none,
    table.hline(),
    table.header[*Planet*][*Radius in km*][*Moons*],
    table.hline(stroke: 0.5pt),
    [Mercury], [2 440], [0],
    [Earth], [6 371], [1],
    [Mars], [3 390], [2],
    [Neptune], [24 622], [16],
    table.hline(),
  ),
  caption: [Four planets of the solar system.],
) <planets>

@planets is a table inside a figure, so it is numbered, and the text refers to it by its label: `@planets`. Change the order of the figures, and every reference follows.

#pagebreak()

= Drawing with code

Markup and code are one language. The chart in @radii is drawn from a list of numbers: each bar is a rectangle whose height is computed, filled with a gradient.

#let planets = (
  (name: "Mercury", radius: 2440),
  (name: "Mars", radius: 3390),
  (name: "Earth", radius: 6371),
  (name: "Neptune", radius: 24622),
)

#let bar(planet) = stack(
  spacing: 5pt,
  text(size: 8pt, fill: luma(90), str(planet.radius)),
  rect(
    width: 46pt,
    height: planet.radius / 200 * 1pt,
    radius: (top: 3pt),
    fill: gradient.linear(rgb("#7ab8f5"), rgb("#0f6cbd"), angle: 90deg),
  ),
  text(size: 9pt, planet.name),
)

#figure(
  stack(dir: ltr, spacing: 18pt, ..planets.map(planet => align(bottom, bar(planet)))),
  caption: [The planets' radii, in kilometres.],
) <radii>

Add a planet to the list and the chart grows a bar.

= Code and layout

Source code is set in a fixed-width font, coloured by its language:

```rust
fn main() {
    println!("Typeset inside a WaOS program.");
}
```

#grid(
  columns: (1fr, 1fr),
  gutter: 16pt,
  [
    A grid places content side by side. This is its first cell, a paragraph of text that wraps within its column.
  ],
  block(fill: rgb("#eef5fc"), inset: 10pt, radius: 4pt)[
    *A note.* Blocks have fills, insets and rounded corners; this is one, in the second cell.
  ],
)

= What next

- *Export PDF* writes these pages as a PDF.
- *New* starts an empty document; *Save* keeps it as a `.typ` file.
- Typst's own guide and reference are at #link("https://typst.app/docs").

The fonts a document can name here are the ones this app carries: Libertinus Serif, New Computer Modern Math and DejaVu Sans Mono.
