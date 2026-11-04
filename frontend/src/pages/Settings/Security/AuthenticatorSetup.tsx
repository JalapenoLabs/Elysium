// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { AuthenticatorCodeInput } from '../../../components/AuthenticatorCodeInput'
import { CopyButton } from '../../../components/CopyButton'

type Props = {
  // The QR code to scan, as an image URL.
  qrCode: string
  // The same secret as a key to type into an app that cannot scan.
  secretKey: string
  code: string
  onCodeChange: (code: string) => void
  // Why the last code was refused, shown under it.
  codeError: string | null
  isDisabled: boolean
}

// Setting an authenticator app up: the QR code beside its key, which copies, then the six-digit
// code the app shows once it holds the secret. The same for a first app, whose secret Kratos
// makes, and for moving to a new one, whose secret Elysium holds until the code checks out.
export function AuthenticatorSetup(props: Props) {
  const { t } = useTranslation('auth')

  return <div className='flex flex-col gap-6 sm:flex-row sm:items-start'>
    <img
      src={props.qrCode}
      alt={t('security.twoFactor.qrAlt')}
      className='size-44 shrink-0 rounded-lg bg-white p-2'
    />
    <div className='min-w-0'>
      <p className='compact text-sm'>{t('security.twoFactor.scan')}</p>
      <p className='compact text-xs opacity-70'>{t('security.twoFactor.manualKey')}</p>
      <div className='relaxed flex items-center gap-2'>
        <code className='min-w-0 break-all rounded-md bg-surface-secondary px-3 py-2 text-sm'>{
          props.secretKey
        }</code>
        <CopyButton value={props.secretKey} label={t('security.twoFactor.copyKey')} />
      </div>
      <p className='compact text-sm'>{t('security.twoFactor.enterCode')}</p>
      <AuthenticatorCodeInput
        // Shown once the app is to be set up, so its code is what is typed next.
        shouldFocus
        id='two-factor-code'
        label={t('fields.authenticatorCode')}
        value={props.code}
        isDisabled={props.isDisabled}
        isInvalid={Boolean(props.codeError)}
        onChange={props.onCodeChange}
      />
      {props.codeError && <p className='mt-2 text-xs text-danger'>{props.codeError}</p>}
    </div>
  </div>
}
