// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'
import { useParams } from 'react-router'

// Redux
import { selectBrowserSessionById } from '../../../store/browserSessionsSlice'
import { useAppSelector } from '../../../store/hooks'

// User interface
import { Breadcrumbs, Chip, Spinner } from '@heroui/react'
import { BrowserStream } from '../../../browser/BrowserStream'

// Misc
import { useBrowserSessionsLoader } from '../../../hooks/useServerData'
import { UrlTree } from '../../../urls'
import { stateChipColors, stateLabelKeys } from './browserPresentation'

// `/settings/browsers/:browserSessionId`: one browser, live, to sign in to an account in.
export function BrowserSessionPage() {
  const { t } = useTranslation([ 'browsers', 'settings' ])
  const { browserSessionId = '' } = useParams()
  const status = useBrowserSessionsLoader()
  const browserSession = useAppSelector((state) => selectBrowserSessionById(state, browserSessionId))

  if (status === 'loading') {
    return <div className='grid place-items-center py-16'>
      <Spinner />
    </div>
  }

  if (!browserSession) {
    return <p className='py-10 text-center text-sm opacity-70'>{
      t('session.missing')
    }</p>
  }

  const { state, error } = browserSession.status

  return <div className='container'>
    <Breadcrumbs className='compact'>
      <Breadcrumbs.Item href={UrlTree.settings}>{t('settings:title')}</Breadcrumbs.Item>
      <Breadcrumbs.Item href={UrlTree.settingsBrowsers}>{t('title')}</Breadcrumbs.Item>
      <Breadcrumbs.Item>{browserSession.name}</Breadcrumbs.Item>
    </Breadcrumbs>
    <div className='level relaxed items-center'>
      <h1 className='text-3xl font-bold'>{
        browserSession.name
      }</h1>
      <Chip size='sm' variant='soft' color={stateChipColors[state]}>{
        t(stateLabelKeys[state])
      }</Chip>
    </div>

    <p className='compact text-sm opacity-70'>{t('session.hint')}</p>

    {state === 'starting' && <div className='grid place-items-center gap-3 py-16'>
      <Spinner />
      <p className='text-sm opacity-70'>{t('session.starting')}</p>
    </div>}

    {state === 'failed' && <p className='py-10 text-center text-sm text-danger'>{
      error ?? t('session.failed')
    }</p>}

    {state === 'running' && <BrowserStream browserSessionId={browserSession.id} />}
  </div>
}
