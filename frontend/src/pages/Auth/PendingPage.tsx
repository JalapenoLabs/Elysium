// Copyright © 2026 Jalapeno Labs

// Core
import { useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import useSWR from 'swr'

// User interface
import { Button, Spinner } from '@heroui/react'
import { LuHourglass } from 'react-icons/lu'

// Misc
import { getMe } from '../../api/routes/authRoutes'
import { signOut } from '../../auth/signOut'
import { PENDING_APPROVAL_POLL_MS } from '../../constants'
import { getLoginUrl, UrlTree } from '../../urls'

// Where someone waits after signing up, until an admin approves them. The one view that
// polls: a pending person may not open the event stream, so it asks every few seconds and
// opens the workspace, with a full page load, the moment they are let in.
export function PendingPage() {
  const { t } = useTranslation('auth')
  const { data: me, error } = useSWR('v1/me', getMe, {
    refreshInterval: PENDING_APPROVAL_POLL_MS,
  })
  const status = me?.user.status

  useEffect(() => {
    if (status === 'active') {
      window.location.assign(UrlTree.root)
      return
    }
    if (status === 'disabled') {
      window.location.assign(getLoginUrl({ notice: 'disabled' }))
    }
  }, [ status ])

  // A rejected sign-up has no account left, and its session ends with it.
  useEffect(() => {
    if (error) {
      console.debug('PendingPage could not read the account; signing in again', { error })
      window.location.assign(UrlTree.login)
    }
  }, [ error ])

  if (!me) {
    return <div className='grid place-items-center py-10'>
      <Spinner />
    </div>
  }

  return <div className='text-center'>
    <LuHourglass className='relaxed mx-auto size-10 text-accent' aria-hidden />
    <h1 className='text-2xl font-bold'>{t('pending.heading')}</h1>
    <p className='compact mt-2 text-sm opacity-70'>{
      t('pending.description', { name: me.user.name })
    }</p>
    <p className='relaxed text-xs opacity-60'>{t('pending.automatic')}</p>
    <Button variant='outline' onPress={() => void signOut()}>
      <span>{t('pending.signOut')}</span>
    </Button>
  </div>
}
