// Copyright © 2026 Jalapeno Labs

import type { KnowledgeAnswerSource } from '../../../api/routes/knowledgeRoutes'

// Core
import { describe, expect, it, vi } from 'vitest'

// Misc
import { presentClaims, waitingLabelKey } from './knowledgeAskPresentation'

function source(number: number, title: string): KnowledgeAnswerSource {
  return {
    number,
    position: number - 1,
    citation: {
      documentId: `document-${number}`,
      source: 'slack',
      title,
      url: null,
      pageId: null,
      meetingId: null,
      occurredAt: '2026-11-03T10:00:00Z',
      person: { id: 'person', name: 'Jordan Lee' },
    },
  }
}

describe('presentClaims', () => {
  it('gives each claim its sources in the order it cites them', () => {
    const claims = presentClaims({
      claims: [
        { text: 'We deploy on Thursdays.', sources: [ 2, 1 ]},
        { text: 'Deploys broke twice.', sources: [ 2 ]},
      ],
      sources: [ source(1, 'Deploy plan'), source(2, 'Retro') ],
      unsupportedClaims: 0,
    })

    expect(claims.map((claim) => claim.sources.map((cited) => cited.citation.title))).toEqual([
      [ 'Retro', 'Deploy plan' ],
      [ 'Retro' ],
    ])
    expect(claims[0].text).toBe('We deploy on Thursdays.')
  })

  it('leaves out a number the answer lists no source for', () => {
    const debug = vi.spyOn(console, 'debug').mockImplementation(() => undefined)
    const claims = presentClaims({
      claims: [{ text: 'Something.', sources: [ 1, 7 ]}],
      sources: [ source(1, 'Deploy plan') ],
      unsupportedClaims: 0,
    })

    expect(claims[0].sources).toHaveLength(1)
    expect(debug).toHaveBeenCalled()
    debug.mockRestore()
  })
})

describe('waitingLabelKey', () => {
  it('names an ask still on its way, and nothing once it ended', () => {
    expect(waitingLabelKey('queued')).toBe('ask.state.queued')
    expect(waitingLabelKey('running')).toBe('ask.state.running')
    expect(waitingLabelKey('answered')).toBeNull()
    expect(waitingLabelKey('failed')).toBeNull()
  })
})
