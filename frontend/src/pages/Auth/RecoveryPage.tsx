// Copyright © 2026 Jalapeno Labs

import type { RecoveryFlow } from '@ory/client-fetch'
import type { FormEvent } from 'react'

// Core
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useSearchParams } from 'react-router'

// User interface
import { Alert, Button, FieldError, Form, Input, Label, Link, Spinner, TextField } from '@heroui/react'
import { KratosMessages } from './KratosMessages'

// Misc
import { kratos } from '../../api/kratos'
import { collectMessages, csrfToken, readKratosFailure } from '../../api/kratosFlows'
import { KRATOS_FLOW_PARAM, UrlTree } from '../../urls'
import { fieldErrorText } from './kratosPresentation'

// Forgotten passwords. Kratos makes a one-time link for the address, which signs its holder
// in and opens Sign-in & security to set a new password. Elysium sends no email yet, so the
// link is written to the API's log for an admin to pass on; the page says so plainly.
export function RecoveryPage() {
  const { t } = useTranslation([ 'auth', 'common' ])
  const [ params ] = useSearchParams()
  const flowId = params.get(KRATOS_FLOW_PARAM)
  const [ flow, setFlow ] = useState<RecoveryFlow | null>(null)
  const [ failure, setFailure ] = useState<string | null>(null)
  const [ email, setEmail ] = useState('')
  const [ isSubmitting, setIsSubmitting ] = useState(false)

  useEffect(() => {
    let isCurrent = true
    // A link that expired lands here with its flow, which explains why.
    const request = flowId
      ? kratos.getRecoveryFlow({ id: flowId })
      : kratos.createBrowserRecoveryFlow()
    request
      .then((loaded) => {
        if (isCurrent) {
          setFlow(loaded)
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
        setFailure(t('recovery.loadError'))
      })
    return () => {
      isCurrent = false
    }
  }, [ flowId, t ])

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
      }
      else {
        setFailure(t('common:errors.unexpected'))
      }
    }
    setIsSubmitting(false)
  }

  const isSent = flow?.state === 'sent_email'
  const messages = flow
    ? collectMessages(flow)
    : null

  return <div>
    <h1 className='text-2xl font-bold'>{t('recovery.heading')}</h1>
    <p className='relaxed mt-1 text-sm opacity-70'>{t('recovery.description')}</p>

    {failure && <Alert status='danger' className='compact'>
      <Alert.Indicator />
      <Alert.Content>
        <Alert.Description>{failure}</Alert.Description>
      </Alert.Content>
    </Alert>}

    {!flow && !failure && <div className='grid place-items-center py-10'>
      <Spinner />
    </div>}

    {isSent && <Alert status='success' className='relaxed'>
      <Alert.Indicator />
      <Alert.Content>
        <Alert.Title>{t('recovery.sent.title')}</Alert.Title>
        <Alert.Description>{t('recovery.sent.description')}</Alert.Description>
      </Alert.Content>
    </Alert>}

    {flow && messages && !isSent && <Form onSubmit={submit} validationBehavior='aria'>
      <KratosMessages messages={messages.form} />
      <TextField
        isRequired
        autoFocus
        className='relaxed'
        type='email'
        autoComplete='email'
        isInvalid={Boolean(messages.byField.email)}
        value={email}
        onChange={setEmail}
      >
        <Label>{t('fields.email')}</Label>
        <Input />
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
