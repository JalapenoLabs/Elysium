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
import { PageCreateFooter } from '../../../layout/page/PageCreateFooter'

// Misc
import { cancelClaudeSignIn, finishClaudeSignIn, startClaudeSignIn } from '../../../api/routes/llmRoutes'
import { getApiErrorMessage } from '../../../api/errors'
import { usePageSave } from '../../../layout/page/pageSave'
import { toast } from '../../../toast/toast'

type Props = {
  onSaved: (llm: Llm) => void
}

// Signs a Claude account in through the official CLI, which runs in Elysium's workspace: start
// it, open the page it links to, sign in there, and paste back the code that page shows. A wrong
// code ends the sign-in, so the person starts again. A failure is a toast that stays until
// dismissed, so nothing moves the form. The page's Save is the step ahead, Start and then Finish,
// in the header and below the form; Start needs nothing typed. Leaving cancels a waiting sign-in,
// so a started one is unsaved work and leaving asks first.
export function ClaudeSignInForm(props: Props) {
  const { t } = useTranslation([ 'llms', 'common' ])
  const dispatch = useAppDispatch()
  const [ signIn, setSignIn ] = useState<ClaudeSignIn | null>(null)
  const [ name, setName ] = useState('')
  const [ priority, setPriority ] = useState(0)
  const [ code, setCode ] = useState('')

  // The waiting sign-in, read when the page goes away, so its CLI never outlives the page.
  const waitingId = useRef<string | null>(null)
  useEffect(() => () => {
    if (waitingId.current) {
      void cancelClaudeSignIn(waitingId.current).catch((error: unknown) => {
        console.debug('ClaudeSignInForm could not cancel its sign-in on leaving', { error })
      })
    }
  }, [])

  // Starting saves nothing yet, so it resolves false: Save and continue stays on the page,
  // where the sign-in now waits for its code.
  async function start() {
    try {
      const response = await startClaudeSignIn()
      waitingId.current = response.signIn.id
      setSignIn(response.signIn)
    }
    catch (error) {
      console.debug('ClaudeSignInForm could not start a sign-in', { error })
      toast.danger(getApiErrorMessage(error) ?? t('common:errors.unexpected'))
    }
    return false
  }

  async function finish(started: ClaudeSignIn) {
    try {
      const response = await finishClaudeSignIn(started.id, {
        code: code.trim(),
        name: name.trim() || undefined,
        priority,
      })
      waitingId.current = null
      dispatch(llmUpserted(response.llm))
      toast.success(t('signIn.signedIn', { name: response.llm.name }))
      props.onSaved(response.llm)
      return true
    }
    catch (error) {
      console.debug('ClaudeSignInForm could not finish the sign-in', { error })
      // The CLI exits on any answer, so the sign-in is over either way.
      waitingId.current = null
      setSignIn(null)
      setCode('')
      toast.danger(getApiErrorMessage(error) ?? t('common:errors.unexpected'))
      return false
    }
  }

  const onSubmit = usePageSave({
    save: () => signIn
      ? finish(signIn)
      : start(),
    isDirty: Boolean(signIn) || Boolean(name.trim()) || priority !== 0,
    canSaveUnchanged: true,
    blockedReason: signIn && !code.trim()
      ? t('signIn.codeNeeded')
      : null,
    label: signIn
      ? t('signIn.finish')
      : t('signIn.start'),
  })

  return <Form
    validationBehavior='aria'
    className='flex flex-col gap-5'
    onSubmit={onSubmit}
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

    <PageCreateFooter />
  </Form>
}
