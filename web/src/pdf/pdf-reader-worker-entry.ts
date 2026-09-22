import {
  createWasmPdfReaderEngine,
  installPdfReaderWorker,
  type PdfReaderWorkerScope,
} from './pdf-reader-worker.js'
import init, { read_pdf, verify_pdf_reader_response } from '../../generated/flow_wasm.js'

const scope = globalThis as unknown as PdfReaderWorkerScope
let installedHandler: PdfReaderWorkerScope['onmessage'] = null
let startupCode: string | null = null

const ready = init()
  .then(() => {
    const adapter = createWasmPdfReaderEngine({ read_pdf, verify_pdf_reader_response })
    installPdfReaderWorker(scope, adapter.run, adapter.verifyResultHash)
    installedHandler = scope.onmessage
  })
  .catch(() => {
    startupCode = 'FLOW_PDF_READER_WORKER_STARTUP'
  })

scope.onmessage = (event) => {
  void ready.then(() => {
    if (installedHandler !== null) {
      installedHandler(event)
    } else if (event.data.type !== 'cancel') {
      scope.postMessage({
        type: 'failed',
        requestId: event.data.request.requestId,
        code: startupCode ?? 'FLOW_PDF_READER_WORKER_STARTUP',
      })
    }
  })
}
