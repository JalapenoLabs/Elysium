// Copyright © 2026 Jalapeno Labs

import type { KnowledgePage } from '../../../api/routes/pageRoutes'

// Core
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import useSWR from 'swr'

// Redux
import { useAppSelector } from '../../../store/hooks'
import { selectPageTree } from '../../../store/pagesSlice'

// User interface
import { Button, buttonVariants, Link, Spinner } from '@heroui/react'
import { LuGitCompareArrows } from 'react-icons/lu'
import { PageHistoryRow } from './PageHistoryRow'

// Misc
import { listPageHistory, pageHistoryKey } from '../../../api/routes/pageRoutes'
import { useUserNames } from '../../../hooks/useUserNames'
import { getPageCompareUrl } from '../../../urls'
import { describePageHistory, pageAgentName, pageHistorySentenceKeys } from './pageHistoryPresentation'

type Props = {
  page: KnowledgePage
  // How many entries show: a history holds no text, so it all loads at once, but drawing all of a
  // long one would help nobody. Held by `PageHistory`, as `picked` is, so a person who opens a
  // version comes back to the list as they left it.
  shownCount: number
  onShowMore: () => void
  picked: number[]
  onPick: (version: number, isPicked: boolean) => void
}

// Everything that happened to a page, newest first. It is read from SWR alone, since no other view
// shows it, and `page.changed` revalidates it, so a publish made anywhere appears here.
export function PageHistoryList(props: Props) {
  const { t } = useTranslation([ 'knowledge', 'common' ])
  const userNames = useUserNames()
  const tree = useAppSelector(selectPageTree)
  const pageId = props.page.id
  const history = useSWR(pageHistoryKey(pageId), () => listPageHistory(pageId))
  const rows = useMemo(() => describePageHistory(history.data?.history ?? []), [ history.data ])

  if (history.error && !history.data) {
    return <p className='text-sm text-danger'>{t('pages.history.loadError')}</p>
  }
  if (!history.data) {
    return <div className='grid place-items-center py-6'>
      <Spinner size='sm' />
    </div>
  }

  // The tree knows live pages alone, so a parent deleted since reads as what it is.
  function parentName(parentId: string | null) {
    if (parentId === null) {
      return t('pages.history.topLevel')
    }
    return tree.byId.get(parentId)?.title ?? t('pages.history.deletedParent')
  }

  const canCompare = props.page.currentVersion > 1
  const [ firstPick, secondPick ] = props.picked

  return <div>
    {/* It stays in view while the two versions are picked far apart; its offset cancels the padding
        at the top of the content column, which would otherwise show rows passing above it. */}
    {canCompare && <div className={[
      'compact sticky -top-4 z-10 flex flex-wrap items-center justify-between gap-x-4 gap-y-2',
      'border-b border-separator bg-background py-2',
    ].join(' ')}>
      <p className='text-sm opacity-70'>{
        secondPick === undefined
          ? t(firstPick === undefined
            ? 'pages.history.pickTwo'
            : 'pages.history.pickOneMore')
          : t('pages.history.picked', { first: firstPick, second: secondPick })
      }</p>
      {secondPick === undefined
        ? <Button size='sm' variant='secondary' isDisabled>
          <LuGitCompareArrows className='size-4' aria-hidden />
          <span>{t('pages.history.compare')}</span>
        </Button>
        : <Link
          href={getPageCompareUrl(pageId, firstPick, secondPick)}
          className={buttonVariants({ size: 'sm', variant: 'primary' })}
        >
          <LuGitCompareArrows className='size-4' aria-hidden />
          <span>{t('pages.history.compare')}</span>
        </Link>}
    </div>}
    <ol className='compact'>{
      rows.slice(0, props.shownCount).map((row) => {
        const { entry, version } = row
        const name = userNames[entry.createdBy] ?? t('common:table.someone')
        const who = entry.agent
          ? t('pages.history.whoThroughAgent', { name, agent: pageAgentName(entry.agent, t) })
          : name

        return <PageHistoryRow
          key={entry.id}
          page={props.page}
          row={row}
          sentence={t(pageHistorySentenceKeys[row.sentence], {
            who,
            version: row.otherVersion,
            from: row.move && parentName(row.move.fromParentId),
            to: row.move && parentName(row.move.toParentId),
            label: row.label,
            name: row.attachment?.name,
          })}
          isPicked={version !== null && props.picked.includes(version)}
          onPickChange={canCompare && version !== null
            ? (isPicked) => props.onPick(version, isPicked)
            : undefined}
        />
      })
    }</ol>
    {rows.length > props.shownCount && <Button size='sm' variant='ghost' onPress={props.onShowMore}>
      <span>{t('pages.history.older')}</span>
    </Button>}
  </div>
}
