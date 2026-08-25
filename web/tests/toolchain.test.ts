import { describe, expect, it } from 'vitest'

declare const __FLOWPDF_VITEST_PROJECT__: string

describe('FlowPDF unit project', () => {
  it('runs once in the named Node project', () => {
    expect(__FLOWPDF_VITEST_PROJECT__).toBe('unit')
    expect(typeof window).toBe('undefined')
  })
})
