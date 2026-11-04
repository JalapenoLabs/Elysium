// Copyright © 2026 Jalapeno Labs

import type { RecoveryFlow } from '@ory/client-fetch'
import type { FormEvent } from 'react'

// Core
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useSearchParams } from 'react-router'
import { useKratosMessageToasts } from '../../hooks/useKratosMessageToasts'

// User interface
import { Alert, Button, FieldError, Form, Input, Label, Link, Spinner, TextField } from '@heroui/react'

// Misc
import { kratos } from '../../api/kratos'
import { collectMessages, csrfToken, readKratosFailure } from '../../api/kratosFlows'
import { useInitialFocus } from '../../hooks/useInitialFocus'
import { toast } from '../../toast/toast'
import { KRATOS_FLOW_PARAM, UrlTree } from '../../urls'
import { fieldErrorText } from './kratosPresentation'

// Forgotten passwords. Kratos makes a one-time link for the address, which signs its holder
// in and opens Security to set a new password. Elysium sends no email yet, so the
// link is written to the API's log for an admin to pass on; once it is made, the page says
// so in place of the form. A refusal is a toast, so nothing moves the form.
export function RecoveryPage() {
  const { t } = useTranslation([ 'auth', 'common' ])
  const [ params ] = useSearchParams()
  const flowId = params.get(KRATOS_FLOW_PARAM)
  const [ flow, setFlow ] = useState<RecoveryFlow | null>(null)
  const [ loadFailed, setLoadFailed ] = useState(false)
  const toastFormMessages = useKratosMessageToasts()
  const [ email, setEmail ] = useState('')
  const emailRef = useInitialFocus()
  const [ isSubmitting, setIsSubmitting ] = useState(false)

  useEffect(() => {
    let isCurrent = true
    // A link that expired lands here with its flow, which explains why.
    const request = flowId
      ? kratos.getRecoveryFlow({ id: flowId })
      : kratos.createBrowserRecoveryFlow()
    request
      .then((loaded) => {
        if (!isCurrent) {
          return
        }
        setFlow(loaded)
        // Such as why a link expired. A flow already sent says only that an email went out,
        // which Elysium does not send, so the page's own words replace it.
        if (loaded.state !== 'sent_email') {
          toastFormMessages(loaded.ui.messages)
        }
      })
      .catch(async (error: unknown) => {
        const outcome = await readKratosFailure(error)
        if (!isCurrent) {
          return
        }
        if (outcome.kind === 'signedIn') {
          window.location.assign(UrlTree.settingsSecurity)
          return
        }
        setLoadFailed(true)
      })
    return () => {
      isCurrent = false
    }
  }, [ flowId, toastFormMessages ])

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!flow) {
      return
    }

    setIsSubmitting(true)
    try {
      const updated = await kratos.updateRecoveryFlow({
        flow: flow.id,
        updateRecoveryFlowBody: { method: 'link', email: email.trim(), csrf_token: csrfToken(flow) },
      })
      setFlow(updated)
    }
    catch (error) {
      const outcome = await readKratosFailure(error)
      if (outcome.kind === 'invalid') {
        setFlow({ ...flow, ui: outcome.flow.ui })
        toastFormMessages(outcome.flow.ui.messages)
      }
      else {
        console.debug('RecoveryPage could not make a recovery link', { outcome })
        toast.danger(t('common:errors.unexpected'))
      }
    }
    setIsSubmitting(false)
  }

  const isSent = flow?.state === 'sent_email'
  const messages = flow
    ? collectMessages(flow)
    : null

  return <div>
    <div className='relaxed'>
      <h1 className='text-2xl font-bold'>{
        isSent
          ? t('recovery.sent.title')
          : t('recovery.heading')
      }</h1>
      {isSent && <p className='mt-1 text-sm opacity-70'>{t('recovery.sent.description')}</p>}
    </div>

    {loadFailed && <Alert status='danger' className='compact'>
      <Alert.Indicator />
      <Alert.Content>
        <Alert.Description>{t('recovery.loadError')}</Alert.Description>
      </Alert.Content>
    </Alert>}

    {!flow && !loadFailed && <div className='grid place-items-center py-10'>
      <Spinner />
    </div>}

    {flow && messages && !isSent && <Form onSubmit={submit} validationBehavior='aria'>
      <TextField
        isRequired
        id='recovery-email'
        className='relaxed'
        type='email'
        autoComplete='email'
        isInvalid={Boolean(messages.byField.email)}
        value={email}
        onChange={setEmail}
      >
        <Label>{t('fields.email')}</Label>
        <Input ref={emailRef} />
        <FieldError>{fieldErrorText(messages.byField.email, t)}</FieldError>
      </TextField>
      <Button
        fullWidth
        type='submit'
        isDisabled={!email.trim() || isSubmitting}
        isPending={isSubmitting}
      >
        <span>{t('recovery.submit')}</span>
      </Button>
    </Form>}

    <p className='mt-6 text-center text-sm'>
      <Link href={UrlTree.login} className='text-link'>{t('recovery.backToSignIn')}</Link>
    </p>
  </div>
}
