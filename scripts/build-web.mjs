import { cp, mkdir, rm } from 'node:fs/promises'
import { resolve } from 'node:path'

const projectRoot = resolve(import.meta.dirname, '..')
const outputRoot = resolve(projectRoot, 'dist/web')

await mkdir(outputRoot, { recursive: true })
await Promise.all([
  cp(resolve(projectRoot, 'web/index.html'), resolve(outputRoot, 'index.html')),
  cp(resolve(projectRoot, 'web/src/styles.css'), resolve(outputRoot, 'styles.css')),
  copyGeneratedWasm(projectRoot, outputRoot),
])

async function copyGeneratedWasm(root, output) {
  const destination = resolve(output, 'generated')
  await rm(destination, { recursive: true, force: true })
  await cp(resolve(root, 'web/generated'), destination, { recursive: true })
}
