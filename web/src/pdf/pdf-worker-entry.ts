import {
  createWasmPdfExportEngine,
  installPdfWorker,
  type PdfWorkerScope,
} from './pdf-worker.js'
import init, {
  export_pdf,
  recover_owned_source,
  verify_pdf_export_response,
} from '../../generated/flow_wasm.js'

const scope = globalThis as unknown as PdfWorkerScope
let installedHandler: PdfWorkerScope['onmessage'] = null
let startupCode: string | null = null

const ready = init()
  .then(() => {
    const adapter = createWasmPdfExportEngine({
      export_pdf,
      verify_pdf_export_response,
      recover_owned_source,
    })
    installPdfWorker(scope, adapter.run, adapter.verifyResultHash)
    installedHandler = scope.onmessage
  })
  .catch(() => {
    startupCode = 'FLOW_PDF_WORKER_STARTUP'
  })

scope.onmessage = (event) => {
  void ready.then(() => {
    if (installedHandler !== null) {
      installedHandler(event)
    } else if (event.data.type !== 'cancel') {
      scope.postMessage({
        type: 'failed',
        requestId: event.data.request.requestId,
        code: startupCode ?? 'FLOW_PDF_WORKER_STARTUP',
      })
    }
  })
}
