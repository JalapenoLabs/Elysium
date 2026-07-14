// Copyright © 2026 Jalapeno Labs

import type { StageFlashMessage } from '../../components/auth/StageFlash'

// Core
import { useTranslation } from 'react-i18next'

// UI
import { Button } from '@heroui/react'
import { GhostInput } from '../../components/auth/GhostInput'
import { StageFlash } from '../../components/auth/StageFlash'

type Props = {
  email: string
  code: string
  flash: StageFlashMessage | null
  onChange: (code: string) => void
  onSubmit: () => void
  onBack: () => void
}

// The lost-device stage: redeem a one-time recovery code for a session. Success
// routes into the Security Center so a fresh passkey gets enrolled right away.
export function LoginRecoveryStage(props: Props) {
  const { t } = useTranslation('auth')
  const isPristine = !props.code

  return <div className='login-stage'>
    <p className='login-stage__identity'>
      { t('login.signingInAs', { email: props.email }) }
    </p>

    <p className={`login-wordmark ${isPristine ? 'login-wordmark--hero' : 'login-wordmark--kicker'}`}>
      { t('login.recovery.prompt') }
    </p>

    <GhostInput
      value={props.code}
      label={t('login.recovery.prompt')}
      autoComplete='off'
      shakeNonce={props.flash?.tone === 'warning' ? props.flash.nonce : 0}
      onChange={props.onChange}
      onSubmit={props.onSubmit}
    />

    <p className={`login-stage__hint ${isPristine ? '' : 'login-stage__hint--settled'}`}>
      { t('login.recovery.hint') }
    </p>

    <StageFlash flash={props.flash} />

    <div className='login-stage__actions'>
      <Button variant='light' onPress={props.onBack}>
        <span>{ t('login.differentEmail') }</span>
      </Button>
    </div>
  </div>
}
