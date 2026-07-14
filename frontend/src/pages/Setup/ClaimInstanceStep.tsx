// Copyright © 2026 Jalapeno Labs

import type { FormEvent } from 'react'

// Core
import { useEffect, useRef } from 'react'
import { useTranslation } from 'react-i18next'

// UI
import { Input } from '@heroui/react'
import { SubmitButton } from '../../components/form/SubmitButton'

type Props = {
  orgName: string
  onOrgNameChange: (value: string) => void
  onNext: () => void
}

// Step 1 of the claim wizard: name the workspace. Nothing is sent yet; this only
// gathers the instance details, which the owner step submits atomically with the
// chosen credential. See docs/auth-model.md.
export function ClaimInstanceStep(props: Props) {
  const { t } = useTranslation([ 'auth', 'common' ])

  const orgRef = useRef<HTMLInputElement>(null)
  useEffect(() => {
    orgRef.current?.focus()
  }, [])

  const canSubmit = Boolean(props.orgName.trim())

  function onSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (canSubmit) {
      props.onNext()
    }
  }

  return <>
    <p className='mb-2 text-xs uppercase tracking-widest opacity-60'>
      { t('auth:setup.step1.eyebrow') }
    </p>
    <h1 className='text-2xl'>
      <strong>{ t('auth:setup.step1.title', { appName: t('common:appName') }) }</strong>
    </h1>
    <p className='mt-3 opacity-80'>{ t('auth:setup.step1.subtitle') }</p>

    <form onSubmit={onSubmit}>
      <div className='mt-6 compact'>
        <Input
          ref={orgRef}
          label={t('auth:setup.step1.orgNameLabel')}
          placeholder={t('auth:setup.step1.orgNamePlaceholder')}
          value={props.orgName}
          onChange={(event) => props.onOrgNameChange(event.currentTarget.value)}
        />
      </div>
      <SubmitButton
        className='mt-6'
        label={t('auth:setup.step1.action')}
        isValid={canSubmit}
        invalidReason={t('auth:setup.step1.required')}
      />
    </form>
  </>
}
