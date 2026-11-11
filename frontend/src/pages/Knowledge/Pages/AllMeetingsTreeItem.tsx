// Copyright © 2026 Jalapeno Labs

// Core
import { memo } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Link, NavigationTreeItem, NavigationTreeItemContent } from 'react-aria-components'
import { LuArrowRight } from 'react-icons/lu'

// Misc
import { formatNumber } from '../../../format/numbers'
import { TREE_ROW_CLASS_NAME, TREE_ROW_CONTENT_CLASS_NAME } from './pageTreeRow'

type Props = {
  // Unique across the tree, as every row's is.
  itemKey: string
  // The page that lists the branch's meetings whole.
  href: string
  // How many meetings the branch holds in all.
  total: number
  onNavigate?: () => void
}

// The last row of a branch of meetings that lists only its newest: a link to the page that lists
// all of them. Like a meeting's row, it is no page, so it has no menu and moves nowhere.
export const AllMeetingsTreeItem = memo(function AllMeetingsTreeItem(props: Props) {
  const { t } = useTranslation('knowledge')
  const label = t('pages.allMeetings', { count: props.total, formattedCount: formatNumber(props.total) })

  return <NavigationTreeItem
    id={props.itemKey}
    href={props.href}
    textValue={label}
    className={TREE_ROW_CLASS_NAME}
  >
    <NavigationTreeItemContent>
      <div className={TREE_ROW_CONTENT_CLASS_NAME}>
        <span aria-hidden className='size-6 shrink-0' />
        <Link onPress={props.onNavigate} className='flex min-w-0 flex-1 items-center gap-1.5 py-1 outline-none'>
          <LuArrowRight aria-hidden className='size-3.5 shrink-0 opacity-70' />
          <span className='truncate opacity-80'>{label}</span>
        </Link>
        <Button
          slot='drag'
          isDisabled
          aria-label={t('pages.actions.drag', { title: label })}
          className='hidden'
        />
      </div>
    </NavigationTreeItemContent>
  </NavigationTreeItem>
})
