import {
  createWasmLayoutEngine,
  installLayoutWorker,
  type LayoutWorkerScope,
} from './layout-worker.js'
import init, {
  layout_document,
  verify_layout_response,
} from '../../generated/flow_wasm.js'

const scope = globalThis as unknown as LayoutWorkerScope
let installedHandler: LayoutWorkerScope['onmessage'] = null
let startupCode: string | null = null

const ready = init()
  .then(() => {
    const wasm = { layout_document, verify_layout_response }
    const adapter = createWasmLayoutEngine(wasm)
    installLayoutWorker(scope, adapter.run, (result) =>
      result.resultHash.trim().length > 0,
    )
    installedHandler = scope.onmessage
  })
  .catch(() => {
    startupCode = 'FLOW_LAYOUT_WORKER_STARTUP'
  })

scope.onmessage = (event) => {
  void ready.then(() => {
    if (installedHandler !== null) {
      installedHandler(event)
    } else if (event.data.type !== 'cancel') {
      scope.postMessage({
        type: 'failed',
        requestId: event.data.request.requestId,
        code: startupCode ?? 'FLOW_LAYOUT_WORKER_STARTUP',
      })
    }
  })
}
