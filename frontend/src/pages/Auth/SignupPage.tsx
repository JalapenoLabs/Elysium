// Copyright © 2026 Jalapeno Labs

import type { RegistrationFlow } from '@ory/client-fetch'
import type { FormEvent } from 'react'

// Core
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import useSWR from 'swr'
import { useKratosMessageToasts } from '../../hooks/useKratosMessageToasts'

// User interface
import { Alert, Button, FieldError, Form, Input, Label, Link, Spinner, TextField } from '@heroui/react'
import { PasswordInput } from '../../components/PasswordInput'
import { SecretInput } from '../../components/SecretInput'
import { PersonNameFields } from '../../components/PersonNameFields'

// Misc
import { kratos } from '../../api/kratos'
import { collectMessages, csrfToken, readKratosFailure } from '../../api/kratosFlows'
import { getAuthStatus } from '../../api/routes/authRoutes'
import { usePasswordAssessment } from '../../hooks/usePasswordAssessment'
import { toast } from '../../toast/toast'
import { UrlTree } from '../../urls'
import { fieldErrorText } from './kratosPresentation'

// Signing up with a first and last name, an email, and a password. Passkeys are added once the account
// exists; Elysium refuses a passkey sign-up. Everyone but the first person then waits for an
// admin, which the workspace shows them after the full page load that follows. What a submit
// reports is a toast, or an error on its field, so nothing moves the form.
export function SignupPage() {
  const { t } = useTranslation([ 'auth', 'common' ])
  const { data: status } = useSWR('v1/auth/status', getAuthStatus)
  const [ flow, setFlow ] = useState<RegistrationFlow | null>(null)
  const [ loadFailed, setLoadFailed ] = useState(false)
  const toastFormMessages = useKratosMessageToasts()
  const [ firstName, setFirstName ] = useState('')
  const [ lastName, setLastName ] = useState('')
  const [ email, setEmail ] = useState('')
  const [ password, setPassword ] = useState('')
  const [ confirmation, setConfirmation ] = useState('')
  const [ isSubmitting, setIsSubmitting ] = useState(false)
  const assessment = usePasswordAssessment(password, [ firstName, lastName, email ])

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
          setLoadFailed(true)
        }
      })
    return () => {
      isCurrent = false
    }
  }, [])

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
          traits: {
            email: email.trim(),
            name: { first: firstName.trim(), last: lastName.trim() },
          },
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
        toastFormMessages(outcome.flow.ui.messages)
      }
      else if (outcome.kind === 'signedIn') {
        window.location.assign(UrlTree.root)
        return
      }
      else {
        console.debug('SignupPage could not sign up', { outcome })
        toast.danger(t('common:errors.unexpected'))
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
    && firstName.trim().length > 0
    && email.trim().length > 0
    && assessment.isAcceptable
    && confirmation === password
    && !isSubmitting
  // Said only once something is typed, so the empty field never starts out in red.
  const isMismatched = confirmation.length > 0 && confirmation !== password

  return <div>
    <h1 className='text-2xl font-bold'>{t('signup.heading')}</h1>
    <p className='relaxed mt-1 text-sm opacity-70'>{
      status?.firstSignUp
        ? t('signup.firstDescription')
        : t('signup.description')
    }</p>

    {loadFailed && <Alert status='danger' className='compact'>
      <Alert.Indicator />
      <Alert.Content>
        <Alert.Description>{t('signup.loadError')}</Alert.Description>
      </Alert.Content>
    </Alert>}

    {!flow && !loadFailed && <div className='grid place-items-center py-10'>
      <Spinner />
    </div>}

    {flow && messages && <Form onSubmit={submit} validationBehavior='aria'>
      <div className='compact'>
        <PersonNameFields
          autoFocus
          firstName={firstName}
          lastName={lastName}
          onFirstNameChange={setFirstName}
          onLastNameChange={setLastName}
          firstNameError={fieldErrorText(messages.byField['traits.name.first'], t)}
          lastNameError={fieldErrorText(messages.byField['traits.name.last'], t)}
        />
      </div>
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
      <div className='compact'>
        <PasswordInput
          label={t('fields.password')}
          value={password}
          onChange={setPassword}
          assessment={assessment}
          errorMessage={fieldErrorText(messages.byField.password, t)}
        />
      </div>
      <SecretInput
        isRequired
        className='relaxed'
        label={t('fields.confirmPassword')}
        autoComplete='new-password'
        errorMessage={isMismatched
          ? t('security.password.mismatch')
          : null}
        value={confirmation}
        onChange={setConfirmation}
      />
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
