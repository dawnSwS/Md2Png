#set page(width: 800pt, height: auto, margin: 40pt, fill: rgb("ffffff"))
#set text(font: ("Microsoft YaHei", "Segoe UI", "SimSun", "Segoe UI Emoji", "Libertinus Serif"), size: 32pt)
#set par(leading: 1.5em, justify: true)
#show raw: set text(font: ("Consolas", "DejaVu Sans Mono", "Microsoft YaHei", "SimSun"))
#show math.equation: set text(font: ("New Computer Modern Math", "Cambria Math", "Microsoft YaHei", "Segoe UI", "SimSun", "Segoe UI Emoji"))

// Preserve the original wide-equation rotation, but respect nested container width.
#show math.equation.where(block: true): it => context {
  layout(size => {
    let max_w = calc.min(720pt, size.width)
    let unconstrained = block(width: auto, it)
    let m = measure(unconstrained)
    if m.width > max_w {
      let rotated = rotate(-90deg, reflow: true, unconstrained)
      let m_rot = measure(rotated)
      if m_rot.width > max_w {
        let ratio = max_w / m_rot.width
        align(center, scale(x: ratio * 100%, y: ratio * 100%, reflow: true, rotated))
      } else {
        align(center, rotated)
      }
    } else {
      align(center, it)
    }
  })
}

#show raw.where(block: false): box.with(fill: rgb("f6f8fa"), inset: (x: 4pt, y: 0pt), outset: (y: 3pt), radius: 2pt)

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

#show table: it => context {
  layout(size => {
    let max_w = calc.min(720pt, size.width)
    let m = measure(it)
    if m.width > max_w {
      let ratio = max_w / m.width
      scale(x: ratio * 100%, y: ratio * 100%, origin: left + top, reflow: true, it)
    } else {
      it
    }
  })
}
