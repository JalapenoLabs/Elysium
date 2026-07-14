// Copyright © 2026 Jalapeno Labs

import type { FormEvent } from 'react'

// Core
import { useEffect, useRef, useState } from 'react'
import { useNavigate } from 'react-router'
import { useTranslation } from 'react-i18next'

// UI
import { Button, Input } from '@heroui/react'
import { SubmitButton } from '../../components/form/SubmitButton'
import { MethodChooser } from '../../components/enrollment/MethodChooser'
import { TotpEnrollCard } from '../../components/enrollment/TotpEnrollCard'
import { RecoveryCodesCard } from '../../components/enrollment/RecoveryCodesCard'

// Misc
import { useClaimInstance } from '../../auth/methods/useClaimInstance'
import { authMethodByKind } from '../../auth/methods/registry'
import { resolveSession } from '../../auth/session'
import { UrlTree } from '../../urls'

type Props = {
  orgName: string
  onBack: () => void
}

// The claim wizard's internal stage: pick a method, then (for the password path)
// choose a password and confirm a TOTP code, and finally save recovery codes
// before entering the app.
type Stage = 'choose' | 'password' | 'totp' | 'recovery'

// The two methods the first-run claim supports today (magic-link enrollment is not
// built). Rendered as choosable cards by the shared MethodChooser.
const CLAIM_METHODS = [ 'passkey', 'password' ] as const

