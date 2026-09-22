import { describe, expect, it } from 'vitest'

import type { EditorAcceptedSnapshot, DirectionalSelectionDto } from '../src/editor/editor-store.js'
import {
  createVoiceCommandCapture,
  createVoiceDictationCapture,
  resolveVoiceCommand,
  validateVoiceDictationCapture,
  VoiceBridgeError,
  type VoiceCommandWasmBoundary,
} from '../src/voice/voice-command.js'
import { VOICE_PROTOCOL_VERSION } from '../src/voice/voice-protocol.js'

const selection: DirectionalSelectionDto = {
  anchor: {
    nodeId: '00000000-0000-4000-8000-000000000001',
    utf16Offset: 0,
    affinity: 'forward',
  },
  focus: {
    nodeId: '00000000-0000-4000-8000-000000000001',
    utf16Offset: 5,
    affinity: 'forward',
  },
}

function accepted(): EditorAcceptedSnapshot {
  return {
    session: { revision: 4, canonicalHash: 'hash-4' },
    editor: { session: { selection } },
  } as unknown as EditorAcceptedSnapshot
}

function intent(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    protocolVersion: VOICE_PROTOCOL_VERSION,
    locale: 'en-US',
    sourceRevision: 4,
    selection,
    action: { type: 'setInlineMark', mark: { kind: 'bold', value: true } },
    capability: {
      family: 'setInlineMark',
      commandType: 'setInlineMark',
      intent: 'editor.intent.setInlineMark',
      risk: 'formatting',
      confirmation: 'none',
      undo: 'reversible',
    },
    ...overrides,
  }
}

function boundary(response: unknown): VoiceCommandWasmBoundary {
  return {
    resolve_voice_command: (requestJson) => {
      expect(JSON.parse(requestJson)).toMatchObject({
        protocolVersion: 1,
        locale: 'en-US',
        sourceRevision: 4,
        selection,
      })
      return JSON.stringify(response)
    },
  }
}

describe('Rust voice command bridge', () => {
  it('binds a validated intent to the captured source hash without echoing text', () => {
    const capture = createVoiceCommandCapture(accepted(), 'en-US', 'make bold', selection)
    const result = resolveVoiceCommand(
      boundary({ protocolVersion: 1, ok: true, intent: intent(), error: null }),
      capture,
    )

    expect(result.sourceHash).toBe('hash-4')
    expect(result.action).toEqual({
      type: 'setInlineMark',
      mark: { kind: 'bold', value: true },
    })
    expect(JSON.stringify(result)).not.toContain('make bold')
  })

  it('rejects malformed, stale, and capability-forged responses with stable codes', () => {
    const capture = createVoiceCommandCapture(accepted(), 'en-US', 'delete selection', selection)
    const malformed: VoiceCommandWasmBoundary = {
      resolve_voice_command: () => 'not-json',
    }
    expect(() => resolveVoiceCommand(malformed, capture)).toThrowError(
      new VoiceBridgeError('FLOW_VOICE_RESPONSE_DECODE'),
    )

    const stale = intent({ sourceRevision: 5 })
    expect(() =>
      resolveVoiceCommand(
        boundary({ protocolVersion: 1, ok: true, intent: stale, error: null }),
        capture,
      ),
    ).toThrowError(new VoiceBridgeError('FLOW_VOICE_SOURCE_MISMATCH'))

    const forged = intent({
      capability: {
        family: 'setInlineMark',
        commandType: 'replaceSelection',
        intent: 'editor.intent.setInlineMark',
        risk: 'formatting',
        confirmation: 'none',
        undo: 'reversible',
      },
    })
    expect(() =>
      resolveVoiceCommand(
        boundary({ protocolVersion: 1, ok: true, intent: forged, error: null }),
        capture,
      ),
    ).toThrowError(new VoiceBridgeError('FLOW_VOICE_CAPABILITY_MISMATCH'))
  })

  it('passes through privacy-safe Rust rejection codes without raw transcript text', () => {
    const transcript = 'private phrase must not become a diagnostic'
    const capture = createVoiceCommandCapture(accepted(), 'en-US', transcript, selection)
    const response = {
      protocolVersion: 1,
      ok: false,
      intent: null,
      error: { code: 'FLOW_VOICE_COMMAND_UNSUPPORTED' },
    }
    let error: unknown
    try {
      resolveVoiceCommand(boundary(response), capture)
    } catch (caught: unknown) {
      error = caught
    }
    expect(error).toEqual(new VoiceBridgeError('FLOW_VOICE_COMMAND_UNSUPPORTED'))
    expect(String(error)).not.toContain(transcript)
  })

  it('keeps dictation bounded by the existing input host limits', () => {
    const source = accepted()
    expect(validateVoiceDictationCapture(createVoiceDictationCapture(source, ''))).toBe(
      'FLOW_VOICE_DICTATION_EMPTY',
    )
    expect(validateVoiceDictationCapture(createVoiceDictationCapture(source, 'текст'))).toBeNull()
    expect(
      validateVoiceDictationCapture(
        createVoiceDictationCapture(source, 'a'.repeat(32_769)),
      ),
    ).toBe('FLOW_VOICE_DICTATION_LIMIT')
    expect(
      validateVoiceDictationCapture(
        createVoiceDictationCapture(source, String.fromCharCode(0xd800)),
      ),
    ).toBe('FLOW_INVALID_UTF16_BOUNDARY')
  })
})
