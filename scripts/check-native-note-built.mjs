import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { once } from 'node:events'
import { createServer } from 'node:http'
import { resolve } from 'node:path'

import { chromium } from 'playwright'

import { createInspectorRequestHandler } from './serve-inspector.mjs'

function ordinaryPdf() {
  const objects = [
    '<< /Type /Catalog /Pages 2 0 R >>',
    '<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
    '<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>',
  ]
  let pdf = '%PDF-1.7\n'
  const offsets = objects.map((object, index) => {
    const offset = pdf.length
    pdf += `${index + 1} 0 obj\n${object}\nendobj\n`
    return offset
  })
  const xref = pdf.length
  pdf += 'xref\n0 4\n0000000000 65535 f \n'
  for (const offset of offsets) pdf += `${String(offset).padStart(10, '0')} 00000 n \n`
  pdf += `trailer\n<< /Size 4 /Root 1 0 R >>\nstartxref\n${xref}\n%%EOF\n`
  return Buffer.from(pdf)
}

const server = createServer(createInspectorRequestHandler({ documentRoot: resolve(import.meta.dirname, '../dist/web') }))
server.listen(0, '127.0.0.1')
await once(server, 'listening')
let browser
try {
  browser = await chromium.launch()
  const page = await browser.newPage()
  const origin = `http://127.0.0.1:${server.address().port}`
  const workers = []
  page.on('worker', (worker) => workers.push(worker.url()))

  await page.goto(`${origin}/editor.html`)
  const panel = page.locator('[data-pdf-native-edit]')
  const bytes = ordinaryPdf()
  await panel.locator('input[type=file]').setInputFiles({
    name: 'contract.pdf', mimeType: 'application/pdf', buffer: bytes,
  }, { timeout: 10_000 })
  const verifiedOriginal = panel.locator('[data-origin-hash]:enabled')
  await verifiedOriginal.waitFor({ timeout: 10_000 })
  assert.equal(await verifiedOriginal.getAttribute('data-origin-hash'), createHash('sha256').update(bytes).digest('hex'))
  assert.ok(workers.some((url) => url.startsWith(`${origin}/assets/pdf-native-edit-worker-entry-`) && url.endsWith('.js')))
  assert.ok(workers.every((url) => url.startsWith(`${origin}/assets/`) && url.endsWith('.js')))
  process.stdout.write('Built editor accepted the PDF through its bundled WASM worker.\n')
} finally {
  try {
    await browser?.close()
  } finally {
    await new Promise((resolveClose) => server.close(resolveClose))
  }
}
