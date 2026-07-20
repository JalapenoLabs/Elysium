// Copyright © 2026 Jalapeno Labs

import type { SecurityActivity } from '../../proto/generated/security_pb'

// Core
import { useCallback, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

// UI
import { ProfileSection } from './sections/ProfileSection'
import { CredentialsSection } from './sections/CredentialsSection'
import { SessionsSection } from './sections/SessionsSection'
import { AccountsSection } from './sections/AccountsSection'
import { LoginHistorySection } from './sections/LoginHistorySection'

// Misc
import { getSecurityActivity } from '../../api/routes/userRoutes'

// Personal account settings on a single page, Stripe's personal-details model:
// every "me" concern (profile, passkeys, two-step, password, sessions, orgs,
// recent sign-ins) as flat sections separated by hairlines, no sub-navigation.
// Organization-level settings live in their own hub (see OrgSettingsLayout).
//
// Sessions and recent sign-ins come from one `/me/security-activity` call owned
// here and shared with both sections, so the page hits the endpoint once.
export function PersonalDetails() {
  const { t } = useTranslation('auth')

  const [ activity, setActivity ] = useState<SecurityActivity | null>(null)

  const loadActivity = useCallback(async () => {
    try {
      setActivity(await getSecurityActivity())
    }
    catch (error) {
      console.debug('security activity unavailable', { error })
    }
  }, [])

  useEffect(() => {
    void loadActivity()
  }, [ loadActivity ])

  return <section className='mx-auto max-w-3xl px-6 py-8'>
    <div className='mb-2'>
      <h1 className='text-2xl'>
        <strong>{ t('settings.personal.title') }</strong>
      </h1>
      <p className='mt-1 text-sm text-default-500'>{ t('settings.personal.subtitle') }</p>
    </div>

    <ProfileSection />
    <CredentialsSection />
    <SessionsSection sessions={activity?.sessions ?? []} onReload={loadActivity} />
    <AccountsSection />
    <LoginHistorySection events={activity?.recentSignIns ?? []} />
  </section>
}
