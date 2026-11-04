// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'
import type {
  KnowledgeAnswer,
  KnowledgeAnswerSource,
  KnowledgeAskState,
} from '../../../api/routes/knowledgeRoutes'

// What an ask still on its way says while its answer is written.
const waitingLabelKeys = {
  queued: 'ask.state.queued',
  running: 'ask.state.running',
} as const satisfies Partial<Record<KnowledgeAskState, ParseKeys<'knowledge'>>>

// The words for an ask whose answer is still on its way, or null once it ended.
export function waitingLabelKey(state: KnowledgeAskState) {
  if (state === 'queued' || state === 'running') {
    return waitingLabelKeys[state]
  }
  return null
}

// One statement of an answer with the sources it cites, in the order it cites them. A number
// the answer lists no source for is left out: the API never sends one, so it is logged.
export type PresentedClaim = {
  text: string
  sources: KnowledgeAnswerSource[]
}

// An answer's claims, each with its sources, in one pass over each list.
export function presentClaims(answer: KnowledgeAnswer): PresentedClaim[] {
  const sourceByNumber = new Map<number, KnowledgeAnswerSource>()
  for (const source of answer.sources) {
    sourceByNumber.set(source.number, source)
  }

  return answer.claims.map((claim) => {
    const sources: KnowledgeAnswerSource[] = []
    for (const number of claim.sources) {
      const source = sourceByNumber.get(number)
      if (!source) {
        console.debug('An answer cited a source it does not list, so it was left out', { number })
        continue
      }
      sources.push(source)
    }
    return { text: claim.text, sources }
  })
}
