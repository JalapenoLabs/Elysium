// Copyright © 2026 Jalapeno Labs

import type { FormEvent } from 'react'

// UI
import { Input } from '@heroui/react'
import { QRCodeSVG } from 'qrcode.react'
import { SubmitButton } from '../form/SubmitButton'

type Props = {
  // The `otpauth://` URI to render as a QR; null while it is still loading.
  provisioningUri: string | null
  code: string
  onCodeChange: (code: string) => void
  onSubmit: (event: FormEvent<HTMLFormElement>) => void
  title: string
  subtitle: string
  manualHint: string
  codeLabel: string
  actionLabel: string
  requiredReason: string
  errorMessage?: string | null
  isBusy?: boolean
}

// The TOTP enrollment step: a QR of the provisioning URI, the setup key printed
// underneath for authenticators that cannot scan, and the confirming code field.
// Shared by first-run claim and invite acceptance. Copy is passed in so each flow
// words it. See docs/auth-model.md.
export function TotpEnrollCard(props: Props) {
  // The base32 secret from the otpauth URI, shown for manual entry.
  const manualKey = props.provisioningUri
    ? new URLSearchParams(props.provisioningUri.split('?')[1] ?? '').get('secret')
    : null

  return <>
    <h1 className='text-2xl'>
      <strong>{ props.title }</strong>
    </h1>
    <p className='mt-3 opacity-80'>{ props.subtitle }</p>

    { props.provisioningUri
      ? <div className='mt-6 flex justify-center rounded-xl bg-white p-4'>
        <QRCodeSVG value={props.provisioningUri} size={180} />
      </div>
      : null }
    { manualKey
      ? <p className='mt-3 break-all text-center text-xs opacity-70'>
        { props.manualHint } <code>{ manualKey }</code>
      </p>
      : null }

    <form onSubmit={props.onSubmit}>
      <div className='mt-6 compact'>
        <Input
          label={props.codeLabel}
          inputMode='numeric'
          autoComplete='one-time-code'
          value={props.code}
          onChange={(event) => props.onCodeChange(event.currentTarget.value)}
        />
      </div>
      { props.errorMessage
        ? <p className='mt-3 text-sm text-danger'>{ props.errorMessage }</p>
        : null }
      <SubmitButton
        className='mt-6'
        label={props.actionLabel}
        isValid={props.code.trim().length === 6}
        invalidReason={props.requiredReason}
        isSubmitting={props.isBusy}
      />
    </form>
  </>
}
