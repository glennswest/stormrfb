// The browser half of tools/verify-relay.sh (#4): stormconsole's VM page,
// Graphical console, with stormrfb and noVNC attached to the same guest at
// the same time through the console's relay.
//
// Legible means: what each client drew on its canvas is the guest's
// framebuffer, compared pixel by pixel with qemu's own `screendump` (QMP)
// taken at the same moment on a screen that has stopped changing.
// Driven means: keys typed at stormrfb's canvas reach the guest (Alpine
// logs in and clears its screen; Windows boots from the ISO only if a key
// is pressed at "Press any key", then Setup moves to its next page on Enter).
// Measured means: per client and per phase, the bytes the relay delivered,
// the FramebufferUpdateRequests the client sent (one per completed update,
// so "per frame"), and the time spent in the socket's message handler —
// which for both clients is decode plus canvas paint, synchronously.
const fs = require('fs')
const net = require('net')
const path = require('path')
const { chromium } = require('playwright')

const BASE = process.env.CONSOLE
const RUN = process.env.RUN
let failed = 0
const errors = []
function check(ok, what, extra = '') {
  console.log(`  ${ok ? 'ok  ' : 'FAIL'} ${what}${extra ? ` — ${extra}` : ''}`)
  if (!ok) failed++
  return ok
}
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
const sock = (vm, s) => path.join(RUN, 'default', vm, s)

// ---- QMP, on the script's own socket ------------------------------------
function qmp(vm, cmd) {
  return new Promise((resolve, reject) => {
    const s = net.createConnection(sock(vm, 'qmp.sock'))
    let buf = ''
    let negotiated = false
    s.on('data', (d) => {
      buf += d
      let i
      while ((i = buf.indexOf('\n')) >= 0) {
        const m = JSON.parse(buf.slice(0, i))
        buf = buf.slice(i + 1)
        if (m.QMP) s.write(JSON.stringify({ execute: 'qmp_capabilities' }) + '\n')
        else if (m.event) continue
        else if (!negotiated) {
          negotiated = true
          s.write(JSON.stringify(cmd) + '\n')
        } else {
          s.end()
          if (m.error) reject(new Error(`${cmd.execute}: ${JSON.stringify(m.error)}`))
          else resolve(m.return)
        }
      }
    })
    s.on('error', reject)
  })
}

let dumps = 0
async function screendump(vm) {
  const file = sock(vm, `dump-${dumps++}.ppm`)
  await qmp(vm, { execute: 'screendump', arguments: { filename: file } })
  const b = fs.readFileSync(file)
  fs.unlinkSync(file)
  // P6\n<w> <h>\n255\n<rgb>
  const fields = []
  let at = 0
  while (fields.length < 4) {
    while (/\s/.test(String.fromCharCode(b[at]))) at++
    let s = at
    while (!/\s/.test(String.fromCharCode(b[at]))) at++
    fields.push(b.subarray(s, at).toString())
  }
  at++
  const [, w, h] = fields
  const rgb = b.subarray(at)
  let lit = 0
  for (let i = 0; i < rgb.length; i += 3) {
    if (rgb[i] > 16 || rgb[i + 1] > 16 || rgb[i + 2] > 16) lit++
  }
  return { w: +w, h: +h, rgb, lit: lit / (w * h) }
}

// Pixels that differ between two dumps. A blinking text cursor is a few
// hundred, so "settled" allows that much and "changed" asks for more.
function differ(a, b) {
  if (!a || !b || a.w !== b.w || a.h !== b.h) return Infinity
  let n = 0
  for (let i = 0; i < a.rgb.length; i += 3) {
    if (a.rgb[i] !== b.rgb[i] || a.rgb[i + 1] !== b.rgb[i + 1] || a.rgb[i + 2] !== b.rgb[i + 2]) n++
  }
  return n
}
const STILL = 400
const MOVED = 1000

