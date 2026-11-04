// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'
import type { IconType } from 'react-icons'
import type { ConfirmOptions } from '../gates/ConfirmGate'
import type { TranslationNamespace } from '../i18n'
import type { AppDispatch } from '../store'

// A translation key spelled with its namespace (`settings:items.security.title`), since
// search entries come from every area and reuse each area's own words.
export type TranslationKey = Extract<ParseKeys<TranslationNamespace[]>, `${string}:${string}`>

// The kinds of result search shows, each a section of its own. Server-backed kinds (items,
// projects, sessions, initiatives) join this list with their source.
export type SearchGroup = 'pages' | 'actions'

// Who an entry is for, matching who the page or action itself admits: members are the
// company's own people (`selectIsCompanyMember`), admins its active admins.
export type SearchAudience = 'everyone' | 'members' | 'admins'

// What an action can reach when it runs, handed over by the search that runs it: the store,
// and the app's confirmation dialog for an action that asks first.
export type SearchActionContext = {
  dispatch: AppDispatch
  confirm: (options: ConfirmOptions) => void
}

type SearchEntryBase = {
  // Unique across every registry file.
  id: string
  title: TranslationKey
  // Hidden words it also answers to, such as "2FA" for Two-factor authentication, as one
  // string of words, so each language lists its own.
  tags?: TranslationKey
  // The page it sits under, shown beside the title and matched less strongly than it.
  parent?: TranslationKey
  icon: IconType
  // Everyone when left out.
  audience?: SearchAudience
}

// One page or action a part of the app registers in a `searchEntries.ts` beside itself (see
// `searchRegistry.ts`). A page is always a link; an action is a link when it opens a page,
// such as a create page, and runs when it does something in place, such as signing out.
export type SearchEntry = SearchEntryBase & (
  | { group: 'pages', href: string }
  | { group: 'actions', href: string }
  | { group: 'actions', run: (context: SearchActionContext) => void }
)

// Where a chosen result goes: a real link, so it can be middle-clicked or opened in a new
// tab, or something it does in place.
export type SearchTarget =
  | { kind: 'link', href: string }
  | { kind: 'action', run: () => void }

// One result, from whichever source found it, ready to show.
export type SearchResult = {
  // Unique across every source.
  id: string
  group: SearchGroup
  title: string
  // A second, quieter line, such as the page a result sits under.
  detail?: string
  icon: IconType
  // Higher ranks first, across every source.
  score: number
  target: SearchTarget
}

// What one source answers for a query. Every source reports whether it is still loading, so
// the search shows its loading box and spinner without knowing where results come from.
export type SearchSourceAnswer = {
  results: SearchResult[]
  isLoading: boolean
}
