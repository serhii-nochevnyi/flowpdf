import { loadBundledFontCatalog } from './runtime/font-catalog.js'
import { createDefaultWorkerRuntime } from './layout/layout-runtime.js'
import { mountEditorApp, type EditorLocale } from './editor/editor-app.js'
import { loadWasm } from './editor/editor-controller.js'

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
  try {
    const wasm = await loadWasm()
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
    const runtime = createDefaultWorkerRuntime(fontCatalog)
    window.addEventListener('beforeunload', () => runtime.dispose(), { once: true })
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
      },
    )
  } catch (error: unknown) {
    const message = error instanceof Error ? error.message : 'FLOW_EDITOR_STARTUP'
    const alert = document.createElement('p')
    alert.className = 'startup-error'
    alert.setAttribute('role', 'alert')
    alert.textContent =
      locale === 'uk'
        ? `Редактор не запущено. Код: ${message}`
        : `The editor could not start. Code: ${message}`
    appRoot.replaceChildren(alert)
  }
}

void start()
