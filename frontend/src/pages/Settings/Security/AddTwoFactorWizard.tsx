// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'
import type { KratosSettingsFlow } from '../../../hooks/useKratosSettingsFlow'
import type { AddTwoFactorStep } from './addTwoFactorSteps'

// Core
import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Navigate, useNavigate, useSearchParams } from 'react-router'

// User interface
import { StepProgress } from '../../../components/StepProgress'
import { TwoFactorAppStep } from './TwoFactorAppStep'
import { TwoFactorCodesStep } from './TwoFactorCodesStep'
import { TwoFactorNameStep } from './TwoFactorNameStep'

// Misc
import { collectMessages } from '../../../api/kratosFlows'
import { useKratosSettingsFlow } from '../../../hooks/useKratosSettingsFlow'
import { LEAVE_WITHOUT_ASKING, usePageSave } from '../../../layout/page/pageSave'
import { UrlTree } from '../../../urls'
import { fieldErrorText } from '../../Auth/kratosPresentation'
import { ADD_TWO_FACTOR_PARAMS, ADD_TWO_FACTOR_STEPS, getAddTwoFactorStepUrl } from './addTwoFactorSteps'
import { readAuthenticator, readLookupCodes } from './securityPresentation'
import { storeAuthenticatorNickname } from './twoFactorNickname'
import { toast } from '../../../toast/toast'

// Authenticator apps show six digits.
const CODE_LENGTH = 6

// Moving between steps stays on the page, so it never asks about leaving it.
const stepNavigation = { state: LEAVE_WITHOUT_ASKING } as const

const stepLabelKeys = {
  name: 'security.twoFactor.steps.name',
  codes: 'security.twoFactor.steps.codes',
  app: 'security.twoFactor.steps.app',
} as const satisfies Record<AddTwoFactorStep, ParseKeys<'auth'>>

type Props = {
  // The flow holding the new backup codes, unsaved until the app is verified.
  codesFlow: KratosSettingsFlow
  step: AddTwoFactorStep
}

