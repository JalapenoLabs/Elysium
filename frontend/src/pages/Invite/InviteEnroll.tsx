// Copyright © 2026 Jalapeno Labs

import type { FormEvent } from 'react'
import type { InvitePreview } from './InviteAcceptance'

// Core
import { useState } from 'react'
import { useNavigate } from 'react-router'
import { useTranslation } from 'react-i18next'

// UI
import { Button, Input } from '@heroui/react'
import { AuthShell } from '../../components/auth/AuthShell'
import { AuthCard } from '../../components/auth/AuthCard'
import { MethodChooser } from '../../components/enrollment/MethodChooser'
import { TotpEnrollCard } from '../../components/enrollment/TotpEnrollCard'
import { RecoveryCodesCard } from '../../components/enrollment/RecoveryCodesCard'
import { SubmitButton } from '../../components/form/SubmitButton'

// Utility
import { HTTPError } from 'ky'

// Misc
import { useEnrollCredential } from '../../auth/methods/useEnrollCredential'
import { inviteAcceptBegin, inviteAcceptFinish } from '../../api/routes/authRoutes'
import { resolveSession } from '../../auth/session'
import { UrlTree } from '../../urls'

type Props = {
  token: string
  preview: InvitePreview
}

// The stages of accepting as a new user: identity + method, then (for the
// password path) choose a password and confirm a TOTP code, then save the
// one-time recovery codes before entering onboarding.
type Stage = 'identity' | 'password' | 'totp' | 'recovery'

// The methods an invitee may enroll with (mirrors first-run claim).
const INVITE_METHODS = [ 'passkey', 'password' ] as const

const MINIMUM_PASSWORD_LENGTH = 8

