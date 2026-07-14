// Copyright © 2026 Jalapeno Labs

import type { StageFlashMessage } from '../../components/auth/StageFlash'

// Core
import { useTranslation } from 'react-i18next'

// UI
import { GhostInput } from '../../components/auth/GhostInput'
import { StageFlash } from '../../components/auth/StageFlash'

type Props = {
  email: string
  code: string
  flash: StageFlashMessage | null
  onChange: (code: string) => void
  onSubmit: () => void
}

// The second-factor stage: the password verified and the API wants the
// six-digit TOTP code before it mints the session. Same hero treatment; the
// digits stay visible since one-time codes aren't secrets worth masking.
export function LoginTotpStage(props: Props) {
  const { t } = useTranslation('auth')
  const isPristine = !props.code

  return <div className='login-stage'>
    <p className='login-stage__identity'>
      { t('login.signingInAs', { email: props.email }) }
    </p>

    <p className={`login-wordmark ${isPristine ? 'login-wordmark--hero' : 'login-wordmark--kicker'}`}>
      { t('login.totp.prompt') }
    </p>

    <GhostInput
      value={props.code}
      label={t('login.totp.prompt')}
      inputMode='numeric'
      autoComplete='one-time-code'
      shakeNonce={props.flash?.tone === 'warning' ? props.flash.nonce : 0}
      onChange={props.onChange}
      onSubmit={props.onSubmit}
    />

    <p className={`login-stage__hint ${isPristine ? '' : 'login-stage__hint--settled'}`}>
      { t('login.totp.hint') }
    </p>

    <StageFlash flash={props.flash} />
  </div>
}