// The steps of adding two-factor authentication, and the save that finishes them: verify the
// app in its own flow, which turns 2FA on, then save the downloaded codes from theirs. The app
// flow is started here, once the codes flow is in the address, so the two never write it at
// once. See `AddTwoFactorPage`.
export function AddTwoFactorWizard(props: Props) {
  const { t } = useTranslation([ 'auth', 'common' ])
  const navigate = useNavigate()
  const [ params ] = useSearchParams()
  const appFlow = useKratosSettingsFlow(UrlTree.settingsTwoFactorNew, {
    flowParam: ADD_TWO_FACTOR_PARAMS.appFlow,
  })
  const [ draftName, setDraftName ] = useState('')
  const [ isDownloaded, setIsDownloaded ] = useState(false)
  const [ code, setCode ] = useState('')
  const [ isGenerating, setIsGenerating ] = useState(false)
  const [ hasGenerateFailed, setHasGenerateFailed ] = useState(false)
  // The codes flow new codes were asked of. Asking twice, as StrictMode's doubled effects would,
  // could leave the page showing one set while Kratos holds the other.
  const generatedFor = useRef<string | null>(null)

  const appName = params.get(ADD_TWO_FACTOR_PARAMS.name)?.trim() ?? ''
  const codesFlow = props.codesFlow.flow
  const newCodes = codesFlow
    ? readLookupCodes(codesFlow)
    : null
  // Made and waiting to be saved: Kratos offers to confirm them.
  const hasNewCodes = Boolean(newCodes?.canConfirm && newCodes.codes)
  const authenticator = appFlow.flow
    ? readAuthenticator(appFlow.flow)
    : { kind: 'unavailable' } as const
  // The app flow reads the codes as they stand, before the new ones replace them.
  const appFlowCodes = appFlow.flow
    ? readLookupCodes(appFlow.flow)
    : null
  const replacesCodes = Boolean(appFlowCodes?.canReveal || appFlowCodes?.canDisable)
  const isAppVerified = authenticator.kind === 'enrolled'
  const codeMessages = appFlow.flow
    ? collectMessages(appFlow.flow).byField.totp_code
    : undefined

  const submitCodes = props.codesFlow.submit
  const codesFlowId = codesFlow?.id ?? null
  const needsCodes = props.step === 'codes' && !hasNewCodes && !isGenerating && !hasGenerateFailed
  useEffect(() => {
    if (!needsCodes || !codesFlowId || generatedFor.current === codesFlowId) {
      return
    }
    generatedFor.current = codesFlowId
    setIsGenerating(true)
    void submitCodes({ method: 'lookup_secret', lookup_secret_regenerate: true }, null)
      .then((isMade) => {
        setHasGenerateFailed(!isMade)
        if (!isMade) {
          // Retrying asks again.
          generatedFor.current = null
        }
      })
      .finally(() => setIsGenerating(false))
  }, [ codesFlowId, needsCodes, submitCodes ])

  async function finish() {
    if (!isAppVerified) {
      const isOn = await appFlow.submit({ method: 'totp', totp_code: code }, null)
      if (!isOn) {
        setCode('')
        return false
      }
    }

    // The app is on from here. Saving the codes it failed to is what the button retries.
    const areCodesSaved = await props.codesFlow.submit(
      { method: 'lookup_secret', lookup_secret_confirm: true },
      null,
    )
    if (!areCodesSaved) {
      toast.warning(t('security.twoFactor.codesNotSaved'))
      return false
    }

    try {
      await storeAuthenticatorNickname(appName)
    }
    catch (error) {
      console.debug('AddTwoFactorWizard could not save the nickname', { error })
      toast.warning(t('security.twoFactor.nicknameFailed'))
    }
    toast.success(t('security.twoFactor.enrolled'))
    void navigate(UrlTree.settingsSecurity, { state: LEAVE_WITHOUT_ASKING })
    return true
  }

  let blockedReason: string | null = null
  if (props.step !== 'app') {
    blockedReason = t('security.twoFactor.finishSteps')
  }
  else if (!isAppVerified && code.length !== CODE_LENGTH) {
    blockedReason = t('security.twoFactor.enterCodeFirst')
  }

  const onSubmit = usePageSave({
    save: finish,
    isDirty: props.step !== 'name' || Boolean(draftName.trim()),
    blockedReason,
    label: t('security.twoFactor.turnOn'),
  })

  // The last step saves codes made in the step before it; an address that skips there goes back.
  if (props.step === 'app' && codesFlow && !hasNewCodes && !isAppVerified) {
    return <Navigate to={getAddTwoFactorStepUrl(params, 'codes')} replace />
  }

  const steps = ADD_TWO_FACTOR_STEPS.map((step) => ({
    id: step,
    label: t(stepLabelKeys[step]),
    href: getAddTwoFactorStepUrl(params, step),
  }))

  return <>
    <StepProgress
      steps={steps}
      current={props.step}
      label={t('security.twoFactor.steps.label')}
      routerOptions={stepNavigation}
    />

    <section className='rounded-xl border border-separator p-6'>
      {props.step === 'name' && <TwoFactorNameStep
        initialName={appName}
        onDraftChange={setDraftName}
        onNext={(name) => void navigate(getAddTwoFactorStepUrl(params, 'codes', name), stepNavigation)}
      />}

      {props.step === 'codes' && <TwoFactorCodesStep
        codes={hasNewCodes
          ? newCodes?.codes ?? null
          : null}
        appName={appName}
        replacesCodes={replacesCodes}
        isDownloaded={isDownloaded}
        onDownloaded={() => setIsDownloaded(true)}
        hasFailed={hasGenerateFailed}
        onRetry={() => setHasGenerateFailed(false)}
        backHref={getAddTwoFactorStepUrl(params, 'name')}
        onNext={() => void navigate(getAddTwoFactorStepUrl(params, 'app'), stepNavigation)}
      />}

      {props.step === 'app' && <TwoFactorAppStep
        authenticator={authenticator}
        code={code}
        onCodeChange={setCode}
        codeError={fieldErrorText(codeMessages, t) ?? null}
        backHref={getAddTwoFactorStepUrl(params, 'codes')}
        onSubmit={onSubmit}
      />}
    </section>
  </>
}
