// Copyright © 2026 Jalapeno Labs

import type { FormEvent } from 'react'
import type { AuthenticatorReplacement } from '../../../api/routes/authRoutes'

// Core
import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'
import { mutate } from 'swr'

// User interface
import { Button, Form, Input, Label, TextField, Tooltip } from '@heroui/react'
import { LuSmartphone } from 'react-icons/lu'
import { AuthenticatorSetup } from './AuthenticatorSetup'

// Misc
import { getApiErrorCode } from '../../../api/errors'
import {
  confirmAuthenticatorReplacement,
  startAuthenticatorReplacement,
} from '../../../api/routes/authRoutes'
import { AUTHENTICATOR_NICKNAME_MAX_LENGTH } from '../../../constants'
import { LEAVE_WITHOUT_ASKING, usePageSave } from '../../../layout/page/pageSave'
import { getLoginUrl, UrlTree } from '../../../urls'
import { storeAuthenticatorNickname } from './twoFactorNickname'
import { toast } from '../../../toast/toast'

// Authenticator apps show six digits.
const CODE_LENGTH = 6

type Props = {
  // What the app is called now.
  nickname: string
}

// Changing two-factor authentication: the app's nickname, saved by the page's Save, and moving to
// a new app. Moving shows a new QR code the API made and holds; the current app keeps working
// until a code from the new one is entered and the switch lands, so 2FA is never off on the way.
// Walking away before that leaves the current app as it was, and the held one expires.
export function ChangeTwoFactorForm(props: Props) {
  const { t } = useTranslation([ 'auth', 'common' ])
  const navigate = useNavigate()
  const [ nickname, setNickname ] = useState(props.nickname)
  const [ replacement, setReplacement ] = useState<AuthenticatorReplacement | null>(null)
  const [ isStarting, setIsStarting ] = useState(false)
  const [ code, setCode ] = useState('')
  const [ codeError, setCodeError ] = useState<string | null>(null)
  const [ isSwitching, setIsSwitching ] = useState(false)
  const nicknameRef = useRef<HTMLInputElement>(null)
  const trimmed = nickname.trim()
  const canSwitch = code.length === CODE_LENGTH && Boolean(trimmed) && !isSwitching

  useEffect(() => {
    nicknameRef.current?.focus()
  }, [])

  // A move needs a recent sign-in, which the page checked when it opened; one that aged since
  // signs in again and comes back here.
  function signInAgain() {
    window.location.assign(getLoginUrl({ returnTo: UrlTree.settingsTwoFactorEdit, refresh: true }))
  }

  async function rename() {
    try {
      await storeAuthenticatorNickname(trimmed)
      toast.success(t('security.twoFactor.nicknameSaved'))
      void navigate(UrlTree.settingsSecurity, { state: LEAVE_WITHOUT_ASKING })
      return true
    }
    catch (error) {
      console.debug('ChangeTwoFactorForm could not save the nickname', { error })
      toast.danger(t('common:errors.unexpected'))
      return false
    }
  }

  const onSubmit = usePageSave({
    save: rename,
    isDirty: trimmed !== props.nickname,
    blockedReason: trimmed
      ? null
      : t('security.twoFactor.nameRequired'),
  })

  async function start() {
    setIsStarting(true)
    try {
      setReplacement(await startAuthenticatorReplacement())
      setCode('')
      setCodeError(null)
    }
    catch (error) {
      if (getApiErrorCode(error) === 'recent_sign_in_required') {
        signInAgain()
        return
      }
      console.debug('ChangeTwoFactorForm could not start a new app', { error })
      toast.danger(t('common:errors.unexpected'))
    }
    finally {
      setIsStarting(false)
    }
  }

  async function switchApps(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!canSwitch) {
      console.debug('ChangeTwoFactorForm kept the current app: the switch is not ready', { codeLength: code.length })
      return
    }
    setIsSwitching(true)
    try {
      await confirmAuthenticatorReplacement({ code, nickname: trimmed })
      void mutate('v1/me')
      toast.success(t('security.twoFactor.switched', { name: trimmed }))
      void navigate(UrlTree.settingsSecurity, { state: LEAVE_WITHOUT_ASKING })
    }
    catch (error) {
      const errorCode = getApiErrorCode(error)
      if (errorCode === 'recent_sign_in_required') {
        signInAgain()
        return
      }
      if (errorCode === 'authenticator_code_invalid') {
        setCode('')
        setCodeError(t('security.twoFactor.codeInvalid'))
        return
      }
      if (errorCode === 'authenticator_replacement_expired') {
        setReplacement(null)
        toast.warning(t('security.twoFactor.switchExpired'))
        return
      }
      console.debug('ChangeTwoFactorForm could not switch apps', { error })
      toast.danger(t('common:errors.unexpected'))
    }
    finally {
      setIsSwitching(false)
    }
  }

  const switchButton = <Button type='submit' isDisabled={!canSwitch} isPending={isSwitching}>
    <span>{t('security.twoFactor.switch')}</span>
  </Button>

  return <>
    <section className='relaxed rounded-xl border border-separator p-6'>
      <Form onSubmit={onSubmit} validationBehavior='aria'>
        <TextField
          id='two-factor-nickname'
          className='max-w-md'
          isRequired
          maxLength={AUTHENTICATOR_NICKNAME_MAX_LENGTH}
          value={nickname}
          onChange={setNickname}
        >
          <Label>{t('security.twoFactor.nickname')}</Label>
          <Input ref={nicknameRef} autoComplete='off' placeholder={t('security.twoFactor.nicknamePlaceholder')} />
        </TextField>
      </Form>
    </section>

    <section className='relaxed rounded-xl border border-separator p-6'>
      <h2 className='compact text-lg font-semibold'>{t('security.twoFactor.switchHeading')}</h2>
      <p className='relaxed text-sm opacity-70'>{t('security.twoFactor.switchKeepsCurrent')}</p>

      {!replacement && <Button variant='outline' isPending={isStarting} onPress={() => void start()}>
        <LuSmartphone className='size-4' aria-hidden />
        <span>{t('security.twoFactor.switchStart')}</span>
      </Button>}

      {replacement && <Form onSubmit={switchApps} validationBehavior='aria'>
        <div className='relaxed w-full'>
          <AuthenticatorSetup
            qrCode={replacement.qrCode}
            secretKey={replacement.secretKey}
            code={code}
            onCodeChange={(value) => {
              setCode(value)
              setCodeError(null)
            }}
            codeError={codeError}
            isDisabled={isSwitching}
          />
        </div>
        {canSwitch || isSwitching
          ? switchButton
          // A disabled button fires no hover events, so the tooltip's trigger wraps it.
          : <Tooltip delay={200}>
            <Tooltip.Trigger>
              <div>{switchButton}</div>
            </Tooltip.Trigger>
            <Tooltip.Content className='max-w-xs'>
              <span>{
                trimmed
                  ? t('security.twoFactor.enterCodeFirst')
                  : t('security.twoFactor.nameRequired')
              }</span>
            </Tooltip.Content>
          </Tooltip>}
      </Form>}
    </section>
  </>
}