// A coarse picture of a screen, for the log when something is not as expected.
function thumb(d, cols = 64) {
  const rows = Math.round((cols * d.h) / d.w / 2)
  const ramp = ' .:-=+*#%@'
  const out = []
  for (let r = 0; r < rows; r++) {
    let line = ''
    for (let c = 0; c < cols; c++) {
      const x = Math.floor(((c + 0.5) * d.w) / cols)
      const y = Math.floor(((r + 0.5) * d.h) / rows)
      const i = (y * d.w + x) * 3
      const l = (d.rgb[i] * 0.3 + d.rgb[i + 1] * 0.59 + d.rgb[i + 2] * 0.11) / 256
      line += ramp[Math.min(ramp.length - 1, Math.floor(l * ramp.length))]
    }
    out.push(`    |${line}|`)
  }
  return out.join('\n')
}

// Wait until two dumps `gap` ms apart are the same screen (and `ok` holds).
async function settle(vm, { gap = 2000, timeout = 60000, ok = () => true } = {}) {
  const end = Date.now() + timeout
  let prev = await screendump(vm)
  while (Date.now() < end) {
    await sleep(gap)
    const d = await screendump(vm)
    if (differ(d, prev) <= STILL && ok(d)) return d
    prev = d
  }
  return null
}

// ---- the pages -------------------------------------------------------------
// Counted in the page itself: every VNC socket's received bytes and messages,
// the time inside its message handler, and the FBURs it sent.
function instrument() {
  const s = (window.__rfb = { bytes: 0, msgs: 0, fbur: 0, ms: 0, times: [] })
  const Native = window.WebSocket
  const view = (d) =>
    d instanceof ArrayBuffer ? new Uint8Array(d) : ArrayBuffer.isView(d) ? new Uint8Array(d.buffer, d.byteOffset, d.byteLength) : null
  function wrap(fn) {
    if (typeof fn !== 'function') return fn
    return function (ev) {
      const t0 = performance.now()
      try {
        return fn.call(this, ev)
      } finally {
        const dt = performance.now() - t0
        s.ms += dt
        s.msgs++
        s.times.push(dt)
        s.bytes += ev.data?.byteLength ?? ev.data?.size ?? 0
      }
    }
  }
  // Patched on WebSocket.prototype, not subclassed: noVNC 1.7 checks a
  // channel's own and immediate-prototype properties for `close` and the
  // rest, so a subclass reads to it as a channel missing them.
  const P = Native.prototype
  const isVnc = (ws) => String(ws.url).includes('/vnc')
  const send = P.send
  P.send = function (d) {
    const b = isVnc(this) && view(d)
    if (b && b.length === 10 && b[0] === 3) s.fbur++
    return send.call(this, d)
  }
  const on = Object.getOwnPropertyDescriptor(P, 'onmessage')
  Object.defineProperty(P, 'onmessage', {
    configurable: true,
    enumerable: true,
    get() {
      return on.get.call(this)
    },
    set(fn) {
      on.set.call(this, isVnc(this) ? wrap(fn) : fn)
    },
  })
  const add = P.addEventListener
  P.addEventListener = function (t, fn, o) {
    return add.call(this, t, t === 'message' && isVnc(this) ? wrap(fn) : fn, o)
  }
}

const CANVAS = { storm: 'canvas.fb', novnc: 'div.fb canvas' }

