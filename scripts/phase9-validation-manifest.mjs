import { lstatSync } from 'node:fs'
import { resolve } from 'node:path'

export const wasmPrerequisite = Object.freeze({
  buildTaskId: '09-60-01',
  fixtureTaskId: '09-16-02',
  outputs: Object.freeze(['flow_wasm.js', 'flow_wasm.d.ts', 'flow_wasm_bg.wasm']),
})

export function hasGeneratedWasm(cwd) {
  return wasmPrerequisite.outputs.every((name) => {
    const file = lstatSync(resolve(cwd, 'web/generated', name), { throwIfNoEntry: false })
    return file?.isFile() && file.size > 0
  })
}

const cargo = (crate, ...target) => ['cargo', 'test', '--offline', '--locked', '-p', crate, ...target, '--', '--nocapture']
const core = (...targets) => cargo('flow-core', ...targets.flatMap((target) => ['--test', target]))
const contract = (...names) => ['node', '--test', ...names.map((name) => `tests/contracts/${name}.test.mjs`)]
const unit = (name) => ['npm', 'run', 'test:unit', '--', `web/tests/pdf-native-${name}.test.ts`]
const browser = (name) => ['npm', 'run', 'test:browser', '--',
  `web/tests/pdf-native-${name}.browser.test.ts`, '--no-cache', '--configLoader', 'runner', '--browser.screenshotFailures=false']
const requirements = (...ids) => ids.map((id) => `NPDF-0${id}`)
const all = requirements(1, 2, 3, 4, 5)

function task(id, lane, argv, requirementIds, aliases = [], timeout = 300_000) {
  return Object.freeze({ id, lane, argv: Object.freeze(argv), requirements: Object.freeze(requirementIds),
    covers: Object.freeze([id, ...aliases]), timeout })
}

export const tasks = Object.freeze([
  task(wasmPrerequisite.buildTaskId, 'local', ['npm', 'run', 'build:wasm'], requirements(1, 5), [], 1_200_000),
  task('09-77-01', 'local', ['node', '--test', 'scripts/verify-wasm-bindgen-tool.test.mjs'], requirements(1), ['09-27-01']),
  task('09-01-01', 'local', cargo('flow-core', '--lib', 'pdf::native'), requirements(1), ['09-01-02']),
  task('09-18-01', 'local', core('pdf_native_graph', 'pdf_native_session', 'pdf_reader', 'pdf_scene', 'pdf_cos'),
    requirements(1), ['09-18-02']),
  task('09-02-01', 'local', cargo('flow-wasm', '--test', 'pdf_native_edit'), all,
    ['09-04-02', '09-06-02', '09-08-02', '09-10-02', '09-14-01']),
  task('09-02-02', 'local', unit('edit'), all, ['09-14-02']),
  task('09-02-03', 'local', ['npm', 'run', 'typecheck'], all, ['09-03-01']),
  task('09-19-01', 'local', unit('edit-panel'), all,
    ['09-19-02', '09-05-01', '09-07-01', '09-09-01', '09-11-01', '09-15-01']),
  task('09-03-03', 'local', contract('native-note-tracer-gate'), requirements(1)),
  task('09-04-01', 'local', core('pdf_native_text'), requirements(2)),
  task('09-06-01', 'local', core('pdf_native_objects'), requirements(3)),
  task('09-08-01', 'local', core('pdf_native_pages', 'pdf_native_forms'), requirements(1, 4), ['09-10-01']),
  task('09-12-01', 'local', core('pdf_native_verifier', 'pdf_native_redaction'), requirements(5),
    ['09-12-02', '09-13-01', '09-13-02']),
  task('09-16-01', 'local', ['node', 'scripts/generate-pdf-native-fixtures.mjs', '--check'], all, ['09-20-01']),
  task(wasmPrerequisite.fixtureTaskId, 'local', contract('pdf-native-fixtures'), all, ['09-20-02']),
  task('09-17-01', 'smoke', contract('pdf-native-external', 'phase9-gate'), all),
  task('09-03-02', 'browser', ['node', 'scripts/check-native-note-tracer.mjs', '--run'], requirements(1), [], 2_400_000),
  task('09-05-02', 'browser', ['npx', '--no-install', 'vitest', 'run', '--project', 'browser',
    'web/tests/pdf-native-text.browser.test.ts'], requirements(2)),
  task('09-07-02', 'browser', browser('objects'), requirements(3)),
  task('09-09-02', 'browser', browser('pages'), requirements(4)),
  task('09-11-02', 'browser', browser('forms'), requirements(1, 4)),
  task('09-15-02', 'browser', browser('redaction'), requirements(5)),
  task('09-17-02', 'release', ['node', 'scripts/verify-pdf-native-external.mjs', '--strict-external'], all, [], 2_400_000),
  task('09-17-03', 'terminal', ['npm', 'run', 'check:phase9'], all),
])

export function assertManifest(inventory = tasks) {
  if (JSON.stringify(inventory) !== JSON.stringify(tasks)) throw new Error('PHASE9_MANIFEST_MISMATCH')
  const ids = inventory.flatMap(({ covers }) => covers)
  const commands = inventory.map(({ argv }) => JSON.stringify(argv))
  if (new Set(commands).size !== commands.length || new Set(ids).size !== ids.length || inventory.filter(({ lane }) => lane === 'terminal').length !== 1 ||
      inventory.at(-1).lane !== 'terminal' || inventory.some(({ argv }) => !argv.length)) {
    throw new Error('PHASE9_MANIFEST_INVALID')
  }
}
