// Copyright © 2026 Jalapeno Labs

import type { LearningFeedEntry } from '../../../api/routes/learningRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { shallowEqual } from 'react-redux'
import useSWR from 'swr'

// Redux
import { useAppSelector } from '../../../store/hooks'
import { selectUserNamesById } from '../../../store/usersSlice'

// User interface
import { Button, Chip, Link, Spinner, toast } from '@heroui/react'
import { EmptyNotice } from '../../../components/EmptyNotice'
import { DateText } from '../../../format/DateText'

// Misc
import { getLearningFeed, LEARNING_FEED_KEY } from '../../../api/routes/learningRoutes'
import { useLearningCategories } from '../../../hooks/useLearningCategories'
import { useUsersLoader } from '../../../hooks/useServerData'
import { getLearningUrl } from '../../../urls'
import { describeHistoryEntry, feedSentenceKeys, fieldLabelKeys } from './learningPresentation'

// The pages read after the first one, and where the first page ended when they were read.
type OlderPages = {
  from: string | null
  entries: LearningFeedEntry[]
  nextBefore: string | null
}

// For admins: every change to every learning, newest first, so they follow what people and
// their agents save. The first page is read from SWR, which `learning.changed` revalidates, so a
// new learning appears at the top as it is saved; Show older reads on from the last one shown.
export function LearningFeed() {
  const { t, i18n } = useTranslation([ 'learnings', 'common' ])
  useUsersLoader()
  const userNames = useAppSelector(selectUserNamesById, shallowEqual)
  const { labels: categoryLabels } = useLearningCategories()
  const firstPage = useSWR(LEARNING_FEED_KEY, () => getLearningFeed(null))
  const [ older, setOlder ] = useState<OlderPages | null>(null)
  const [ isLoadingOlder, setIsLoadingOlder ] = useState(false)

  if (firstPage.error) {
    return <p className='py-10 text-center text-sm text-danger'>{t('feed.loadError')}</p>
  }
  if (!firstPage.data) {
    return <div className='grid place-items-center py-16'>
      <Spinner />
    </div>
  }

  // Older pages read on from where the first page ended. Once a new change moves the first
  // page, they no longer follow on from it, so they are dropped rather than leave a gap.
  const firstPageEnd = firstPage.data.nextBefore
  const olderPages = older?.from === firstPageEnd
    ? older
    : null
  const nextBefore = olderPages
    ? olderPages.nextBefore
    : firstPageEnd
  const entries = [ ...firstPage.data.entries, ...olderPages?.entries ?? [] ]

  async function showOlder(before: string) {
    setIsLoadingOlder(true)
    try {
      const page = await getLearningFeed(before)
      setOlder({
        from: firstPageEnd,
        entries: [ ...olderPages?.entries ?? [], ...page.entries ],
        nextBefore: page.nextBefore,
      })
    }
    catch (error) {
      console.debug('LearningFeed could not read older entries', { error, before })
      toast.danger(t('feed.loadError'))
    }
    finally {
      setIsLoadingOlder(false)
    }
  }

  if (!entries.length) {
    return <EmptyNotice>{t('feed.empty')}</EmptyNotice>
  }

  const listFormatter = new Intl.ListFormat(i18n.language, { type: 'conjunction' })

  return <div>
    <ol className='relaxed flex flex-col divide-y divide-separator rounded-xl border border-separator'>{
      entries.map(({ entry, learning }) => {
        const description = describeHistoryEntry(entry)
        const name = userNames[entry.createdBy] ?? t('common:table.someone')
        const who = entry.agent
          ? t('history.byAgent', { name, agent: entry.agent.name ?? t('agent.unnamed') })
          : name
        const fields = listFormatter.format(description.changedFields.map((field) => t(fieldLabelKeys[field])))
        const sentence = description.kind
          ? t(feedSentenceKeys[description.kind], { who, fields })
          : t('feed.entries.unknown', { who })

        return <li key={entry.id} className='flex flex-col gap-1 px-4 py-3 text-sm'>
          <div className='level gap-3'>
            <span className='opacity-80'>{sentence}</span>
            <DateText value={entry.createdAt} className='shrink-0 text-xs opacity-50' />
          </div>
          <div className='flex flex-wrap items-center gap-2'>
            <Link href={getLearningUrl(learning.id)} className='font-medium text-link'>{learning.title}</Link>
            <Chip size='sm' variant='soft'>{categoryLabels[learning.category] ?? learning.category}</Chip>
          </div>
        </li>
      })
    }</ol>
    {nextBefore && <div className='flex justify-center'>
      <Button variant='outline' isPending={isLoadingOlder} onPress={() => showOlder(nextBefore)}>
        <span>{t('feed.older')}</span>
      </Button>
    </div>}
  </div>
}