async function open(browser, vm, client) {
  const ctx = await browser.newContext({ viewport: { width: 1600, height: 1100 } })
  await ctx.addInitScript(instrument)
  const page = await ctx.newPage()
  page.on('pageerror', (e) => errors.push(`${vm}/${client} pageerror: ${e.message.split('\n')[0]}`))
  page.on('console', (m) => {
    if (m.type() === 'error') errors.push(`${vm}/${client} console: ${m.text().split('\n')[0]}`)
  })
  await page.goto(`${BASE}/#/vm/default/${vm}?door=vnc&rfb=${client}`)
  await page.locator('nav.tabs button', { hasText: 'Graphical console' }).waitFor({ timeout: 20000 })
  // What the browser fetched to draw: everything loaded from the tab on.
  const fetched = []
  page.on('response', async (r) => {
    const u = r.url()
    if (/\.(js|wasm)(\?|$)/.test(u)) {
      try {
        fetched.push([u.split('/').pop(), (await r.body()).length])
      } catch {}
    }
  })
  // `?door=vnc` selects the tab; clicking it is what opens the console.
  await page.locator('nav.tabs button', { hasText: 'Graphical console' }).click()
  // If the tab's own open lost a race with the canvas binding, Connect is
  // the button a person would press next.
  const isOpen = () => page.locator('.bar .state.open').waitFor({ timeout: 8000 }).then(() => true, () => false)
  if (!(await isOpen())) {
    const connect = page.locator('button.sc-primary', { hasText: 'Connect' })
    if (await connect.count()) await connect.click()
    if (!(await isOpen()) && !(await isOpen())) {
      const state = await page.locator('.bar .state').innerText().catch(() => '(no state)')
      const shown = await page.locator('p.error').allInnerTexts().catch(() => [])
      const s = await page.evaluate(() => window.__rfb).catch(() => null)
      console.log(`  ${vm}/${client}: state "${state}", error ${JSON.stringify(shown)}, socket ${JSON.stringify(s && { bytes: s.bytes, msgs: s.msgs, fbur: s.fbur })}`)
      console.log(`  page errors: ${errors.join('; ') || 'none'}`)
      throw new Error(`${vm}/${client}: the console never opened`)
    }
  }
  await page.locator(CANVAS[client]).waitFor({ timeout: 10000 })
  await sleep(500)
  return { page, vm, client, fetched }
}

async function reset(p) {
  await p.page.evaluate(() => Object.assign(window.__rfb, { bytes: 0, msgs: 0, fbur: 0, ms: 0, times: [] }))
}
async function stats(p) {
  return p.page.evaluate(() => {
    const s = window.__rfb
    const t = [...s.times].sort((a, b) => a - b)
    const q = (f) => (t.length ? t[Math.min(t.length - 1, Math.floor(f * t.length))] : 0)
    return { bytes: s.bytes, msgs: s.msgs, fbur: s.fbur, ms: s.ms, p50: q(0.5), p95: q(0.95), max: t.length ? t[t.length - 1] : 0 }
  })
}
const table = []
async function phase(name, pages, fn) {
  for (const p of pages) await reset(p)
  const t0 = Date.now()
  const r = await fn()
  const secs = (Date.now() - t0) / 1000
  for (const p of pages) table.push({ phase: name, client: p.client, secs, ...(await stats(p)) })
  return r
}

// Pixel comparison of one client's canvas with a dump, in the page.
async function compare(p, d) {
  return p.page.evaluate(
    ({ sel, w, h, b64 }) => {
      const c = document.querySelector(sel)
      if (!c || !c.width) return { error: 'no canvas' }
      if (c.width !== w || c.height !== h) return { error: `canvas ${c.width}×${c.height}, guest ${w}×${h}` }
      const px = c.getContext('2d').getImageData(0, 0, w, h).data
      const rgb = Uint8Array.from(atob(b64), (ch) => ch.charCodeAt(0))
      let same = 0
      let worst = 0
      for (let i = 0, j = 0; j < rgb.length; i += 4, j += 3) {
        const dd = Math.max(Math.abs(px[i] - rgb[j]), Math.abs(px[i + 1] - rgb[j + 1]), Math.abs(px[i + 2] - rgb[j + 2]))
        if (dd === 0) same++
        if (dd > worst) worst = dd
      }
      return { same: same / (w * h), worst }
    },
    { sel: CANVAS[p.client], w: d.w, h: d.h, b64: Buffer.from(d.rgb).toString('base64') }
  )
}

