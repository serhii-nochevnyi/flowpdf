import { spawnSync } from 'node:child_process'
import { performance } from 'node:perf_hooks'

export const DEFAULT_STEP_TIMEOUT_MS = 15 * 60 * 1000
export const MAX_CAPTURE_BYTES = 1024 * 1024
export const MAX_FAILURE_OUTPUT_CHARS = 2048

const sensitiveAssignment =
  /\b(authorization|cookie|password|secret|token|api[_-]?key|canonicalJson|pdfBytes)\b\s*[:=]\s*("[^"]*"|'[^']*'|[^\s,;]+)/gi
const bearerToken = /\b(Bearer\s+)[A-Za-z0-9._~+\/-]+/gi

function asText(value) {
  if (value === undefined || value === null) return ''
  return Buffer.isBuffer(value) ? value.toString('utf8') : String(value)
}

export function redactOutput(value) {
  return asText(value)
    .replace(/\0/g, '')
    .replace(sensitiveAssignment, '$1=[REDACTED]')
    .replace(bearerToken, '$1[REDACTED]')
}

export function tailOutput(value, maxChars = MAX_FAILURE_OUTPUT_CHARS) {
  const text = redactOutput(value).trim()
  if (text.length <= maxChars) return text
  const marker = '[truncated]\n'
  return marker + text.slice(-(maxChars - marker.length))
}

function failureCode(result) {
  if (result?.error?.code === 'ETIMEDOUT') return 'timeout'
  if (result?.error?.code === 'ERR_CHILD_PROCESS_STDIO_MAXBUFFER') return 'output-limit'
  if (result?.error !== undefined) return 'spawn'
  if (result?.signal !== null && result?.signal !== undefined) return 'signal'
  return 'exit'
}

function exitCodeFor(result, code) {
  if (Number.isInteger(result?.status)) return result.status
  if (code === 'timeout') return 124
  if (code === 'signal') return 128
  return 1
}

export function runBoundedStep(
  { id, command, args = [], cwd = process.cwd(), env = process.env, timeoutMs = DEFAULT_STEP_TIMEOUT_MS },
  { spawn = spawnSync } = {},
) {
  const startedAt = performance.now()
  let result
  try {
    result = spawn(command, args, {
      cwd,
      env,
      shell: false,
      stdio: ['ignore', 'pipe', 'pipe'],
      encoding: 'utf8',
      timeout: timeoutMs,
      maxBuffer: MAX_CAPTURE_BYTES,
    })
  } catch (error) {
    result = { status: null, signal: null, error, stdout: '', stderr: '' }
  }

  const code = result?.status === 0 ? 'none' : failureCode(result)
  const elapsedMilliseconds = Math.round(performance.now() - startedAt)
  const diagnostic = {
    id,
    status: result?.status === 0 ? 'pass' : 'fail',
    exitCode: exitCodeFor(result, code),
    elapsedMilliseconds,
  }

  return {
    diagnostic,
    details: {
      failureCode: code,
      stderr: tailOutput(result?.stderr),
      stdout: tailOutput(result?.stdout),
    },
  }
}

export function writeFailureDetails(output, taskId, details) {
  if (details?.failureCode === undefined || details.failureCode === 'none') return
  output.write(`[${taskId}] failure=${details.failureCode}\n`)
  for (const [label, value] of [
    ['stderr', tailOutput(details.stderr)],
    ['stdout', tailOutput(details.stdout)],
  ]) {
    if (value) output.write(`[${taskId}] ${label} tail:\n${value}\n`)
  }
}
