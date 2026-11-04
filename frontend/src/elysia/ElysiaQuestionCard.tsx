// Copyright © 2026 Jalapeno Labs

import type { ElysiaQuestion } from '../api/routes/elysiaRoutes'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Description, Form, Label, TextArea, TextField } from '@heroui/react'

// Misc
import { answerElysiaQuestion, dismissElysiaQuestion } from '../api/routes/elysiaRoutes'
import { submitOnEnter } from '../components/submitOnEnter'
import { useTextDraft } from '../drafts/useTextDraft'
import { usePendingAction } from '../hooks/usePendingAction'
import { toast } from '../toast/toast'

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
  // An answer in the person's own words is kept for the tab until it is sent.
  const answerDraft = useTextDraft(`elysia-answer.${props.question.id}`)
  const draft = answerDraft.text ?? ''
  // `option:<answer>` for a suggested answer, `draft` for one typed, or `dismiss`.
  const pending = usePendingAction<`option:${string}` | 'draft' | 'dismiss'>()

  async function answer(text: string) {
    const body = text.trim()
    if (!body) {
      console.debug('ElysiaQuestionCard ignored an empty answer', { questionId: props.question.id })
      return
    }
    try {
      await answerElysiaQuestion(props.question.id, body)
      answerDraft.forget()
    }
    catch (error) {
      console.debug('ElysiaQuestionCard failed to answer', { error, questionId: props.question.id })
      toast.danger(t('questions.failed'))
    }
  }

  async function dismiss() {
    try {
      await dismissElysiaQuestion(props.question.id)
      answerDraft.forget()
    }
    catch (error) {
      console.debug('ElysiaQuestionCard failed to dismiss', { error, questionId: props.question.id })
      toast.danger(t('questions.failed'))
    }
  }

  return <div className='compact rounded-xl border border-elysia-needs-you/40 bg-surface p-3'>
    <p className='compact text-sm whitespace-pre-line'>{props.question.prompt}</p>
    {Boolean(props.question.options.length) && <div className='compact flex flex-wrap gap-2'>{
      props.question.options.map((option) => <Button
        key={option}
        size='sm'
        variant='secondary'
        isDisabled={pending.isOtherPending(`option:${option}`)}
        isPending={pending.pendingKey === `option:${option}`}
        onPress={() => void pending.run(`option:${option}`, () => answer(option))}
      >
        <span>{option}</span>
      </Button>)
    }</div>}
    {/* Enter adds a line; Ctrl or Cmd with Enter answers (ux.md, Hotkeys) */}
    <Form
      validationBehavior='aria'
      onKeyDownCapture={submitOnEnter}
      onSubmit={(event) => {
        event.preventDefault()
        void pending.run('draft', () => answer(draft))
      }}
    >
      <TextField
        id={`elysia-question-${props.question.id}-answer`}
        className='w-full'
        value={draft}
        maxLength={ANSWER_MAX_CHARACTERS}
        onChange={answerDraft.change}
      >
        <Label className='sr-only'>{t('questions.answerLabel')}</Label>
        <TextArea
          rows={1}
          placeholder={t('questions.answerLabel')}
        />
        <Description className='text-xs'>{t('composer.hint')}</Description>
      </TextField>
      <div className='mt-2 level-right gap-2'>
        <Button
          size='sm'
          variant='ghost'
          isDisabled={pending.isOtherPending('dismiss')}
          isPending={pending.pendingKey === 'dismiss'}
          onPress={() => void pending.run('dismiss', dismiss)}
        >
          <span>{t('questions.dismiss')}</span>
        </Button>
        <Button
          type='submit'
          size='sm'
          isDisabled={!draft.trim() || pending.isOtherPending('draft')}
          isPending={pending.pendingKey === 'draft'}
        >
          <span>{t('questions.answer')}</span>
        </Button>
      </div>
    </Form>
  </div>
}