// Step 2 of the claim wizard: the owner's identity and their chosen sign-in method.
//
// The passkey path enrolls and claims in one tap; the password path collects a
// password, then shows a TOTP QR and confirms a code. Either way the server writes
// nothing until the credential verifies (see docs/auth-model.md), so backing out
// leaves the instance unclaimed. The enrollment machinery and the method / TOTP /
// recovery-code UI are shared (see `auth/methods/useEnrollCredential` and
// `components/enrollment/*`) with invite acceptance.
export function ClaimOwnerStep(props: Props) {
  const { t } = useTranslation([ 'auth', 'common' ])
  const navigate = useNavigate()

  const [ givenName, setGivenName ] = useState('')
  const [ familyName, setFamilyName ] = useState('')
  const [ displayName, setDisplayName ] = useState('')
  const [ email, setEmail ] = useState('')
  const [ stage, setStage ] = useState<Stage>('choose')
  const [ password, setPassword ] = useState('')
  const [ confirmPassword, setConfirmPassword ] = useState('')
  const [ code, setCode ] = useState('')
  const [ localError, setLocalError ] = useState<string | null>(null)

  const enroll = useClaimInstance({ orgName: props.orgName, email, givenName, familyName, displayName })

  const nameRef = useRef<HTMLInputElement>(null)
  useEffect(() => {
    nameRef.current?.focus()
  }, [])

  // Given name + email are the minimum to create the account; display name is
  // optional (the server falls back to the given name).
  const identityReady = Boolean(givenName.trim() && email.trim())

  async function finishSuccess() {
    await resolveSession()
    navigate(UrlTree.onboarding)
  }

  async function onChoose(kind: 'passkey' | 'password' | 'magic_link') {
    if (!identityReady) {
      return
    }
    if (kind === 'passkey') {
      const claimed = await enroll.enrollPasskey()
      if (claimed) {
        setStage('recovery')
      }
      return
    }
    setStage('password')
  }

  async function onPasswordNext(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    setLocalError(null)
    if (password.length < 8) {
      setLocalError(t('auth:setup.password.tooShort'))
      return
    }
    if (password !== confirmPassword) {
      setLocalError(t('auth:setup.password.mismatch'))
      return
    }
    const started = await enroll.beginPassword(password)
    if (started) {
      setStage('totp')
    }
  }

  async function onTotpFinish(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const claimed = await enroll.completePassword(code)
    if (claimed) {
      setStage('recovery')
    }
  }

  // Map the hook's semantic error code to copy; local validation wins when set.
  let errorMessage = localError
  if (!errorMessage && enroll.error === 'invalidCode') {
    errorMessage = t('auth:setup.totp.invalidCode')
  }
  if (!errorMessage && enroll.error === 'failed') {
    errorMessage = t('auth:setup.step2.failed')
  }

  if (stage === 'recovery') {
    return <>
      <p className='mb-2 text-xs uppercase tracking-widest opacity-60'>
        { t('auth:setup.step2.eyebrow') }
      </p>
      <RecoveryCodesCard
        codes={enroll.recoveryCodes ?? []}
        title={t('auth:setup.recovery.title')}
        subtitle={t('auth:setup.recovery.subtitle')}
        copyLabel={t('auth:setup.recovery.copy')}
        actionLabel={t('auth:setup.recovery.action')}
        isBusy={enroll.isBusy}
        onContinue={() => {
          void finishSuccess()
        }}
      />
    </>
  }

  if (stage === 'totp') {
    return <>
      <p className='mb-2 text-xs uppercase tracking-widest opacity-60'>
        { t('auth:setup.step2.eyebrow') }
      </p>
      <TotpEnrollCard
        provisioningUri={enroll.provisioningUri}
        code={code}
        onCodeChange={setCode}
        onSubmit={onTotpFinish}
        title={t('auth:setup.totp.title')}
        subtitle={t('auth:setup.totp.subtitle')}
        manualHint={t('auth:setup.totp.manualHint')}
        codeLabel={t('auth:setup.totp.codeLabel')}
        actionLabel={t('auth:setup.totp.action')}
        requiredReason={t('auth:setup.totp.required')}
        errorMessage={errorMessage}
        isBusy={enroll.isBusy}
      />
    </>
  }

  if (stage === 'password') {
    return <>
      <p className='mb-2 text-xs uppercase tracking-widest opacity-60'>
        { t('auth:setup.step2.eyebrow') }
      </p>
      <h1 className='text-2xl'>
        <strong>{ t(`auth:${authMethodByKind.password.titleKey}`) }</strong>
      </h1>

      <form onSubmit={onPasswordNext}>
        <div className='mt-6'>
          <div className='compact'>
            <Input
              type='password'
              label={t('auth:setup.password.passwordLabel')}
              autoComplete='new-password'
              value={password}
              onChange={(event) => setPassword(event.currentTarget.value)}
            />
          </div>
          <div className='compact'>
            <Input
              type='password'
              label={t('auth:setup.password.confirmLabel')}
              autoComplete='new-password'
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
          label={t('auth:setup.password.action')}
          isValid={Boolean(password && confirmPassword)}
          invalidReason={t('auth:setup.password.passwordLabel')}
          isSubmitting={enroll.isBusy}
        />
        <Button className='mt-3 w-full' variant='light' onPress={() => setStage('choose')}>
          <span>{ t('auth:setup.step2.back') }</span>
        </Button>
      </form>
    </>
  }

  // stage === 'choose'
  return <>
    <p className='mb-2 text-xs uppercase tracking-widest opacity-60'>
      { t('auth:setup.step2.eyebrow') }
    </p>
    <h1 className='text-2xl'>
      <strong>{ t('auth:setup.step2.title') }</strong>
    </h1>
    <p className='mt-3 opacity-80'>{ t('auth:setup.step2.subtitle') }</p>

    <div className='mt-6'>
      <div className='level w-full items-start'>
        <div className='compact w-full'>
          <Input
            ref={nameRef}
            label={t('auth:setup.step2.givenNameLabel')}
            placeholder={t('auth:setup.step2.givenNamePlaceholder')}
            value={givenName}
            onChange={(event) => setGivenName(event.currentTarget.value)}
          />
        </div>
        <div className='compact w-full'>
          <Input
            label={t('auth:setup.step2.familyNameLabel')}
            placeholder={t('auth:setup.step2.familyNamePlaceholder')}
            value={familyName}
            onChange={(event) => setFamilyName(event.currentTarget.value)}
          />
        </div>
      </div>
      <div className='compact'>
        <Input
          label={t('auth:setup.step2.displayNameLabel')}
          placeholder={t('auth:setup.step2.displayNamePlaceholder')}
          description={t('auth:setup.step2.displayNameHint')}
          value={displayName}
          onChange={(event) => setDisplayName(event.currentTarget.value)}
        />
      </div>
      <div className='compact'>
        <Input
          type='email'
          label={t('auth:setup.step2.emailLabel')}
          placeholder={t('auth:setup.step2.emailPlaceholder')}
          value={email}
          onChange={(event) => setEmail(event.currentTarget.value)}
        />
      </div>
    </div>

    <h2 className='mt-6 text-sm uppercase tracking-widest opacity-60'>
      { t('auth:setup.step2.methodTitle') }
    </h2>
    <MethodChooser
      methods={CLAIM_METHODS}
      isDisabled={!identityReady || enroll.isBusy}
      onChoose={onChoose}
    />

    { errorMessage
      ? <p className='mt-3 text-sm text-danger'>{ errorMessage }</p>
      : null }

    <Button className='mt-6 w-full' variant='light' onPress={props.onBack}>
      <span>{ t('auth:setup.step2.back') }</span>
    </Button>
  </>
}
