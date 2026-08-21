import assert from 'node:assert/strict'
import { mkdtemp, writeFile } from 'node:fs/promises'
import { createServer, get } from 'node:http'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { Readable } from 'node:stream'
import test from 'node:test'

import { createInspectorRequestHandler } from './serve-inspector.mjs'

test('a read failure after headers does not escape the request handler', async () => {
  const root = await mkdtemp(join(tmpdir(), 'flowpdf-inspector-'))
  await writeFile(join(root, 'index.html'), 'fixture')
  const handler = createInspectorRequestHandler({
    documentRoot: root,
    streamFile: () => new Readable({
      read() {
        this.destroy(new Error('injected read failure'))
      },
    }),
  })
  const server = createServer(handler)
  await new Promise((resolve, reject) => {
    server.once('error', reject)
    server.listen(0, '127.0.0.1', resolve)
  })

  try {
    const address = server.address()
    assert.notEqual(address, null)
    const port = typeof address === 'object' ? address.port : 0
    await new Promise((resolve) => {
      const request = get(`http://127.0.0.1:${port}/`, (response) => {
        response.on('aborted', resolve)
        response.on('error', resolve)
        response.on('end', resolve)
        response.resume()
      })
      request.on('error', resolve)
    })
    assert.equal(server.listening, true)
  } finally {
    await new Promise((resolve, reject) => server.close((error) => error ? reject(error) : resolve()))
  }
})
