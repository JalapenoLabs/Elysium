// Copyright © 2026 Jalapeno Labs

import type { StageFlashMessage } from '../../components/auth/StageFlash'

// Core
import { useTranslation } from 'react-i18next'

// UI
import { GhostInput } from '../../components/auth/GhostInput'
import { StageFlash } from '../../components/auth/StageFlash'

type Props = {
  email: string
  flash: StageFlashMessage | null
  onChange: (email: string) => void
  onSubmit: () => void
}

// The gate's opening stage. Pristine, it is a title screen: the wordmark is the
// hero with a "just start typing" pulse beneath. The first keystroke recedes
// the wordmark into a kicker and the typed email becomes the hero text.
export function LoginIdentifierStage(props: Props) {
  const { t } = useTranslation([ 'auth', 'common' ])
  const isPristine = !props.email

  return <div className='login-stage'>
    <p className={`login-wordmark ${isPristine ? 'login-wordmark--hero' : 'login-wordmark--kicker'}`}>
      { t('common:appName') }
    </p>

    <GhostInput
      value={props.email}
      label={t('auth:login.identifierLabel')}
      inputMode='email'
      autoComplete='email'
      shakeNonce={props.flash?.tone === 'warning' ? props.flash.nonce : 0}
      onChange={props.onChange}
      onSubmit={props.onSubmit}
    />

    <p className={`login-stage__hint ${isPristine ? '' : 'login-stage__hint--settled'}`}>
      { isPristine
        ? t('auth:login.hint')
        : t('auth:login.enterHint') }
    </p>

    <StageFlash flash={props.flash} />
  </div>
}
