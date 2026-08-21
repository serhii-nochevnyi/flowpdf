import { cp, mkdir, rm } from 'node:fs/promises'
import { resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const projectRoot = resolve(import.meta.dirname, '..')

export async function cleanWebOutput(root) {
  const output = resolve(root, 'dist/web')
  await rm(output, { recursive: true, force: true })
  await mkdir(output, { recursive: true })
}

export async function copyWebAssets(root) {
  const output = resolve(root, 'dist/web')
  await mkdir(output, { recursive: true })
  await Promise.all([
    cp(resolve(root, 'web/index.html'), resolve(output, 'index.html')),
    cp(resolve(root, 'web/src/styles.css'), resolve(output, 'styles.css')),
    copyGeneratedWasm(root, output),
  ])
}

async function copyGeneratedWasm(root, output) {
  const destination = resolve(output, 'generated')
  await rm(destination, { recursive: true, force: true })
  await cp(resolve(root, 'web/generated'), destination, { recursive: true })
}

if (process.argv[1] !== undefined && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url))) {
  const mode = process.argv[2]
  if (mode === '--clean') {
    await cleanWebOutput(projectRoot)
  } else if (mode === '--copy-assets') {
    await copyWebAssets(projectRoot)
  } else {
    throw new Error('usage: build-web.mjs --clean | --copy-assets')
  }
}
