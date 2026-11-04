// Copyright © 2026 Jalapeno Labs

import type { ClaudeSignIn, Llm } from '../../../api/routes/llmRoutes'

// Core
import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch } from '../../../store/hooks'
import { llmUpserted } from '../../../store/llmsSlice'

// User interface
import {
  Button,
  Description,
  FieldError,
  Form,
  Input,
  Label,
  Link,
  NumberField,
  TextField,
} from '@heroui/react'
import { LuExternalLink } from 'react-icons/lu'

// Misc
import { cancelClaudeSignIn, finishClaudeSignIn, startClaudeSignIn } from '../../../api/routes/llmRoutes'
import { getApiErrorMessage } from '../../../api/errors'
import { toast } from '../../../toast/toast'

type Props = {
  onSaved: (llm: Llm) => void
  onCancel: () => void
}

// Signs a Claude account in through the official CLI, which runs in Elysium's workspace: start
// it, open the page it links to, sign in there, and paste back the code that page shows. A wrong
// code ends the sign-in, so the person starts again. A failure is a toast that stays until
// dismissed, so nothing moves the form.
export function ClaudeSignInForm(props: Props) {
  const { t } = useTranslation([ 'llms', 'common' ])
  const dispatch = useAppDispatch()
  const [ signIn, setSignIn ] = useState<ClaudeSignIn | null>(null)
  const [ name, setName ] = useState('')
  const [ priority, setPriority ] = useState(0)
  const [ code, setCode ] = useState('')
  const [ isBusy, setIsBusy ] = useState(false)

  // The waiting sign-in, read when the page goes away, so its CLI never outlives the page.
  const waitingId = useRef<string | null>(null)
  useEffect(() => () => {
    if (waitingId.current) {
      void cancelClaudeSignIn(waitingId.current).catch((error: unknown) => {
        console.debug('ClaudeSignInForm could not cancel its sign-in on leaving', { error })
      })
    }
  }, [])

  async function start() {
    setIsBusy(true)
    try {
      const response = await startClaudeSignIn()
      waitingId.current = response.signIn.id
      setSignIn(response.signIn)
    }
    catch (error) {
      console.debug('ClaudeSignInForm could not start a sign-in', { error })
      toast.danger(getApiErrorMessage(error) ?? t('common:errors.unexpected'))
    }
    setIsBusy(false)
  }

  async function finish() {
    if (!signIn) {
      console.debug('ClaudeSignInForm was asked to finish without a sign-in')
      return
    }
    setIsBusy(true)
    try {
      const response = await finishClaudeSignIn(signIn.id, {
        code: code.trim(),
        name: name.trim() || undefined,
        priority,
      })
      waitingId.current = null
      dispatch(llmUpserted(response.llm))
      toast.success(t('signIn.signedIn', { name: response.llm.name }))
      props.onSaved(response.llm)
    }
    catch (error) {
      console.debug('ClaudeSignInForm could not finish the sign-in', { error })
      // The CLI exits on any answer, so the sign-in is over either way.
      waitingId.current = null
      setSignIn(null)
      setCode('')
      toast.danger(getApiErrorMessage(error) ?? t('common:errors.unexpected'))
    }
    setIsBusy(false)
  }

  async function cancel() {
    if (waitingId.current) {
      const id = waitingId.current
      waitingId.current = null
      await cancelClaudeSignIn(id).catch((error: unknown) => {
        console.debug('ClaudeSignInForm could not cancel its sign-in', { error })
      })
    }
    props.onCancel()
  }

  return <Form
    validationBehavior='aria'
    className='flex flex-col gap-5'
    onSubmit={(event) => {
      event.preventDefault()
      void (signIn
        ? finish()
        : start())
    }}
  >
    <p className='text-sm opacity-80'>{t('signIn.description')}</p>

    {/* Name, defaulting to the account's email */}
    <TextField
      isDisabled={Boolean(signIn)}
      value={name}
      onChange={setName}
    >
      <Label>{t('form.name')}</Label>
      <Input placeholder={t('signIn.namePlaceholder')} />
      <Description>{t('signIn.nameHint')}</Description>
    </TextField>

    {/* Priority */}
    <NumberField
      isDisabled={Boolean(signIn)}
      step={1}
      value={priority}
      onChange={(value) => setPriority(Number.isNaN(value)
        ? 0
        : value)}
    >
      <Label>{t('form.priority')}</Label>
      <NumberField.Group>
        <NumberField.DecrementButton />
        <NumberField.Input />
        <NumberField.IncrementButton />
      </NumberField.Group>
      <Description>{t('form.priorityHint')}</Description>
    </NumberField>

    {signIn && <div className='flex flex-col gap-4 rounded-xl border border-separator p-4'>
      <Link
        href={signIn.authorizeUrl}
        target='_blank'
        rel='noreferrer'
        className='text-link'
      >
        <span>{t('signIn.openPage')}</span>
        <LuExternalLink className='size-4' aria-hidden />
      </Link>

      {/* The code Anthropic's page shows */}
      <TextField
        autoFocus
        isRequired
        autoComplete='off'
        value={code}
        onChange={setCode}
      >
        <Label>{t('signIn.code')}</Label>
        <Input className='font-mono' spellCheck={false} />
        <Description>{t('signIn.codeHint')}</Description>
        <FieldError />
      </TextField>
    </div>}

    <div className='flex justify-end gap-2'>
      <Button variant='tertiary' onPress={() => void cancel()}>
        <span>{t('common:actions.cancel')}</span>
      </Button>
      <Button
        type='submit'
        isPending={isBusy}
        isDisabled={Boolean(signIn) && !code.trim()}
      >
        <span>{signIn
          ? t('signIn.finish')
          : t('signIn.start')}</span>
      </Button>
    </div>
  </Form>
}
