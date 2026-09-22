import assert from 'node:assert/strict'
import test from 'node:test'

import {
  DEFAULT_STEP_TIMEOUT_MS,
  MAX_CAPTURE_BYTES,
  redactOutput,
  runBoundedStep,
  tailOutput,
  writeFailureDetails,
} from './phase-gate-runner.mjs'

test('bounded runner keeps the safe four-field diagnostic and passes timeout/output limits', () => {
  let options
  const { diagnostic, details } = runBoundedStep(
    { id: 'test-pass', command: 'node', args: ['-e', ''], cwd: '/tmp', env: {}, timeoutMs: 42 },
    {
      spawn: (_command, _args, receivedOptions) => {
        options = receivedOptions
        return { status: 0, signal: null, stdout: 'ok', stderr: '' }
      },
    },
  )

  assert.deepEqual(Object.keys(diagnostic).sort(), ['elapsedMilliseconds', 'exitCode', 'id', 'status'])
  assert.equal(diagnostic.status, 'pass')
  assert.equal(diagnostic.exitCode, 0)
  assert.equal(details.failureCode, 'none')
  assert.equal(options.timeout, 42)
  assert.equal(options.maxBuffer, MAX_CAPTURE_BYTES)
  assert.deepEqual(options.stdio, ['ignore', 'pipe', 'pipe'])
  assert.equal(DEFAULT_STEP_TIMEOUT_MS > 0, true)
})

test('failure output is bounded and redacts payload-like values', () => {
  const secret = 'token=super-secret canonicalJson={"auth":"private"}'
  const { diagnostic, details } = runBoundedStep(
    { id: 'test-fail', command: 'node' },
    {
      spawn: () => ({ status: 7, signal: null, stdout: `${secret}\n${'x'.repeat(5000)}`, stderr: secret }),
    },
  )

  assert.equal(diagnostic.status, 'fail')
  assert.equal(diagnostic.exitCode, 7)
  assert.equal(details.failureCode, 'exit')
  assert.doesNotMatch(details.stderr, /super-secret|private/)
  assert.ok(details.stdout.length <= 2050)
  assert.match(redactOutput('Authorization: Bearer abc123'), /\[REDACTED\]/)
  assert.match(tailOutput('a'.repeat(30), 10), /^\[truncated\]/)
})

test('timeout and spawn failures have deterministic nonzero exit codes', () => {
  const timeout = runBoundedStep(
    { id: 'test-timeout', command: 'node' },
    {
      spawn: () => ({
        status: null,
        signal: 'SIGTERM',
        error: Object.assign(new Error('timed out'), { code: 'ETIMEDOUT' }),
        stdout: '',
        stderr: 'timed out',
      }),
    },
  )
  const spawnError = runBoundedStep(
    { id: 'test-spawn', command: 'missing' },
    {
      spawn: () => {
        throw Object.assign(new Error('missing executable'), { code: 'ENOENT' })
      },
    },
  )

  assert.equal(timeout.diagnostic.exitCode, 124)
  assert.equal(timeout.details.failureCode, 'timeout')
  assert.equal(spawnError.diagnostic.exitCode, 1)
  assert.equal(spawnError.details.failureCode, 'spawn')
})

test('failure detail writer emits only bounded sanitized tails', () => {
  const chunks = []
  writeFailureDetails(
    { write: (value) => chunks.push(value) },
    'test-fail',
    { failureCode: 'exit', stderr: 'token=secret', stdout: '' },
  )
  const output = chunks.join('')
  assert.match(output, /failure=exit/)
  assert.doesNotMatch(output, /secret/)
})
