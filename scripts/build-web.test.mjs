import assert from 'node:assert/strict'
import { mkdtemp, mkdir, readFile, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import test from 'node:test'

import { cleanWebOutput } from './build-web.mjs'

test('cleanWebOutput removes stale compiler output before the next emit', async () => {
  const root = await mkdtemp(join(tmpdir(), 'flowpdf-web-build-'))
  const output = join(root, 'dist/web')
  await mkdir(output, { recursive: true })
  await writeFile(join(output, 'deleted-module.js'), 'stale')

  await cleanWebOutput(root)

  await assert.rejects(readFile(join(output, 'deleted-module.js')), { code: 'ENOENT' })
})
