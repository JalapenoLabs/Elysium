// Copyright © 2026 Jalapeno Labs

// Core
import { useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { Navigate, Outlet, useLocation } from 'react-router'
import useSWR from 'swr'

// Redux
import { meLoaded, selectAccessRefusal, selectMe } from '../store/authSlice'
import { useAppDispatch, useAppSelector } from '../store/hooks'

// User interface
import { Button, Spinner } from '@heroui/react'

// Misc
import { getMe } from '../api/routes/authRoutes'
import { startEventStream } from '../realtime/eventStream'
import { getLoginUrl, UrlTree } from '../urls'

// Guards every workspace page. It asks the API who is signed in, then lets them through or
// sends them where they must go first: signing in, the second factor, the waiting page, or
// setting up an authenticator app. Any request that learns the session ended or the account
// changed lands a refusal in Redux (`src/api/index.ts`), and this routes on it the same way.
//
// The event stream opens here, once the person may use the workspace.
export function AuthGate() {
  const { t } = useTranslation([ 'auth', 'common' ])
  const dispatch = useAppDispatch()
  const location = useLocation()
  const me = useAppSelector(selectMe)
  const refusal = useAppSelector(selectAccessRefusal)

  const { error, mutate } = useSWR('v1/me', async () => {
    const response = await getMe()
    dispatch(meLoaded(response))
    return response
  })

  const isOnSecurityPage = location.pathname === UrlTree.settingsSecurity
  const mustEnroll = refusal === 'mfa_enrollment_required' || Boolean(me?.mfaEnrollmentRequired)
  const hasAccess = me?.user.status === 'active' && !mustEnroll && !refusal

  useEffect(() => {
    if (hasAccess) {
      startEventStream()
    }
  }, [ hasAccess ])

  const returnTo = `${location.pathname}${location.search}`

  if (refusal === 'unauthenticated') {
    return <Navigate to={getLoginUrl({ returnTo })} replace />
  }
  if (refusal === 'second_factor_required') {
    return <Navigate to={getLoginUrl({ returnTo, secondFactor: true })} replace />
  }
  if (refusal === 'account_pending' || me?.user.status === 'pending') {
    return <Navigate to={UrlTree.pending} replace />
  }
  if (refusal === 'account_disabled' || me?.user.status === 'disabled') {
    return <Navigate to={getLoginUrl({ notice: 'disabled' })} replace />
  }

  if (!me) {
    return <div className='grid h-dvh place-items-center'>{
      error
        ? <div className='text-center'>
          <p className='compact text-sm opacity-70'>{t('gate.loadError')}</p>
          <Button size='sm' variant='outline' onPress={() => mutate()}>
            <span>{t('common:actions.retry')}</span>
          </Button>
        </div>
        : <Spinner />
    }</div>
  }

  // The security page is the one place someone who must set up an authenticator may be.
  if (mustEnroll && !isOnSecurityPage) {
    return <Navigate to={UrlTree.settingsSecurity} replace />
  }

  return <Outlet />
}
