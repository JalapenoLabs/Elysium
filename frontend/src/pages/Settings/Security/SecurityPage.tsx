// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../../store/hooks'
import { selectAccessRefusal, selectMe } from '../../../store/authSlice'

// User interface
import { Alert, Breadcrumbs, Button, Spinner } from '@heroui/react'
import { KratosMessages } from '../../Auth/KratosMessages'
import { AuthenticatorSection } from './AuthenticatorSection'
import { LookupCodesSection } from './LookupCodesSection'
import { PasskeysSection } from './PasskeysSection'
import { PasswordSection } from './PasswordSection'
import { ProfileSection } from './ProfileSection'
import { SessionsSection } from './SessionsSection'

// Misc
import { UrlTree } from '../../../urls'
import { useSettingsFlow } from './useSettingsFlow'

// Sign-in & security: the profile, the password, passkeys, the authenticator app, lookup
// codes, and where this person is signed in. Every change runs through Kratos. When the
// workspace requires an authenticator app, this is the one page someone without one may
// open, and it says why.
export function SecurityPage() {
  const { t } = useTranslation([ 'auth', 'settings', 'common' ])
  const me = useAppSelector(selectMe)
  const refusal = useAppSelector(selectAccessRefusal)
  const settings = useSettingsFlow()
  const flow = settings.flow
  const mustEnroll = refusal === 'mfa_enrollment_required' || Boolean(me?.mfaEnrollmentRequired)

  return <div className='container'>
    <Breadcrumbs className='compact'>
      <Breadcrumbs.Item href={UrlTree.settings}>{t('settings:title')}</Breadcrumbs.Item>
      <Breadcrumbs.Item>{t('security.title')}</Breadcrumbs.Item>
    </Breadcrumbs>
    <h1 className='relaxed text-3xl font-bold'>{
      t('security.title')
    }</h1>

    {mustEnroll && <Alert status='warning' className='relaxed'>
      <Alert.Indicator />
      <Alert.Content>
        <Alert.Title>{t('security.enrollment.title')}</Alert.Title>
        <Alert.Description>{t('security.enrollment.description')}</Alert.Description>
      </Alert.Content>
    </Alert>}

    {settings.loadFailed && <div className='py-10 text-center'>
      <p className='compact text-sm text-danger'>{t('security.loadError')}</p>
      <Button size='sm' variant='outline' onPress={settings.retry}>
        <span>{t('common:actions.retry')}</span>
      </Button>
    </div>}

    {!flow && !settings.loadFailed && <div className='grid place-items-center py-16'>
      <Spinner />
    </div>}

    {flow && <>
      <KratosMessages messages={flow.ui.messages ?? []} />
      {/* Keyed by flow so each panel starts from what Kratos now holds after a change. */}
      <ProfileSection key={`profile-${flow.id}`} flow={flow} onSubmit={settings.submit} />
      <PasswordSection flow={flow} onSubmit={settings.submit} />
      <AuthenticatorSection flow={flow} onSubmit={settings.submit} />
      <LookupCodesSection flow={flow} onSubmit={settings.submit} />
      <PasskeysSection flow={flow} onSubmit={settings.submit} />
      <SessionsSection />
    </>}
  </div>
}