// Legible: both clients equal to the guest's own framebuffer. A text
// cursor blinks, so a few hundred pixels may differ between the dump and a
// read of the canvas; retried, and the best of five is what is reported.
async function legible(label, vm, pages) {
  let best = {}
  let dump
  for (let attempt = 0; attempt < 5; attempt++) {
    await sleep(1500)
    dump = await screendump(vm)
    for (const p of pages) {
      const r = await compare(p, dump)
      if (!best[p.client] || (r.same ?? -1) > (best[p.client].same ?? -1)) best[p.client] = r
    }
    if (pages.every((p) => best[p.client].same === 1)) break
  }
  console.log(`  ${label}: guest ${dump.w}×${dump.h}, ${(dump.lit * 100).toFixed(1)}% lit`)
  for (const p of pages) {
    const r = best[p.client]
    check(
      !r.error && r.same >= 0.999,
      `${label}: ${p.client} draws the guest's framebuffer`,
      r.error || `${(r.same * 100).toFixed(3)}% of pixels exact, worst channel diff ${r.worst}`
    )
  }
  return dump
}

async function typeAt(p, text) {
  await p.page.locator(CANVAS.storm).focus()
  for (const ch of text) {
    if (ch === '\n') await p.page.keyboard.press('Enter')
    else await p.page.keyboard.type(ch)
    await sleep(40)
  }
}

