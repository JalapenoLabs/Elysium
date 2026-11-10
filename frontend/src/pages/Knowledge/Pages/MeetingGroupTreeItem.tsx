// Copyright © 2026 Jalapeno Labs

import type { MeetingTreeGroup } from './pageTree'

// Core
import { memo } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Link, NavigationTreeItem, NavigationTreeItemContent } from 'react-aria-components'
import { MeetingTreeItem } from './MeetingTreeItem'
import { PageTreeChevron } from './PageTreeChevron'

// Misc
import { formatNumber } from '../../../format/numbers'
import { getProjectMeetingsUrl } from '../../../urls'
import { TREE_ROW_CLASS_NAME, TREE_ROW_CONTENT_CLASS_NAME } from './pageTreeRow'

type Props = {
  group: MeetingTreeGroup
  onNavigate?: () => void
}

// A project's Meetings group, under its brief: the Meetings page narrowed to the project, with its
// count, and every meeting with a topic on the project under it, newest first, each a linked entry
// of the meeting's one page. Like its entries it is no page, so it has no menu and moves nowhere.
export const MeetingGroupTreeItem = memo(function MeetingGroupTreeItem(props: Props) {
  const { t } = useTranslation('knowledge')
  const label = t('pages.meetingsGroup')

  return <NavigationTreeItem
    id={props.group.key}
    href={getProjectMeetingsUrl(props.group.projectId)}
    textValue={label}
    className={TREE_ROW_CLASS_NAME}
  >
    <NavigationTreeItemContent>{
      (row) => <div className={TREE_ROW_CONTENT_CLASS_NAME}>
        <PageTreeChevron isExpanded={row.isExpanded} />
        <Link onPress={props.onNavigate} className='flex min-w-0 flex-1 items-center gap-2 py-1 outline-none'>
          <span className='flex-1 truncate'>{label}</span>
          <span className='text-xs opacity-70'>{formatNumber(props.group.entries.length)}</span>
        </Link>
        <Button
          slot='drag'
          isDisabled
          aria-label={t('pages.actions.drag', { title: label })}
          className='hidden'
        />
      </div>
    }</NavigationTreeItemContent>
    {props.group.entries.map((entry) => <MeetingTreeItem
      key={entry.key}
      entry={entry}
      onNavigate={props.onNavigate}
    />)}
  </NavigationTreeItem>
})
