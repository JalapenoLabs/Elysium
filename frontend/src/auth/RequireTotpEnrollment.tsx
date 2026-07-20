// Copyright © 2026 Jalapeno Labs

import type { FormEvent, ReactNode } from 'react'

// Core
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../store/hooks'

// UI
import { AuthShell } from '../components/auth/AuthShell'
import { AuthCard } from '../components/auth/AuthCard'
import { TotpEnrollCard } from '../components/enrollment/TotpEnrollCard'

// Misc
import { useTotpEnrollment } from './methods/useTotpEnrollment'
import { resolveSession } from './session'

type Props = {
  children: ReactNode
}

// A hard gate: a member of a require-TOTP org who has no authenticator enrolled
// cannot use the app until they set one up (like a "set up 2-step now"
// interstitial). The server signals this via `/context`'s
// `totp_enrollment_required`; confirming enrollment re-resolves the session,
// which clears the flag and drops the gate. See docs/auth-model.md.
export function RequireTotpEnrollment(props: Props) {
  const { t } = useTranslation('auth')
  const required = useAppSelector((state) => state.sessionContext.totpEnrollmentRequired)

  const enrollment = useTotpEnrollment()
  const { begin } = enrollment
  const [ code, setCode ] = useState('')

  // Begin enrollment ONLY when the gate is actually shown. This component wraps
  // the whole app, so an unconditional auto-start would mint a throwaway TOTP
  // secret on every page load; gating it on `required` means the server is only
  // asked when the user really has to enroll.
  useEffect(() => {
    if (required) {
      void begin()
    }
  }, [ required, begin ])

  if (!required) {
    return props.children
  }

  async function onConfirm(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const confirmed = await enrollment.confirm(code)
    if (confirmed) {
      // The flag lives on the session context; re-resolving reseeds it as false
      // and the gate falls away on the next render.
      await resolveSession()
    }
  }

  const errorMessage = enrollment.error === 'invalidCode'
    ? t('gate.totp.invalidCode')
    : enrollment.error === 'failed'
      ? t('gate.totp.failed')
      : null

  return <AuthShell state='needsInput'>
    <AuthCard>
      <p className='mb-2 text-xs uppercase tracking-widest opacity-60'>
        { t('gate.totp.eyebrow') }
      </p>
      <TotpEnrollCard
        provisioningUri={enrollment.provisioningUri}
        code={code}
        onCodeChange={setCode}
        onSubmit={onConfirm}
        title={t('gate.totp.title')}
        subtitle={t('gate.totp.subtitle')}
        manualHint={t('gate.totp.manualHint')}
        codeLabel={t('gate.totp.codeLabel')}
        actionLabel={t('gate.totp.action')}
        requiredReason={t('gate.totp.required')}
        errorMessage={errorMessage}
        isBusy={enrollment.isBusy}
      />
    </AuthCard>
  </AuthShell>
}