;(async () => {
  const browser = await chromium.launch()

  // ---- Alpine ---------------------------------------------------------------
  console.log('\n--- alpine: attach both clients to the paused guest')
  const a = { storm: await open(browser, 'alpine', 'storm'), novnc: await open(browser, 'alpine', 'novnc') }
  const ap = [a.storm, a.novnc]
  check(true, 'alpine: both clients connected through the relay')
  await phase('alpine boot', ap, async () => {
    await qmp('alpine', { execute: 'cont' })
    const end = Date.now() + 180000
    while (Date.now() < end) {
      const log = fs.existsSync(sock('alpine', 'serial.log')) ? fs.readFileSync(sock('alpine', 'serial.log'), 'utf8') : ''
      if (/login:/.test(log)) break
      await sleep(1000)
    }
    return settle('alpine', { timeout: 60000, ok: (d) => d.lit > 0.001 })
  })
  const booted = await legible('alpine at its login prompt', 'alpine', ap)
  console.log(thumb(booted))

  // Click into the canvas (focus, pointer through the relay), log in, and
  // scroll a lot of text: the activity is the measurement.
  await a.storm.page.locator(CANVAS.storm).click({ position: { x: 10, y: 10 } })
  await typeAt(a.storm, 'root\n')
  await sleep(3000)
  const loggedIn = await screendump('alpine')
  check(differ(loggedIn, booted) > MOVED, 'alpine: typing at stormrfb changed the guest screen (login)', `${differ(loggedIn, booted)} pixels changed`)
  await phase('alpine scrolling ls -lR', ap, async () => {
    await typeAt(a.storm, 'ls -lR /lib /usr /etc /bin /sbin\n')
    return settle('alpine', { gap: 2500, timeout: 120000 })
  })
  await legible('alpine after scrolling', 'alpine', ap)
  await typeAt(a.storm, 'clear\n')
  const cleared = await settle('alpine', { timeout: 20000 })
  check(cleared && cleared.lit < 0.01, 'alpine: `clear` typed at stormrfb cleared the screen', cleared ? `${(cleared.lit * 100).toFixed(2)}% lit` : 'never settled')
  await legible('alpine cleared', 'alpine', ap)

  // ---- Windows Server 2022 Setup --------------------------------------------
  console.log('\n--- windows: attach both clients to the paused installer')
  const w = { storm: await open(browser, 'windows', 'storm'), novnc: await open(browser, 'windows', 'novnc') }
  const wp = [w.storm, w.novnc]
  check(true, 'windows: both clients connected through the relay')
  // The ISO boots only if a key is pressed at "Press any key to boot from
  // CD or DVD": pressed at stormrfb, so reaching Setup proves the key did.
  await w.storm.page.locator(CANVAS.storm).click({ position: { x: 10, y: 10 } })
  const setup = await phase('windows boot to Setup', wp, async () => {
    await qmp('windows', { execute: 'cont' })
    for (let i = 0; i < 30; i++) {
      await w.storm.page.keyboard.press('Enter')
      await sleep(700)
    }
    return settle('windows', { gap: 3000, timeout: 420000, ok: (d) => d.lit > 0.5 })
  })
  if (!check(!!setup, 'windows: the installer reached Setup (a full screen, settled)')) {
    console.log(thumb(await screendump('windows')))
  } else {
    const first = await legible('windows Setup, first page', 'windows', wp)
    console.log(thumb(first))
    // Next, the way a person without a mouse would press it: Enter (the
    // default button), then its mnemonic Alt+N, then Tab to it and Enter.
    // Setup can draw before it takes input, so it is given a while first.
    // If stormrfb's keys do nothing, the same keys through noVNC and then
    // through QMP (no VNC at all) say whether it is the client or the guest.
    let how = null
    let other = null
    await sleep(20000)
    const moved = async () => differ(await screendump('windows'), first) > MOVED
    const tries = [['Enter'], ['Alt+n'], ['Tab', 'Tab', 'Tab', 'Enter']]
    await phase('windows Setup: keys to the next page', wp, async () => {
      for (const keys of tries) {
        await w.storm.page.locator(CANVAS.storm).focus()
        for (const k of keys) {
          await w.storm.page.keyboard.press(k)
          await sleep(300)
        }
        await sleep(5000)
        if (await moved()) {
          how = keys.join(' ')
          break
        }
      }
      return settle('windows', { gap: 2000, timeout: 60000, ok: (d) => d.lit > 0.5 })
    })
    if (!how) {
      console.log(thumb(await screendump('windows'), 120))
      await w.novnc.page.locator(CANVAS.novnc).click({ position: { x: 5, y: 5 } })
      for (const keys of tries) {
        for (const k of keys) {
          await w.novnc.page.keyboard.press(k)
          await sleep(300)
        }
        await sleep(5000)
        if (await moved()) {
          other = `noVNC: ${keys.join(' ')}`
          break
        }
      }
      if (!other) {
        await qmp('windows', { execute: 'send-key', arguments: { keys: [{ type: 'qcode', data: 'ret' }] } })
        await sleep(5000)
        other = (await moved()) ? 'QMP send-key ret' : 'nothing: not noVNC, not QMP send-key'
      }
      console.log(`  windows: what moved Setup instead: ${other}`)
    }
    const next = await legible('windows Setup, after the keys', 'windows', wp)
    console.log(thumb(next))
    check(!!how, 'windows: keys typed at stormrfb moved Setup to its next page', how ? `${how}: ${differ(next, first)} pixels changed` : 'no key changed the page')
  }

  // ---- what each client fetched, and the numbers ------------------------------
  console.log('\n--- what the browser fetched to draw (from the tab on, alpine pages)')
  for (const p of ap) {
    const total = p.fetched.reduce((n, [, b]) => n + b, 0)
    console.log(`  ${p.client.padEnd(6)} ${total} B: ${p.fetched.map(([f, b]) => `${f} ${b}`).join(', ') || '(nothing new)'}`)
  }
  console.log('\n--- per phase, per client (handler = the socket message handler: decode + paint)')
  console.log('  phase                                   client  secs   bytes      msgs  FBURs  B/FBUR   handler ms  ms/FBUR  p50 ms  p95 ms  max ms')
  for (const r of table) {
    const per = r.fbur ? r.bytes / r.fbur : 0
    console.log(
      `  ${r.phase.padEnd(40)}${r.client.padEnd(8)}${r.secs.toFixed(1).padStart(5)} ${String(r.bytes).padStart(10)} ${String(r.msgs).padStart(8)} ${String(r.fbur).padStart(6)} ${per.toFixed(0).padStart(7)} ${r.ms.toFixed(1).padStart(12)} ${(r.fbur ? r.ms / r.fbur : 0).toFixed(3).padStart(8)} ${r.p50.toFixed(3).padStart(7)} ${r.p95.toFixed(3).padStart(7)} ${r.max.toFixed(2).padStart(7)}`
    )
  }
  console.log('RESULTS_JSON ' + JSON.stringify(table))

  check(!errors.length, 'no page errors', errors.join('; '))
  await browser.close()
  console.log(`\n${failed} failed`)
  process.exit(failed ? 1 : 0)
})().catch((e) => {
  console.error(e)
  process.exit(2)
})
