// Copyright © 2026 Jalapeno Labs

import type { TFunction } from 'i18next'
import type { KnowledgeResult } from '../api/routes/knowledgeRoutes'
import type { SearchResult } from './searchTypes'

// Misc
import { presentKnowledgeSource } from '../pages/Knowledge/knowledgePresentation'
import { getKnowledgeCitationUrl } from '../urls'
import { scoreSearchMatch, toSearchWords } from './searchMatch'

// The documents the knowledge search found, as results beside the registry's and the records'.
// Each is scored by its title with the registry's own rule, so the merged list ranks every
// source alike, and carries the knowledge search's own order (`sourceRank`), which decides
// between documents the title cannot tell apart: most are found by what they say, not by their
// titles. Each opens at its best passage, in its page when it was indexed from one. A page's id
// is the one the record search gives it, so a page found both ways is one result.
export function toKnowledgeResults(
  results: KnowledgeResult[],
  query: string,
  t: TFunction<'knowledge'>,
) {
  const queryWords = toSearchWords(query)
  const searchResults: SearchResult[] = []

  for (const [ index, result ] of results.entries()) {
    const { citation } = result
    const source = presentKnowledgeSource(citation.source, t)
    const title = citation.title || source.label
    const text = {
      titleWords: toSearchWords(title),
      tagWords: [],
      parentWords: [],
    }
    searchResults.push({
      id: citation.pageId
        ? `page:${citation.pageId}`
        : `knowledge:${citation.documentId}`,
      group: 'knowledge',
      title,
      detail: t('result.detail', { source: source.label, person: citation.person.name }),
      icon: source.icon,
      score: scoreSearchMatch(queryWords, text) ?? 0,
      sourceRank: index,
      target: {
        kind: 'link',
        href: getKnowledgeCitationUrl(citation, result.passages[0]?.position),
      },
    })
  }
  return searchResults
}
