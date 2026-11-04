// Copyright © 2026 Jalapeno Labs

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Spinner } from '@heroui/react'

// Misc
import { useRunsPageLoader } from '../../hooks/useServerData'

type Props = {
  // The oldest run already shown; this loads the page of runs older than it.
  before: string
}

// Loads one older page of runs into Redux, where the table renders it from, and offers the
// page after it when another follows. Each page is one more of these, so every loaded page
// stays revalidated when the event stream reconnects.
export function OlderRunsLoader(props: Props) {
  const { t } = useTranslation('runs')
  const page = useRunsPageLoader(props.before)
  const [ showsOlder, setShowsOlder ] = useState(false)

  if (page.status === 'loading') {
    return <div className='grid place-items-center py-6'>
      <Spinner />
    </div>
  }
  if (page.status === 'failed') {
    return <p className='py-4 text-sm text-danger'>{t('list.loadError')}</p>
  }
  if (!page.hasMore || !page.oldest) {
    return null
  }
  if (showsOlder) {
    return <OlderRunsLoader before={page.oldest} />
  }
  return <div className='flex justify-center py-4'>
    <Button size='sm' variant='outline' onPress={() => setShowsOlder(true)}>
      <span>{t('list.loadOlder')}</span>
    </Button>
  </div>
}
