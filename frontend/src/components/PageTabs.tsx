// Copyright © 2026 Jalapeno Labs

import type { Key } from '@heroui/react'
import type { ReactNode } from 'react'

// User interface
import { Chip, Tabs } from '@heroui/react'

// Misc
import { formatNumber } from '../format/numbers'

export type PageTab<Id extends string> = {
  id: Id
  label: string
  // How many things the tab lists, or null while that is not known yet, when no badge
  // shows. Zero shows, so an empty list reads as empty rather than unknown.
  count: number | null
  // The count is what waits on the person, such as the inbox or sign-ups to approve: it is
  // accented while above zero.
  isWaiting?: boolean
  // A tab that is a page of its own, such as the Action items area's, is a real link.
  href?: string
}

type Props<Id extends string> = {
  label: string
  tabs: PageTab<Id>[]
  selectedKey: Id
  // Unset for tabs that are links, which change the page by navigating.
  onSelectionChange?: (key: Key) => void
  // The tabs' `Tabs.Panel`s, for tabs that are parts of one page.
  children?: ReactNode
}

// A page's lists, one per tab, each tab named with its total beside it (`~/.claude/docs/ux.md`,
// Lists of data). Pair it with `useTabParam`, so the open tab lives in the address.
export function PageTabs<Id extends string>(props: Props<Id>) {
  // Spaced below as a whole, so whatever follows the tabs (an `Outlet`, when the tabs are pages)
  // sits apart from them, and spaced between the list and its panel.
  return <Tabs
    className='relaxed'
    variant='secondary'
    selectedKey={props.selectedKey}
    onSelectionChange={props.onSelectionChange}
  >
    <Tabs.ListContainer className='relaxed'>
      <Tabs.List aria-label={props.label}>{
        props.tabs.map((tab) => <Tabs.Tab key={tab.id} id={tab.id} href={tab.href}>
          {/* A label stays on one line; a list too long for the page scrolls instead. */}
          <span className='whitespace-nowrap'>{tab.label}</span>
          {tab.count !== null && <Chip
            size='sm'
            variant='soft'
            color={tab.isWaiting && tab.count > 0
              ? 'accent'
              : 'default'}
            className='ml-2 tabular-nums'
          >{
              formatNumber(tab.count)
            }</Chip>}
          <Tabs.Indicator />
        </Tabs.Tab>)
      }</Tabs.List>
    </Tabs.ListContainer>
    {props.children}
  </Tabs>
}
