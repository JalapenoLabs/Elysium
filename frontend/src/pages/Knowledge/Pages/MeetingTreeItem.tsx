// Copyright © 2026 Jalapeno Labs

import type { MeetingTreeEntry } from './pageTree'

// Core
import { memo } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Link, NavigationTreeItem, NavigationTreeItemContent } from 'react-aria-components'
import { LuVideo } from 'react-icons/lu'

// Misc
import { getMeetingUrl } from '../../../urls'
import { TREE_ROW_CLASS_NAME, TREE_ROW_CONTENT_CLASS_NAME } from './pageTreeRow'

type Props = {
  entry: MeetingTreeEntry
  onNavigate?: () => void
}

// A meeting in the tree: its home under the Meetings page, or a linked entry in a project's Meetings
// group, each a real link to its one page. A meeting is no page, so it has no menu, moves nowhere,
// and nothing drops onto it (`PageTree`). Its drag button stays disabled and out of sight, as a
// fixed page's does, since React Aria expects every row to have one.
export const MeetingTreeItem = memo(function MeetingTreeItem(props: Props) {
  const { t } = useTranslation([ 'meetings', 'knowledge' ])
  const title = props.entry.title || t('list.untitled')

  return <NavigationTreeItem
    id={props.entry.key}
    href={getMeetingUrl(props.entry.meetingId)}
    textValue={title}
    className={TREE_ROW_CLASS_NAME}
  >
    <NavigationTreeItemContent>
      <div className={TREE_ROW_CONTENT_CLASS_NAME}>
        <span aria-hidden className='size-6 shrink-0' />
        {/* The row's real link: `NavigationTreeItem` hands it the address and `aria-current`. */}
        <Link onPress={props.onNavigate} className='flex min-w-0 flex-1 items-center gap-1.5 py-1 outline-none'>
          <LuVideo aria-hidden className='size-3.5 shrink-0 opacity-70' />
          {/* Cut to the panel's width, so the whole title shows on hover. */}
          <span title={title} className='truncate'>{title}</span>
        </Link>
        <Button
          slot='drag'
          isDisabled
          aria-label={t('knowledge:pages.actions.drag', { title })}
          className='hidden'
        />
      </div>
    </NavigationTreeItemContent>
  </NavigationTreeItem>
})
