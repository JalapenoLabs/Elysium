// Copyright © 2026 Jalapeno Labs

// Core
import { useState } from 'react'
import { Navigate } from 'react-router'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../store/hooks'

// UI
import { AuthShell } from '../../components/auth/AuthShell'
import { AuthCard } from '../../components/auth/AuthCard'
import { ClaimInstanceStep } from './ClaimInstanceStep'
import { ClaimOwnerStep } from './ClaimOwnerStep'

// Misc
import { UrlTree } from '../../urls'

// First-run: claim an unclaimed instance in two steps. Step 1 names the workspace;
// step 2 creates the owner account with a chosen sign-in method (passkey, or a
// password with a TOTP second factor). The API persists nothing until the chosen
// credential verifies, so nothing is written if the visitor backs out. This entry
// component owns the wizard step and the instance name; each step owns its inputs.
// See docs/auth-model.md.
export function ClaimInstance() {
  const { t } = useTranslation('auth')
  const status = useAppSelector((state) => state.session.status)
  const instanceClaimed = useAppSelector((state) => state.session.instanceClaimed)

  const [ step, setStep ] = useState<'instance' | 'owner'>('instance')
  const [ orgName, setOrgName ] = useState('')

  // The claim screen is only for a fresh, unclaimed instance. An authenticated user
  // belongs in the app; a claimed-but-anonymous visitor should sign in instead.
  if (status === 'authenticated') {
    return <Navigate to={UrlTree.home} replace />
  }
  if (instanceClaimed === true) {
    return <Navigate to={UrlTree.login} replace />
  }

  return <AuthShell state='idle' showPresence={false}>
    <AuthCard>
      <p className='mb-4 text-xs uppercase tracking-[0.2em] opacity-50'>
        { t('setup.stepLabel', { current: step === 'instance' ? 1 : 2, total: 2 }) }
      </p>
      { step === 'instance'
        ? <ClaimInstanceStep
          orgName={orgName}
          onOrgNameChange={setOrgName}
          onNext={() => setStep('owner')}
        />
        : <ClaimOwnerStep
          orgName={orgName}
          onBack={() => setStep('instance')}
        /> }
    </AuthCard>
  </AuthShell>
}
