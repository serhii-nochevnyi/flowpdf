import { createReadStream } from 'node:fs'
import { stat } from 'node:fs/promises'
import { createServer } from 'node:http'
import { extname, resolve, sep } from 'node:path'
import { pipeline } from 'node:stream/promises'
import { fileURLToPath } from 'node:url'

const host = '127.0.0.1'
const port = Number.parseInt(process.env.FLOWPDF_INSPECTOR_PORT ?? '4173', 10)
const root = resolve(import.meta.dirname, '../dist/web')
const contentTypes = new Map([
  ['.css', 'text/css; charset=utf-8'],
  ['.html', 'text/html; charset=utf-8'],
  ['.js', 'text/javascript; charset=utf-8'],
  ['.map', 'application/json; charset=utf-8'],
  ['.wasm', 'application/wasm'],
])

if (!Number.isSafeInteger(port) || port < 1 || port > 65_535) {
  throw new Error('FLOW_INVALID_INSPECTOR_PORT')
}

export function createInspectorRequestHandler({
  documentRoot = root,
  streamFile = createReadStream,
} = {}) {
  return async (request, response) => {
    if (request.method !== 'GET' && request.method !== 'HEAD') {
      response.writeHead(405, { Allow: 'GET, HEAD' }).end()
      return
    }

    try {
      const requestUrl = new URL(request.url ?? '/', `http://${host}:${port}`)
      const pathname = requestUrl.pathname === '/' ? '/index.html' : requestUrl.pathname
      const candidate = resolve(documentRoot, `.${decodeURIComponent(pathname)}`)
      if (candidate !== documentRoot && !candidate.startsWith(`${documentRoot}${sep}`)) {
        response.writeHead(404).end()
        return
      }
      const file = await stat(candidate)
      if (!file.isFile()) {
        response.writeHead(404).end()
        return
      }
      response.writeHead(200, {
        'Cache-Control': 'no-store',
        'Content-Length': String(file.size),
        'Content-Type': contentTypes.get(extname(candidate)) ?? 'application/octet-stream',
        'X-Content-Type-Options': 'nosniff',
      })
      if (request.method === 'HEAD') {
        response.end()
        return
      }
      await pipeline(streamFile(candidate), response)
    } catch {
      if (!response.headersSent) {
        response.writeHead(404).end()
      } else if (!response.destroyed) {
        response.destroy()
      }
    }
  }
}

if (process.argv[1] !== undefined && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url))) {
  const server = createServer(createInspectorRequestHandler())
  server.listen(port, host, () => {
    process.stdout.write(`FlowPDF Foundation Inspector: http://${host}:${port}\n`)
  })
}
