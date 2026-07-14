// Copyright © 2026 Jalapeno Labs

import type { StageFlashMessage } from '../../components/auth/StageFlash'

// Core
import { useTranslation } from 'react-i18next'

// UI
import { Button } from '@heroui/react'
import { StageFlash } from '../../components/auth/StageFlash'

type Props = {
  email: string
  // True while the native WebAuthn prompt is up; actions hide until it settles.
  isCeremonyActive: boolean
  canUsePassword: boolean
  flash: StageFlashMessage | null
  onRetry: () => void
  onUsePassword: () => void
  onUseRecovery: () => void
  onBack: () => void
}

// The passkey ceremony stage: the OS is showing its native prompt, so this
// screen's whole job is one giant instruction. After a failed ceremony the
// retry and fallback actions appear.
export function LoginPasskeyStage(props: Props) {
  const { t } = useTranslation('auth')

  return <div className='login-stage'>
    <p className='login-stage__identity'>
      { t('login.signingInAs', { email: props.email }) }
    </p>

    <h1 className='login-stage__prompt'>
      { t('login.passkey.prompt') }
    </h1>

    <p className='login-stage__hint login-stage__hint--settled'>
      { t('login.passkey.hint') }
    </p>

    <StageFlash flash={props.flash} />

    { !props.isCeremonyActive
      && <div className='login-stage__actions'>
        <Button color='primary' variant='flat' onPress={props.onRetry}>
          <span>{ t('login.passkey.retry') }</span>
        </Button>
        { props.canUsePassword
          && <Button variant='light' onPress={props.onUsePassword}>
            <span>{ t('login.passkey.usePassword') }</span>
          </Button>
        }
        <Button variant='light' onPress={props.onUseRecovery}>
          <span>{ t('login.passkey.useRecovery') }</span>
        </Button>
        <Button variant='light' onPress={props.onBack}>
          <span>{ t('login.differentEmail') }</span>
        </Button>
      </div>
    }
  </div>
}
