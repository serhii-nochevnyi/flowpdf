import { existsSync } from 'node:fs'
import { resolve } from 'node:path'
import { spawn } from 'node:child_process'

const projectRoot = resolve(import.meta.dirname, '..')
const port = Number.parseInt(process.env.FLOWPDF_IME_PORT ?? '4187', 10)
const url = `http://127.0.0.1:${port}/`
const browserPath = resolve(projectRoot, 'work/playwright')
if (existsSync(browserPath) && process.env.PLAYWRIGHT_BROWSERS_PATH === undefined) {
  process.env.PLAYWRIGHT_BROWSERS_PATH = browserPath
}

if (!existsSync(resolve(projectRoot, 'web/generated/flow_wasm.js'))) {
  throw new Error('FLOW_IME_VERIFICATION_REQUIRES_NPM_BUILD_WASM')
}

const { chromium } = await import('playwright')
const server = spawn(process.execPath, [
  'node_modules/vite/bin/vite.js',
  '--host',
  '127.0.0.1',
  '--port',
  String(port),
  '--strictPort',
], {
  cwd: projectRoot,
  env: {
    ...process.env,
  },
  stdio: ['ignore', 'pipe', 'pipe'],
})

let browser
try {
  await waitForServer(url)
  browser = await chromium.launch({
    headless: true,
    args: ['--single-process'],
  })
  const context = await browser.newContext()
  const page = await context.newPage()
  await page.route(`${url}src/main.js`, (route) => route.abort())
  await page.goto(url)
  await page.addScriptTag({ type: 'module', url: `${url}src/main.tsx` })
  await page.locator('[data-action="editor-open-older"]').click()
  await waitForRevision(page, 1)

  const baselineText = await page.locator('[data-editor-document] p').first().textContent()
  const input = page.locator('[data-editor-input-host]')
  await input.focus()
  const cdp = await context.newCDPSession(page)
  await cdp.send('Input.imeSetComposition', {
    text: 'ї',
    selectionStart: 1,
    selectionEnd: 1,
  })
  await cdp.send('Input.insertText', { text: 'ї' })
  await waitForRevision(page, 2)

  const composedText = await page.locator('[data-editor-document] p').first().textContent()
  if (composedText === null || !composedText.includes('ї')) {
    throw new Error('FLOW_IME_COMMITTED_TEXT_MISSING')
  }

  await page.locator('[data-action="editor-undo"]').click()
  await waitForRevision(page, 3)
  const undoneText = await page.locator('[data-editor-document] p').first().textContent()
  if (undoneText !== baselineText) throw new Error('FLOW_IME_UNDO_WAS_NOT_ATOMIC')

  process.stdout.write('Chromium CDP IME verification passed: one composition, one undo.\n')
} finally {
  await browser?.close()
  server.kill('SIGTERM')
}

async function waitForServer(target) {
  const deadline = Date.now() + 10_000
  while (Date.now() < deadline) {
    try {
      const response = await fetch(target)
      if (response.ok) return
    } catch {
      // The static server is still starting.
    }
    await new Promise((resolve) => setTimeout(resolve, 50))
  }
  throw new Error('FLOW_IME_SERVER_TIMEOUT')
}

async function waitForRevision(page, expected) {
  await page.waitForFunction(
    (revision) => document.querySelector('[data-editor-revision]')?.textContent === String(revision),
    expected,
    { timeout: 10_000 },
  )
}
