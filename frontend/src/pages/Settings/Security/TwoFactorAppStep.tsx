// Copyright © 2026 Jalapeno Labs

import type { FormEvent } from 'react'
import type { AuthenticatorState } from './securityPresentation'

// Core
import { useContext } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { buttonVariants, Form, Link, Spinner } from '@heroui/react'
import { LuArrowLeft } from 'react-icons/lu'
import { AuthenticatorSetup } from './AuthenticatorSetup'
import { PageSaveButton } from '../../../layout/page/PageSaveButton'

// Misc
import { LEAVE_WITHOUT_ASKING, PageSaveStateContext } from '../../../layout/page/pageSave'

type Props = {
  // The app flow's authenticator: the QR code to scan, or enrolled once verified.
  authenticator: AuthenticatorState
  code: string
  onCodeChange: (code: string) => void
  codeError: string | null
  backHref: string
  // The page's save, which Enter runs as the button does.
  onSubmit: (event: FormEvent<HTMLFormElement>) => void
}

// The last step of adding two-factor authentication: scan the app and enter its code. The
// page's own Save, named Turn on 2FA, verifies the app and then saves the backup codes, so the
// button here and the one in the header are one action. Once the app is verified it stays on,
// and the button only saves the codes, should that part have failed.
export function TwoFactorAppStep(props: Props) {
  const { t } = useTranslation([ 'auth', 'common' ])
  const pageSave = useContext(PageSaveStateContext)

  return <Form onSubmit={props.onSubmit} validationBehavior='aria'>
    <div className='relaxed w-full'>
      {props.authenticator.kind === 'available' && <AuthenticatorSetup
        qrCode={props.authenticator.qrCode}
        secretKey={props.authenticator.secretKey}
        code={props.code}
        onCodeChange={props.onCodeChange}
        codeError={props.codeError}
        isDisabled={Boolean(pageSave?.isSaving)}
      />}
      {/* Verified while the save runs on to the codes, which the button then finishes. */}
      {props.authenticator.kind === 'enrolled' && !pageSave?.isSaving && <p className='text-sm'>{
        t('security.twoFactor.appVerified')
      }</p>}
      {(props.authenticator.kind === 'unavailable'
        || (props.authenticator.kind === 'enrolled' && pageSave?.isSaving))
        && <div className='grid place-items-center py-10'>
        <Spinner />
      </div>}
    </div>

    <div className='page-create-footer w-full'>
      <Link
        href={props.backHref}
        routerOptions={{ state: LEAVE_WITHOUT_ASKING }}
        className={buttonVariants({ variant: 'outline' })}
      >
        <LuArrowLeft className='size-4' aria-hidden />
        <span>{t('common:actions.back')}</span>
      </Link>
      {pageSave?.save && <PageSaveButton
        save={pageSave.save}
        isSaving={pageSave.isSaving}
        onPress={() => void pageSave.runSave()}
      />}
    </div>
  </Form>
}