// Anonymous invite acceptance: enroll a credential for a brand-new profile via
// the shared enrollment machine, against the invite endpoints. A 409 from begin
// means the email already has a profile; the visitor signs in and reopens the
// link to join instead.
export function InviteEnroll(props: Props) {
  const { t } = useTranslation([ 'auth', 'common' ])
  const navigate = useNavigate()

  const [ stage, setStage ] = useState<Stage>('identity')
  // An email-pinned invite locks the field; open invites collect it.
  const [ email, setEmail ] = useState(props.preview.email)
  const [ givenName, setGivenName ] = useState('')
  const [ familyName, setFamilyName ] = useState('')
  const [ displayName, setDisplayName ] = useState('')
  const [ password, setPassword ] = useState('')
  const [ confirmPassword, setConfirmPassword ] = useState('')
  const [ totpCode, setTotpCode ] = useState('')
  const [ localError, setLocalError ] = useState<string | null>(null)
  const [ accountExists, setAccountExists ] = useState(false)

  const enroll = useEnrollCredential({
    begin: async (method, opaqueRequest) => {
      try {
        return await inviteAcceptBegin(props.token, {
          email: email.trim(),
          given_name: givenName.trim(),
          family_name: familyName.trim(),
          display_name: displayName.trim(),
          method,
          opaque_registration_request: opaqueRequest,
        })
      }
      catch (error) {
        // 409: this email already has a profile; joining is a sign-in away.
        if (error instanceof HTTPError && error.response.status === 409) {
          setAccountExists(true)
        }
        throw error
      }
    },
    finish: (body) => inviteAcceptFinish(body),
  })

  const identityReady = Boolean(givenName.trim()) && Boolean(email.trim())

  async function onChooseMethod(kind: string) {
    if (!identityReady) {
      return
    }
    setLocalError(null)
    if (kind === 'password') {
      setStage('password')
      return
    }
    const enrolled = await enroll.enrollPasskey()
    if (enrolled) {
      setStage('recovery')
    }
  }

  async function onPasswordNext(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    setLocalError(null)
    if (password.length < MINIMUM_PASSWORD_LENGTH) {
      setLocalError(t('auth:invite.password.tooShort'))
      return
    }
    if (password !== confirmPassword) {
      setLocalError(t('auth:invite.password.mismatch'))
      return
    }
    const begun = await enroll.beginPassword(password)
    if (begun) {
      setStage('totp')
    }
  }

  async function onTotpFinish(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const enrolled = await enroll.completePassword(totpCode)
    if (enrolled) {
      setStage('recovery')
    }
  }

  // The finish set the session cookie; resolve it and walk into onboarding so
  // the new member finishes their profile.
  async function onRecoveryContinue() {
    await resolveSession()
    navigate(UrlTree.onboardingProfile)
  }

  let errorMessage = localError
  if (!errorMessage && accountExists) {
    errorMessage = t('auth:invite.accept.accountExists')
  }
  if (!errorMessage && enroll.error === 'invalidCode') {
    errorMessage = t('auth:invite.totp.invalidCode')
  }
  if (!errorMessage && enroll.error === 'failed') {
    errorMessage = t('auth:invite.accept.failed')
  }

  let content = null

  if (stage === 'recovery') {
    content = <RecoveryCodesCard
      codes={enroll.recoveryCodes ?? []}
      title={t('auth:invite.recovery.title')}
      subtitle={t('auth:invite.recovery.subtitle')}
      copyLabel={t('auth:invite.recovery.copy')}
      downloadLabel={t('auth:invite.recovery.download')}
      actionLabel={t('auth:invite.recovery.action')}
      onContinue={onRecoveryContinue}
    />
  }
  else if (stage === 'totp') {
    content = <TotpEnrollCard
      provisioningUri={enroll.provisioningUri}
      code={totpCode}
      onCodeChange={setTotpCode}
      onSubmit={onTotpFinish}
      title={t('auth:invite.totp.title')}
      subtitle={t('auth:invite.totp.subtitle')}
      manualHint={t('auth:invite.totp.manualHint')}
      codeLabel={t('auth:invite.totp.codeLabel')}
      actionLabel={t('auth:invite.totp.action')}
      requiredReason={t('auth:invite.totp.required')}
      errorMessage={errorMessage}
      isBusy={enroll.isBusy}
    />
  }
  else if (stage === 'password') {
    content = <>
      <h1 className='text-2xl'>
        <strong>{ t('auth:invite.password.title') }</strong>
      </h1>
      <form onSubmit={onPasswordNext}>
        <div className='mt-6'>
          <div className='compact'>
            <Input
              autoFocus
              type='password'
              label={t('auth:invite.password.passwordLabel')}
              value={password}
              onChange={(event) => setPassword(event.currentTarget.value)}
            />
          </div>
          <div className='compact'>
            <Input
              type='password'
              label={t('auth:invite.password.confirmLabel')}
              value={confirmPassword}
              onChange={(event) => setConfirmPassword(event.currentTarget.value)}
            />
          </div>
        </div>
        { errorMessage
          ? <p className='mt-3 text-sm text-danger'>{ errorMessage }</p>
          : null }
        <SubmitButton
          className='mt-6'
          label={t('auth:invite.password.action')}
          isValid={Boolean(password) && Boolean(confirmPassword)}
          invalidReason={t('auth:invite.password.required')}
          isSubmitting={enroll.isBusy}
        />
      </form>
      <Button
        className='mt-3 w-full'
        variant='light'
        onPress={() => setStage('identity')}
      >
        <span>{ t('common:actions.back') }</span>
      </Button>
    </>
  }
  else {
    content = <>
      <p className='mb-2 text-xs uppercase tracking-widest opacity-60'>
        { t('auth:invite.accept.eyebrow') }
      </p>
      <h1 className='text-2xl'>
        <strong>{ t('auth:invite.accept.title', { orgName: props.preview.org_name }) }</strong>
      </h1>
      <p className='mt-3 opacity-80'>
        { t('auth:invite.accept.body', {
          roleName: t(`auth:settings.members.roles.${props.preview.role}`, props.preview.role),
        }) }
      </p>

      <div className='mt-6'>
        <div className='compact level items-start'>
          <Input
            autoFocus
            className='w-full'
            label={t('auth:invite.accept.givenNameLabel')}
            value={givenName}
            onChange={(event) => setGivenName(event.currentTarget.value)}
          />
          <Input
            className='w-full'
            label={t('auth:invite.accept.familyNameLabel')}
            value={familyName}
            onChange={(event) => setFamilyName(event.currentTarget.value)}
          />
        </div>
        <div className='compact'>
          <Input
            label={t('auth:invite.accept.displayNameLabel')}
            description={t('auth:invite.accept.displayNameHint')}
            value={displayName}
            onChange={(event) => setDisplayName(event.currentTarget.value)}
          />
        </div>
        <div className='compact'>
          <Input
            type='email'
            label={t('auth:invite.accept.emailLabel')}
            isDisabled={Boolean(props.preview.email)}
            value={email}
            onChange={(event) => setEmail(event.currentTarget.value)}
          />
        </div>
      </div>

      <p className='mt-6 text-sm opacity-70'>{ t('auth:invite.accept.methodTitle') }</p>
      <MethodChooser
        methods={INVITE_METHODS}
        isDisabled={!identityReady || enroll.isBusy}
        onChoose={onChooseMethod}
      />

      { errorMessage
        ? <p className='mt-3 text-sm text-danger'>{ errorMessage }</p>
        : null }
      { accountExists
        ? <Button
          className='mt-3 w-full'
          color='primary'
          variant='flat'
          onPress={() => navigate(UrlTree.login)}
        >
          <span>{ t('auth:invite.accept.signInFirst') }</span>
        </Button>
        : null }
    </>
  }

  return <AuthShell state={stage === 'recovery' ? 'success' : 'idle'}>
    <AuthCard>
      { content }
    </AuthCard>
  </AuthShell>
}
