import { loadBundledFontCatalog } from './runtime/font-catalog.js'
import { createDefaultWorkerRuntime } from './layout/layout-runtime.js'
import { mountEditorApp, type EditorLocale } from './editor/editor-app.js'
import init, * as wasm from '../generated/flow_wasm.js'

const locale: EditorLocale = document.documentElement.lang.toLowerCase().startsWith('en')
  ? 'en'
  : 'uk'
const root = document.querySelector<HTMLElement>('#app')

if (root === null) {
  throw new Error('FLOW_EDITOR_ROOT_MISSING')
}
const appRoot = root

document.title =
  locale === 'uk'
    ? 'FlowPDF — Семантичний редактор'
    : 'FlowPDF — Semantic editor'

async function start(): Promise<void> {
  const lifecycle = new AbortController()
  let runtime: ReturnType<typeof createDefaultWorkerRuntime> | undefined
  function dispose() {
    lifecycle.abort()
    runtime?.dispose()
    runtime = undefined
  }
  try {
    await init()
    if (
      wasm.font_catalog_identity === undefined ||
      wasm.hyphenation_data_identity === undefined
    ) {
      throw new Error('FLOW_FONT_CATALOG_IDENTITY_UNAVAILABLE')
    }
    const fontCatalog = await loadBundledFontCatalog({
      font_catalog_identity: wasm.font_catalog_identity,
      hyphenation_data_identity: wasm.hyphenation_data_identity,
    })
    runtime = createDefaultWorkerRuntime(fontCatalog)
    window.addEventListener('pagehide', (event) => {
      if (!event.persisted) dispose()
    }, { signal: lifecycle.signal })
    appRoot.replaceChildren()
    await mountEditorApp(
      appRoot,
      { locale },
      {
        wasm: Promise.resolve(wasm),
        layoutScheduler: runtime.layoutScheduler,
        layoutRequestFactory: runtime.layoutRequestFactory,
        pdfExportScheduler: runtime.pdfExportScheduler,
        pdfExportRequestFactory: runtime.pdfExportRequestFactory,
        pdfReaderScheduler: runtime.pdfReaderScheduler,
        pdfReconstructionScheduler: runtime.pdfReconstructionScheduler,
      },
      undefined,
      {
        nativePdf: runtime.nativePdf,
        pdfReconstructionReaderScheduler: runtime.pdfReconstructionReaderScheduler,
        signal: lifecycle.signal,
      },
    )
  } catch (error: unknown) {
    dispose()
    const message = error instanceof Error && /^FLOW_[A-Z_]{1,80}$/.test(error.message)
      ? error.message
      : 'FLOW_EDITOR_STARTUP'
    const alert = document.createElement('p')
    alert.className = 'startup-error'
    alert.setAttribute('role', 'alert')
    alert.textContent =
      locale === 'uk'
        ? `Редактор не запущено. Код: ${message}`
        : `The editor could not start. Code: ${message}`
    const retry = document.createElement('button')
    retry.type = 'button'
    retry.textContent = locale === 'uk' ? 'Спробувати ще раз' : 'Retry startup'
    retry.addEventListener('click', () => {
      retry.disabled = true
      void start()
    }, { once: true })
    appRoot.replaceChildren(alert, retry)
  }
}

void start()
