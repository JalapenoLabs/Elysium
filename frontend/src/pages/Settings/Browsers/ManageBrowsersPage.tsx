// Copyright © 2026 Jalapeno Labs

import type { BrowserSession } from '../../../api/routes/browserSessionRoutes'

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { browserSessionDeleted, selectAllBrowserSessions } from '../../../store/browserSessionsSlice'
import { useAppDispatch, useAppSelector } from '../../../store/hooks'

// User interface
import { Breadcrumbs, Button, Spinner, toast, useOverlayState } from '@heroui/react'
import { LuPlus } from 'react-icons/lu'
import { AddBrowserSessionModal } from './AddBrowserSessionModal'
import { BrowserSessionCard } from './BrowserSessionCard'

// Misc
import { getApiErrorMessage } from '../../../api/errors'
import { deleteBrowserSession } from '../../../api/routes/browserSessionRoutes'
import { useConfirm } from '../../../hooks/useConfirm'
import { useBrowserSessionsLoader } from '../../../hooks/useServerData'
import { UrlTree } from '../../../urls'

// `/settings/browsers`: the browsers Elysium runs for the signed-in person to sign in to their
// accounts in, as themselves.
export function ManageBrowsersPage() {
  const { t } = useTranslation([ 'browsers', 'settings', 'common' ])
  const dispatch = useAppDispatch()
  const confirm = useConfirm()
  const browserSessions = useAppSelector(selectAllBrowserSessions)
  const status = useBrowserSessionsLoader()
  const addState = useOverlayState()

  function confirmDelete(browserSession: BrowserSession) {
    confirm({
      title: t('delete.title', { name: browserSession.name }),
      message: t('delete.body'),
      tone: 'danger',
      confirmText: t('common:actions.delete'),
      onConfirm: async () => {
        try {
          await deleteBrowserSession(browserSession.id)
        }
        catch (error) {
          console.debug('ManageBrowsersPage failed to delete a browser', { error, id: browserSession.id })
          toast.danger(getApiErrorMessage(error) ?? t('common:errors.unexpected'))
          // Keeps the dialog open to try again.
          throw error
        }
        dispatch(browserSessionDeleted(browserSession.id))
        toast.success(t('toasts.deleted', { name: browserSession.name }))
      },
    })
  }

  return <div className='container'>
    <Breadcrumbs className='compact'>
      <Breadcrumbs.Item href={UrlTree.settings}>{t('settings:title')}</Breadcrumbs.Item>
      <Breadcrumbs.Item>{t('title')}</Breadcrumbs.Item>
    </Breadcrumbs>
    <section>
      <div className='level relaxed items-center'>
        <h1 className='text-3xl font-bold'>{
          t('title')
        }</h1>
        <Button
          size='sm'
          variant='outline'
          className='shrink-0'
          onPress={addState.open}
        >
          <LuPlus className='size-4' aria-hidden />
          <span>{t('page.add')}</span>
        </Button>
      </div>

      <p className='relaxed text-sm opacity-70'>{t('page.description')}</p>

      {status === 'loading' && <div className='grid place-items-center py-16'>
        <Spinner />
      </div>}

      {status === 'failed' && <p className='py-10 text-center text-sm text-danger'>{
        t('page.loadError')
      }</p>}

      {status === 'loaded' && !browserSessions.length && <p className='py-10 text-center text-sm opacity-70'>{
        t('page.empty')
      }</p>}

      {status === 'loaded' && browserSessions.map((browserSession) => <BrowserSessionCard
        key={browserSession.id}
        browserSession={browserSession}
        onDelete={confirmDelete}
      />)}
    </section>
    <AddBrowserSessionModal state={addState} />
  </div>
}
