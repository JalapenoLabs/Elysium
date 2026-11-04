// Copyright © 2026 Jalapeno Labs

import type { Key } from '@heroui/react'

// Core
import { useSearchParams } from 'react-router'

// Misc
import { TAB_PARAM } from '../urls'

// The open tab of a page, kept in the address (`?tab=`) so it survives a reload and a link can
// open it. `tabs` are the tabs the person can see, in order: the first is the default, and
// opening it leaves the address plain. An unknown or hidden tab in the address opens the
// first, so someone who loses access to a tab keeps the ones they can see.
//
// Switching replaces the history entry rather than adding one, and the page's leave prompt
// never asks about it (`Page`): a tab shows another part of the same page, whose form stays
// mounted, so nothing typed is lost.
export function useTabParam<Tab extends string>(tabs: readonly Tab[]) {
  const [ params, setParams ] = useSearchParams()
  const requested = params.get(TAB_PARAM)
  const selectedTab = tabs.find((tab) => tab === requested) ?? tabs[0]

  function selectTab(key: Key) {
    const tab = tabs.find((candidate) => candidate === key)
    if (!tab) {
      console.debug('useTabParam ignored a tab this page does not show', { key, tabs })
      return
    }
    setParams((current) => {
      const next = new URLSearchParams(current)
      if (tab === tabs[0]) {
        next.delete(TAB_PARAM)
        return next
      }
      next.set(TAB_PARAM, tab)
      return next
    }, { replace: true })
  }

  return [ selectedTab, selectTab ] as const
}
