// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../../store/hooks'
import { selectAccessRefusal, selectMe } from '../../../store/authSlice'

// User interface
import { Alert, Button, Chip, Spinner, Tabs } from '@heroui/react'
import { LookupCodesSection } from './LookupCodesSection'
import { PasskeysSection } from './PasskeysSection'
import { PasswordCard } from './PasswordCard'
import { SessionsSection } from './SessionsSection'
import { SettingsPage } from '../SettingsPage'
import { TwoFactorCard } from './TwoFactorCard'

// Misc
import { useKratosSettingsFlow } from '../../../hooks/useKratosSettingsFlow'
import { useTabParam } from '../../../hooks/useTabParam'
import { UrlTree } from '../../../urls'
import { readPasskeys } from './securityPresentation'
import { useSignedInSessions } from './useSignedInSessions'

const TABS = [ 'general', 'passkeys', 'lookup', 'sessions' ] as const
type Tab = typeof TABS[number]

const tabLabelKeys = {
  general: 'security.tabs.general',
  passkeys: 'security.tabs.passkeys',
  lookup: 'security.tabs.lookup',
  sessions: 'security.tabs.sessions',
} as const satisfies Record<Tab, ParseKeys<'auth'>>

// Security: the password and two-factor authentication on General, then passkeys,
// lookup codes, and where this person is signed in, each on its own tab. Every change runs
// through Kratos. When the workspace requires an authenticator app, this is the one page
// someone without one may open, and it says why.
export function SecurityPage() {
  const { t } = useTranslation([ 'auth', 'common' ])
  const me = useAppSelector(selectMe)
  const refusal = useAppSelector(selectAccessRefusal)
  const settings = useKratosSettingsFlow(UrlTree.settingsSecurity)
  const signedIn = useSignedInSessions()
  const flow = settings.flow
  const mustEnroll = refusal === 'mfa_enrollment_required' || Boolean(me?.mfaEnrollmentRequired)
  // In the address, so a sign-in a change asked for comes back to the tab it left.
  const [ selectedTab, selectTab ] = useTabParam(TABS)

  // Lists show their totals beside their names, once they are known.
  const countByTab: Record<Tab, number | null> = {
    general: null,
    passkeys: flow
      ? readPasskeys(flow).length
      : null,
    lookup: null,
    sessions: signedIn.isLoaded
      ? signedIn.sessions.length
      : null,
  }

  return <SettingsPage title={t('security.title')}>
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

    {flow && <Tabs variant='secondary' selectedKey={selectedTab} onSelectionChange={selectTab}>
      <Tabs.ListContainer className='relaxed'>
        <Tabs.List aria-label={t('security.tabs.label')}>{
          TABS.map((tab) => <Tabs.Tab key={tab} id={tab}>
            <span>{t(tabLabelKeys[tab])}</span>
            {countByTab[tab] !== null && <Chip size='sm' variant='soft' className='ml-2'>{
              countByTab[tab]
            }</Chip>}
            <Tabs.Indicator />
          </Tabs.Tab>)
        }</Tabs.List>
      </Tabs.ListContainer>
      <Tabs.Panel id='general'>
        <PasswordCard flow={flow} onSubmit={settings.submit} />
        <TwoFactorCard flow={flow} />
      </Tabs.Panel>
      <Tabs.Panel id='passkeys'>
        <PasskeysSection flow={flow} onSubmit={settings.submit} />
      </Tabs.Panel>
      <Tabs.Panel id='lookup'>
        <LookupCodesSection flow={flow} onSubmit={settings.submit} />
      </Tabs.Panel>
      <Tabs.Panel id='sessions'>
        <SessionsSection />
      </Tabs.Panel>
    </Tabs>}
  </SettingsPage>
}
