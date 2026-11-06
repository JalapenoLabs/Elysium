// Copyright © 2026 Jalapeno Labs

import type { SearchGroup, SearchResult, SearchSourceAnswer } from './searchTypes'

export type SearchResultSection = {
  group: SearchGroup
  results: SearchResult[]
}

// Every source's results, each thing once: two sources that found the same thing give it the same
// id, and the later source's result stands. The knowledge search comes last, so a page found by its
// title and by what it says opens at what it says.
export function mergeSearchResults(sources: SearchSourceAnswer[]) {
  const resultsById = new Map<string, SearchResult>()
  for (const source of sources) {
    for (const result of source.results) {
      resultsById.set(result.id, result)
    }
  }
  return [ ...resultsById.values() ]
}

// Every source's results in one order: the closest match first; then, between two results one
// source ranked itself, that source's order; then by title, so other equal matches read
// alphabetically whichever source found them.
export function rankSearchResults(results: SearchResult[]) {
  return [ ...results ].sort((first, second) => second.score - first.score
    || compareSourceRanks(first, second)
    || first.title.localeCompare(second.title))
}

function compareSourceRanks(first: SearchResult, second: SearchResult) {
  if (first.group !== second.group || first.sourceRank === undefined || second.sourceRank === undefined) {
    return 0
  }
  return first.sourceRank - second.sourceRank
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
