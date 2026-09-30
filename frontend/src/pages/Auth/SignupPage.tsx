// Copyright © 2026 Jalapeno Labs

import type { RegistrationFlow } from '@ory/client-fetch'
import type { FormEvent } from 'react'

// Core
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import useSWR from 'swr'

// User interface
import { Alert, Button, FieldError, Form, Input, Label, Link, Spinner, TextField } from '@heroui/react'
import { PasswordInput } from '../../components/PasswordInput'
import { KratosMessages } from './KratosMessages'

// Misc
import { kratos } from '../../api/kratos'
import { collectMessages, csrfToken, readKratosFailure } from '../../api/kratosFlows'
import { getAuthStatus } from '../../api/routes/authRoutes'
import { usePasswordAssessment } from '../../hooks/usePasswordAssessment'
import { UrlTree } from '../../urls'
import { fieldErrorText } from './kratosPresentation'

// Signing up with a name, an email, and a password. Passkeys are added once the account
// exists; Elysium refuses a passkey sign-up. Everyone but the first person then waits for an
// admin, which the workspace shows them after the full page load that follows.
export function SignupPage() {
  const { t } = useTranslation([ 'auth', 'common' ])
  const { data: status } = useSWR('v1/auth/status', getAuthStatus)
  const [ flow, setFlow ] = useState<RegistrationFlow | null>(null)
  const [ failure, setFailure ] = useState<string | null>(null)
  const [ name, setName ] = useState('')
  const [ email, setEmail ] = useState('')
  const [ password, setPassword ] = useState('')
  const [ isSubmitting, setIsSubmitting ] = useState(false)
  const assessment = usePasswordAssessment(password, [ name, email ])

  useEffect(() => {
    let isCurrent = true
    kratos.createBrowserRegistrationFlow()
      .then((created) => {
        if (isCurrent) {
          setFlow(created)
        }
      })
      .catch(async (error: unknown) => {
        const outcome = await readKratosFailure(error)
        if (outcome.kind === 'signedIn') {
          window.location.assign(UrlTree.root)
          return
        }
        if (isCurrent) {
          setFailure(t('signup.loadError'))
        }
      })
    return () => {
      isCurrent = false
    }
  }, [ t ])

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!flow) {
      return
    }

    setIsSubmitting(true)
    try {
      await kratos.updateRegistrationFlow({
        flow: flow.id,
        updateRegistrationFlowBody: {
          method: 'password',
          csrf_token: csrfToken(flow),
          traits: { email: email.trim(), name: name.trim() },
          password,
        },
      })
      window.location.assign(UrlTree.root)
      return
    }
    catch (error) {
      const outcome = await readKratosFailure(error)
      if (outcome.kind === 'invalid') {
        setFlow({ ...flow, ui: outcome.flow.ui })
      }
      else if (outcome.kind === 'signedIn') {
        window.location.assign(UrlTree.root)
        return
      }
      else {
        setFailure(t('common:errors.unexpected'))
      }
    }
    setIsSubmitting(false)
  }

  if (status && !status.signupOpen) {
    return <div>
      <h1 className='text-2xl font-bold'>{t('signup.closed.heading')}</h1>
      <p className='relaxed mt-2 text-sm opacity-70'>{t('signup.closed.description')}</p>
      <Link href={UrlTree.login} className='text-link'>{t('signup.signIn')}</Link>
    </div>
  }

  const messages = flow
    ? collectMessages(flow)
    : null
  const canSubmit = Boolean(flow)
    && name.trim().length > 0
    && email.trim().length > 0
    && assessment.isAcceptable
    && !isSubmitting

  return <div>
    <h1 className='text-2xl font-bold'>{t('signup.heading')}</h1>
    <p className='relaxed mt-1 text-sm opacity-70'>{
      status?.firstSignUp
        ? t('signup.firstDescription')
        : t('signup.description')
    }</p>

    {failure && <Alert status='danger' className='compact'>
      <Alert.Indicator />
      <Alert.Content>
        <Alert.Description>{failure}</Alert.Description>
      </Alert.Content>
    </Alert>}

    {!flow && !failure && <div className='grid place-items-center py-10'>
      <Spinner />
    </div>}

    {flow && messages && <Form onSubmit={submit} validationBehavior='aria'>
      <KratosMessages messages={messages.form} />
      <TextField
        isRequired
        autoFocus
        className='compact'
        autoComplete='name'
        maxLength={100}
        isInvalid={Boolean(messages.byField['traits.name'])}
        value={name}
        onChange={setName}
      >
        <Label>{t('fields.name')}</Label>
        <Input />
        <FieldError>{fieldErrorText(messages.byField['traits.name'], t)}</FieldError>
      </TextField>
      <TextField
        isRequired
        className='compact'
        type='email'
        autoComplete='email'
        isInvalid={Boolean(messages.byField['traits.email'])}
        value={email}
        onChange={setEmail}
      >
        <Label>{t('fields.email')}</Label>
        <Input />
        <FieldError>{fieldErrorText(messages.byField['traits.email'], t)}</FieldError>
      </TextField>
      <div className='relaxed'>
        <PasswordInput
          label={t('fields.password')}
          value={password}
          onChange={setPassword}
          assessment={assessment}
          errorMessage={fieldErrorText(messages.byField.password, t)}
        />
      </div>
      <Button fullWidth type='submit' isDisabled={!canSubmit} isPending={isSubmitting}>
        <span>{t('signup.submit')}</span>
      </Button>
    </Form>}

    <p className='mt-6 text-center text-sm opacity-70'>
      <span>{t('signup.haveAccount')}</span>
      {' '}
      <Link href={UrlTree.login} className='text-link'>{t('signup.signIn')}</Link>
    </p>
  </div>
}
