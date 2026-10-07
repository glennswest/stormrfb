// The browser half of tools/check-slides.sh (#11): open the deck Marp
// rendered with the bare template (one 1280×720 <section> per slide) and,
// for each slide, say whether anything overflows it: the section's own
// content past its box, or a code block or table wider or taller than
// the space it was given. Each slide is also printed as a base64 PNG line
// (`PNG slide-NN <base64>`) so a reader of the build log can look at it.
const { chromium } = require('playwright')

;(async () => {
  const browser = await chromium.launch()
  const page = await browser.newPage({ viewport: { width: 1280, height: 720 } })
  await page.goto(`file://${process.env.DECK}`)
  await page.waitForLoadState('networkidle')
  const report = await page.evaluate(() =>
    [...document.querySelectorAll('section')].map((s, i) => {
      const box = s.getBoundingClientRect()
      const over = []
      if (s.scrollHeight > s.clientHeight + 1) over.push(`content ${s.scrollHeight}px tall in ${s.clientHeight}px`)
      if (s.scrollWidth > s.clientWidth + 1) over.push(`content ${s.scrollWidth}px wide in ${s.clientWidth}px`)
      // Anything drawn below or right of the slide, and wide blocks that scroll.
      for (const el of s.querySelectorAll('h1,h2,h3,p,li,pre,table,img,blockquote')) {
        const r = el.getBoundingClientRect()
        if (r.bottom > box.bottom + 1) over.push(`${el.tagName.toLowerCase()} ends ${Math.round(r.bottom - box.bottom)}px below the slide`)
        if (r.right > box.right + 1) over.push(`${el.tagName.toLowerCase()} ends ${Math.round(r.right - box.right)}px right of the slide`)
        if ((el.tagName === 'PRE' || el.tagName === 'TABLE') && el.scrollWidth > el.clientWidth + 1)
          over.push(`${el.tagName.toLowerCase()} is ${el.scrollWidth}px wide in ${el.clientWidth}px`)
      }
      const title = (s.querySelector('h1,h2')?.textContent || '').trim()
      return { n: i + 1, title, width: Math.round(box.width), height: Math.round(box.height), over: [...new Set(over)] }
    })
  )
  let bad = 0
  for (const r of report) {
    const ok = r.over.length === 0
    if (!ok) bad++
    console.log(`  ${ok ? 'ok  ' : 'OVER'} slide ${String(r.n).padStart(2)} ${r.width}x${r.height} ${r.title}${ok ? '' : ' — ' + r.over.join('; ')}`)
  }
  const sections = await page.$$('section')
  for (let i = 0; i < sections.length; i++) {
    const png = await sections[i].screenshot()
    console.log(`PNG slide-${String(i + 1).padStart(2, '0')} ${png.toString('base64')}`)
  }
  console.log(`\n${report.length} slides, ${bad} overflowing`)
  await browser.close()
  process.exit(bad ? 1 : 0)
})().catch((e) => {
  console.error(e)
  process.exit(2)
})
