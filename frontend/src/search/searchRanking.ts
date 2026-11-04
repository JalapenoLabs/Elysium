// Copyright © 2026 Jalapeno Labs

import type { SearchGroup, SearchResult } from './searchTypes'

export type SearchResultSection = {
  group: SearchGroup
  results: SearchResult[]
}

// Every source's results in one order: the closest match first, then by title, so equal
// matches read alphabetically whichever source found them.
export function rankSearchResults(results: SearchResult[]) {
  return [ ...results ].sort((first, second) => second.score - first.score
    || first.title.localeCompare(second.title))
}

// Ranked results as sections by kind, each section placed where its best result ranks and
// keeping the ranked order inside it, so the closest match always leads.
export function groupSearchResults(rankedResults: SearchResult[]) {
  const sections: SearchResultSection[] = []
  const sectionByGroup = new Map<SearchGroup, SearchResultSection>()

  for (const result of rankedResults) {
    let section = sectionByGroup.get(result.group)
    if (!section) {
      section = { group: result.group, results: []}
      sectionByGroup.set(result.group, section)
      sections.push(section)
    }
    section.results.push(result)
  }
  return sections
}
