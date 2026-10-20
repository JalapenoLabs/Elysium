// Copyright © 2026 Jalapeno Labs

import type { ElysiaQuestion } from '../api/routes/elysiaRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Form, Label, TextArea, TextField, toast } from '@heroui/react'

// Misc
import { answerElysiaQuestion, dismissElysiaQuestion } from '../api/routes/elysiaRoutes'

// An answer's length, as the API bounds it.
const ANSWER_MAX_CHARACTERS = 2_000

type Props = {
  question: ElysiaQuestion
}

// One of Elysia's questions: her suggested answers as buttons, or an answer in the person's
// own words. Answering queues her next turn; the question leaves the panel when the
// `elysia.updated` event that follows arrives.
export function ElysiaQuestionCard(props: Props) {
  const { t } = useTranslation('elysia')
  const [ draft, setDraft ] = useState('')
  const [ isSending, setIsSending ] = useState(false)

  async function answer(text: string) {
    const body = text.trim()
    if (!body) {
      console.debug('ElysiaQuestionCard ignored an empty answer', { questionId: props.question.id })
      return
    }
    setIsSending(true)
    try {
      await answerElysiaQuestion(props.question.id, body)
      setDraft('')
    }
    catch (error) {
      console.debug('ElysiaQuestionCard failed to answer', { error, questionId: props.question.id })
      toast.danger(t('questions.failed'))
    }
    finally {
      setIsSending(false)
    }
  }

  async function dismiss() {
    setIsSending(true)
    try {
      await dismissElysiaQuestion(props.question.id)
    }
    catch (error) {
      console.debug('ElysiaQuestionCard failed to dismiss', { error, questionId: props.question.id })
      toast.danger(t('questions.failed'))
    }
    finally {
      setIsSending(false)
    }
  }

  return <div className='compact rounded-xl border border-elysia-needs-you/40 bg-surface p-3'>
    <p className='compact text-sm whitespace-pre-line'>{props.question.prompt}</p>
    {Boolean(props.question.options.length) && <div className='compact flex flex-wrap gap-2'>{
      props.question.options.map((option) => <Button
        key={option}
        size='sm'
        variant='secondary'
        isDisabled={isSending}
        onPress={() => void answer(option)}
      >
        <span>{option}</span>
      </Button>)
    }</div>}
    <Form
      validationBehavior='aria'
      onSubmit={(event) => {
        event.preventDefault()
        void answer(draft)
      }}
    >
      <TextField
        className='w-full'
        value={draft}
        maxLength={ANSWER_MAX_CHARACTERS}
        onChange={setDraft}
      >
        <Label className='sr-only'>{t('questions.answerLabel')}</Label>
        <TextArea
          rows={1}
          placeholder={t('questions.answerLabel')}
          onKeyDown={(event) => {
            if (event.key === 'Enter' && !event.shiftKey) {
              event.preventDefault()
              void answer(draft)
            }
          }}
        />
      </TextField>
      <div className='mt-2 level-right gap-2'>
        <Button size='sm' variant='ghost' isDisabled={isSending} onPress={() => void dismiss()}>
          <span>{t('questions.dismiss')}</span>
        </Button>
        <Button type='submit' size='sm' isDisabled={!draft.trim()} isPending={isSending}>
          <span>{t('questions.answer')}</span>
        </Button>
      </div>
    </Form>
  </div>
}
