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
  password: string
  flash: StageFlashMessage | null
  onChange: (password: string) => void
  onSubmit: () => void
  onBack: () => void
}

// The password stage, entered like a new page when the server says a password
// is required. Same type-anywhere treatment as the identifier stage, with the
// typed value masked into hero dots.
export function LoginPasswordStage(props: Props) {
  const { t } = useTranslation('auth')
  const isPristine = !props.password

  return <div className='login-stage'>
    <p className='login-stage__identity'>
      { t('login.signingInAs', { email: props.email }) }
    </p>

    <p className={`login-wordmark ${isPristine ? 'login-wordmark--hero' : 'login-wordmark--kicker'}`}>
      { t('login.password.prompt') }
    </p>

    <GhostInput
      masked
      value={props.password}
      label={t('login.password.prompt')}
      autoComplete='current-password'
      shakeNonce={props.flash?.tone === 'warning' ? props.flash.nonce : 0}
      onChange={props.onChange}
      onSubmit={props.onSubmit}
    />

    <p className={`login-stage__hint ${isPristine ? '' : 'login-stage__hint--settled'}`}>
      { t('login.password.hint') }
    </p>

    <StageFlash flash={props.flash} />

    <div className='login-stage__actions'>
      <Button variant='light' onPress={props.onBack}>
        <span>{ t('login.differentEmail') }</span>
      </Button>
    </div>
  </div>
}
