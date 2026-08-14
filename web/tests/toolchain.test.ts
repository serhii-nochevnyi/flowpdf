import { describe, expect, it } from 'vitest'

describe('FlowPDF unit project', () => {
  it('runs once in the named Node project', () => {
    expect(['native', 'wasm', 'browser']).toHaveLength(3)
  })
})

