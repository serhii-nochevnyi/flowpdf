import init, * as generated from '../../generated/flow_wasm.js'
import { installNativePdfWorker, type NativePdfWorkerScope } from './pdf-native-edit-worker.js'
import type { NativePdfWasmBoundary } from './pdf-native-edit-protocol.js'

const scope = globalThis as unknown as NativePdfWorkerScope
const ready = Promise.resolve().then(async () => {
  await init()
  const wasm = generated as Readonly<Partial<NativePdfWasmBoundary>>
  const {
    native_pdf_open, native_pdf_command, native_pdf_snapshot, native_pdf_close, native_pdf_redaction,
    native_pdf_preview, verify_native_pdf_response,
  } = wasm
  if (typeof native_pdf_open !== 'function' || typeof native_pdf_command !== 'function'
    || typeof native_pdf_snapshot !== 'function' || typeof native_pdf_close !== 'function'
    || typeof native_pdf_redaction !== 'function'
    || typeof verify_native_pdf_response !== 'function') {
    throw new Error('FLOW_PDF_NATIVE_WORKER_STARTUP')
  }
  return {
    native_pdf_open, native_pdf_command, native_pdf_snapshot, native_pdf_close, native_pdf_redaction,
    ...(typeof native_pdf_preview === 'function' ? { native_pdf_preview } : {}),
    verify_native_pdf_response,
  }
})

installNativePdfWorker(scope, ready)
