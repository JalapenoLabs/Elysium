// Copyright © 2026 Jalapeno Labs

// The words of an entry, split once per language rather than on every keystroke.
export type SearchableText = {
  titleWords: string[]
  tagWords: string[]
  parentWords: string[]
}

// How strongly one typed word matches, the best field winning: the title over the hidden
// tags, and both over the page an entry sits under, so "settings" lists Settings itself
// before every page inside it.
const WORD_SCORES = {
  titleExact: 4,
  titlePrefix: 3,
  tagExact: 2,
  tagPrefix: 1.5,
  parentPrefix: 1,
} as const

// A title that begins with what was typed, word for word, outranks one that merely holds
// the same words: "new p" puts New project ahead of a page whose tags say "new".
const TITLE_START_BONUS = 2

// Lowercased words in any script, with accents dropped and punctuation as a break, so
// "Two-factor" is two words and "réglages" matches "reglages".
export function toSearchWords(text: string) {
  return text
    .normalize('NFD')
    .replace(/\p{M}/gu, '')
    .toLowerCase()
    .split(/[^\p{L}\p{N}]+/u)
    .filter(Boolean)
}

// Every typed word must begin some word of the entry; a word found nowhere rules it out.
// Returns null for no match, and a higher number for a closer one.
export function scoreSearchMatch(queryWords: string[], text: SearchableText) {
  if (!queryWords.length) {
    return null
  }

  let score = 0
  for (const queryWord of queryWords) {
    const wordScore = scoreWord(queryWord, text)
    if (!wordScore) {
      return null
    }
    score += wordScore
  }

  const startsTitle = queryWords.every((queryWord, index) => text.titleWords[index]?.startsWith(queryWord))
  if (startsTitle) {
    score += TITLE_START_BONUS
  }
  return score
}

function scoreWord(queryWord: string, text: SearchableText) {
  let best = 0
  for (const word of text.titleWords) {
    if (word === queryWord) {
      return WORD_SCORES.titleExact
    }
    if (word.startsWith(queryWord)) {
      best = WORD_SCORES.titlePrefix
    }
  }
  if (best) {
    return best
  }

  for (const word of text.tagWords) {
    if (word === queryWord) {
      return WORD_SCORES.tagExact
    }
    if (word.startsWith(queryWord)) {
      best = WORD_SCORES.tagPrefix
    }
  }
  if (best) {
    return best
  }

  for (const word of text.parentWords) {
    if (word.startsWith(queryWord)) {
      return WORD_SCORES.parentPrefix
    }
  }
  return 0
}
