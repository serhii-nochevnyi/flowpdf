import { createReadStream } from 'node:fs'
import { stat } from 'node:fs/promises'
import { createServer } from 'node:http'
import { extname, resolve, sep } from 'node:path'

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

const server = createServer(async (request, response) => {
  if (request.method !== 'GET' && request.method !== 'HEAD') {
    response.writeHead(405, { Allow: 'GET, HEAD' }).end()
    return
  }

  try {
    const requestUrl = new URL(request.url ?? '/', `http://${host}:${port}`)
    const pathname = requestUrl.pathname === '/' ? '/index.html' : requestUrl.pathname
    const candidate = resolve(root, `.${decodeURIComponent(pathname)}`)
    if (candidate !== root && !candidate.startsWith(`${root}${sep}`)) {
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
    createReadStream(candidate).pipe(response)
  } catch {
    response.writeHead(404).end()
  }
})

server.listen(port, host, () => {
  process.stdout.write(`FlowPDF Foundation Inspector: http://${host}:${port}\n`)
})
