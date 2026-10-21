// Copyright © 2026 Jalapeno Labs

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Spinner } from '@heroui/react'

// Misc
import { TIMELINE_PAGE_SIZE } from '../../constants'
import { useProjectTimelineLoader } from '../../hooks/useServerData'

type Props = {
  projectId: string
  // Where the previous page ended, or null for the newest page.
  before: string | null
}

// Loads one page of a project's timeline into Redux, where the timeline renders it from, and
// offers the next older page when this one was full. Each older page is one more of these,
// so every loaded page stays revalidated by the event stream.
export function TimelinePageLoader(props: Props) {
  const { t } = useTranslation('timeline')
  const page = useProjectTimelineLoader(props.projectId, props.before)
  const [ showsOlder, setShowsOlder ] = useState(false)

  if (page.status === 'loading') {
    return <div className='grid place-items-center py-6'>
      <Spinner />
    </div>
  }
  if (page.status === 'failed') {
    return <p className='py-4 text-sm text-danger'>{t('failed')}</p>
  }
  if (page.pageSize < TIMELINE_PAGE_SIZE || !page.oldest) {
    return null
  }
  if (showsOlder) {
    return <TimelinePageLoader projectId={props.projectId} before={page.oldest} />
  }
  return <div className='flex justify-center py-4'>
    <Button size='sm' variant='outline' onPress={() => setShowsOlder(true)}>
      <span>{t('loadOlder')}</span>
    </Button>
  </div>
}
