// Copyright © 2026 Jalapeno Labs

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Navigate } from 'react-router'

// Redux
import { selectIsCompanyMember } from '../../store/authSlice'
import { useAppSelector } from '../../store/hooks'
import { selectRuns } from '../../store/runsSlice'

// User interface
import { Button, Spinner } from '@heroui/react'
import { EmptyNotice } from '../../components/EmptyNotice'
import { OlderRunsLoader } from './OlderRunsLoader'
import { RunTable } from './RunTable'

// Misc
import { useRunsPageLoader } from '../../hooks/useServerData'
import { UrlTree } from '../../urls'

// `/runs`: every workflow run, newest first, a page at a time. Runs read what was ingested, so
// a guest, whose sidebar has no link here, is sent back to the start.
export function RunsPage() {
  const isCompanyMember = useAppSelector(selectIsCompanyMember)

  if (!isCompanyMember) {
    return <Navigate to={UrlTree.root} replace />
  }
  return <RunsList />
}

function RunsList() {
  const { t } = useTranslation('runs')
  const newest = useRunsPageLoader(null)
  const runs = useAppSelector(selectRuns)
  const [ showsOlder, setShowsOlder ] = useState(false)

  return <div className='container'>
    <h1 className='relaxed text-3xl font-bold'>{
      t('title')
    }</h1>
    {newest.status === 'loading' && <div className='grid place-items-center py-16'>
      <Spinner />
    </div>}
    {newest.status === 'failed' && !runs.length && <p className='py-10 text-center text-sm text-danger'>{
      t('list.loadError')
    }</p>}
    {newest.status === 'loaded' && !runs.length && <EmptyNotice>{
      t('list.empty')
    }</EmptyNotice>}
    {runs.length > 0 && <RunTable runs={runs} />}
    {newest.hasMore && newest.oldest && (showsOlder
      ? <OlderRunsLoader before={newest.oldest} />
      : <div className='flex justify-center py-4'>
        <Button size='sm' variant='outline' onPress={() => setShowsOlder(true)}>
          <span>{t('list.loadOlder')}</span>
        </Button>
      </div>)}
  </div>
}
