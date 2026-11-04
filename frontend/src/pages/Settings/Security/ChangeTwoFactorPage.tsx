// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'
import { Navigate } from 'react-router'

// Redux
import { selectMe } from '../../../store/authSlice'
import { useAppSelector } from '../../../store/hooks'

// User interface
import { Button, Spinner } from '@heroui/react'
import { ChangeTwoFactorForm } from './ChangeTwoFactorForm'
import { SettingsPage } from '../SettingsPage'

// Misc
import { useKratosSettingsFlow } from '../../../hooks/useKratosSettingsFlow'
import { useRecentSignIn } from '../../../hooks/useRecentSignIn'
import { UrlTree } from '../../../urls'
import { readAuthenticator } from './securityPresentation'

// `/settings/security/two-factor/edit`: renaming the authenticator app, or moving to a new one.
// Kratos's flow only says whether an app is set up; the move itself goes through the API, which
// keeps the current app until the new one is verified (`ChangeTwoFactorForm`).
export function ChangeTwoFactorPage() {
  const { t } = useTranslation([ 'auth', 'common' ])
  const me = useAppSelector(selectMe)
  const isRecent = useRecentSignIn(UrlTree.settingsTwoFactorEdit)
  const settings = useKratosSettingsFlow(UrlTree.settingsTwoFactorEdit)
  const flow = settings.flow

  // Someone without an app adds one instead.
  if (flow && readAuthenticator(flow).kind !== 'enrolled') {
    return <Navigate to={UrlTree.settingsSecurity} replace />
  }

  return <SettingsPage
    title={t('security.twoFactor.changeTitle')}
    parents={[{ label: t('security.title'), href: UrlTree.settingsSecurity }]}
  >
    {settings.loadFailed && <div className='py-10 text-center'>
      <p className='compact text-sm text-danger'>{t('security.loadError')}</p>
      <Button size='sm' variant='outline' onPress={settings.retry}>
        <span>{t('common:actions.retry')}</span>
      </Button>
    </div>}

    {(!flow || !isRecent || !me) && !settings.loadFailed && <div className='grid place-items-center py-16'>
      <Spinner />
    </div>}

    {flow && isRecent && me && <ChangeTwoFactorForm nickname={me.authenticatorNickname ?? ''} />}
  </SettingsPage>
}
