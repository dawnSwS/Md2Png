#set page(width: 800pt, height: auto, margin: 40pt, fill: rgb("ffffff"))
#set text(font: ("Microsoft YaHei", "Segoe UI", "SimSun", "Segoe UI Emoji", "Libertinus Serif"), size: 32pt)
#set par(leading: 1.5em, justify: true)
#show raw: set text(font: ("Consolas", "DejaVu Sans Mono", "Microsoft YaHei", "SimSun"))
#show math.equation: set text(font: ("New Computer Modern Math", "Cambria Math", "Microsoft YaHei", "Segoe UI", "SimSun", "Segoe UI Emoji"))

// Resolve inline-math width at paragraph/container level so ordinary formulas
// stay in the text flow, while a single oversized formula cannot cross margins.
#let md-paragraph(body) = layout(size => {
  show math.equation.where(block: false): it => context {
    let natural = measure(it)
    if natural.width > size.width and size.width > 0pt {
      box(scale(size.width / natural.width * 100%, reflow: true, it))
    } else {
      it
    }
  }
  body
})

// Fit wide equations to their current container without rotating the reading direction.
#show math.equation.where(block: true): it => context {
  layout(size => {
    let max-w = calc.min(720pt, size.width)
    let natural = measure(it)
    if natural.width > max-w and max-w > 0pt {
      let ratio = max-w / natural.width
      align(center, scale(x: ratio * 100%, y: ratio * 100%, reflow: true, it))
    } else {
      align(center, it)
    }
  })
}

// Inline code is breakable; a box would clip long commands at the page edge.
#show raw.where(block: false): it => highlight(
  fill: rgb("f6f8fa"),
  text(
    font: ("Consolas", "DejaVu Sans Mono", "Libertinus Mono"),
    size: 0.85em,
    it.text.clusters().join("\u{200b}"),
  ),
)
// Long URLs and identifiers can wrap without splitting grapheme clusters.
// CJK prose already has native break opportunities; keep punctuation with its text.
#show regex("[\\p{Latin}\\p{Greek}\\p{Cyrillic}\\p{N}\\p{M}_:/?&=%#@.+~-]{16,}"): it => text(it.text.clusters().join("\u{200b}"))

#show raw.where(block: true): it => context {
  layout(size => {
    let max_w = calc.min(720pt, size.width)
    let unconstrained = block(fill: rgb("f6f8fa"), inset: 24pt, radius: 8pt, width: auto, text(size: 20pt, it))
    let m = measure(unconstrained)
    if m.width > max_w {
      let ratio = max_w / m.width
      scale(x: ratio * 100%, y: ratio * 100%, origin: left + top, reflow: true, unconstrained)
    } else {
      block(fill: rgb("f6f8fa"), inset: 24pt, radius: 8pt, width: 100%, text(size: 20pt, it))
    }
  })
}

// Keep the table at the container width. Scaling an entire table makes long
// text unreadable and prevents natural table layout; only oversized math atoms
// are scaled inside their own cells.
#set table(
  inset: (x: 0pt, y: 8pt),
  stroke: 0.6pt + rgb("d0d7de"),
  fill: (x, y) => if y == 0 { rgb("eef2f6") } else if calc.odd(y) { white } else { rgb("f8fafc") },
)
#show table: set text(size: 24pt)
#show table: set par(justify: false, leading: 0.45em)

#let md-table-cell(body) = layout(size => {
  let padding = calc.min(8pt, size.width / 8)
  let available = size.width - 2 * padding
  // Very many columns must not overflow merely from padding or one glyph.
  set text(size: calc.min(24pt, available))
  // Long identifiers and URLs get invisible break opportunities. Use grapheme
  // clusters so combining accents and joined emoji remain intact.
  show regex("[\\p{Latin}\\p{Greek}\\p{Cyrillic}\\p{N}\\p{M}_:/?&=%#@.+~-]+"): it => context {
    if measure(it).width > available {
      text(it.text.clusters().join("\u{200b}"))
    } else {
      it
    }
  }
  show raw.where(block: false): it => highlight(
    fill: rgb("f6f8fa"),
    text(
      font: ("Consolas", "DejaVu Sans Mono", "Libertinus Mono"),
      size: 0.85em,
      it.text.clusters().join("\u{200b}"),
    ),
  )
  // This local rule also handles inline math. The global display-equation rule
  // is deliberately overridden here: a rotated formula cannot fit a table row.
  show math.equation: it => context {
    let m = measure(it)
    if m.width > available and available > 0pt {
      let ratio = available / m.width
      box(scale(ratio * 100%, reflow: true, it))
    } else {
      it
    }
  }
  pad(x: padding, body)
})
