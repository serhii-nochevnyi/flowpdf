import {
  createWasmPdfReconstructionEngine,
  installPdfReconstructionWorker,
  type PdfReconstructionWorkerScope,
} from './pdf-reconstruction-worker.js'
import init, {
  accept_pdf_reconstruction,
  reconstruct_pdf,
  verify_pdf_reconstruction_accept_response,
  verify_pdf_reconstruction_response,
} from '../../generated/flow_wasm.js'

const scope = globalThis as unknown as PdfReconstructionWorkerScope
let installedHandler: PdfReconstructionWorkerScope['onmessage'] = null
let startupCode: string | null = null

const ready = init()
  .then(() => {
    const adapter = createWasmPdfReconstructionEngine({
      reconstruct_pdf,
      verify_pdf_reconstruction_response,
      accept_pdf_reconstruction,
      verify_pdf_reconstruction_accept_response,
    })
    installPdfReconstructionWorker(scope, adapter)
    installedHandler = scope.onmessage
  })
  .catch(() => {
    startupCode = 'FLOW_PDF_RECONSTRUCTION_WORKER_STARTUP'
  })

scope.onmessage = (event) => {
  void ready.then(() => {
    if (installedHandler !== null) {
      installedHandler(event)
    } else if (event.data.type !== 'cancel') {
      scope.postMessage({
        type: 'failed',
        requestId:
          event.data.type === 'reconstruct' ? event.data.request.requestId : event.data.requestId,
        code: startupCode ?? 'FLOW_PDF_RECONSTRUCTION_WORKER_STARTUP',
      })
    }
  })
}
