// Copyright © 2026 Jalapeno Labs

// Core
import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch, useAppSelector } from '../../../store/hooks'
import { knowledgeAskUpserted, selectAllKnowledgeAsks } from '../../../store/knowledgeAsksSlice'

// User interface
import { Button, Description, Label, TextArea, TextField } from '@heroui/react'
import { LuSparkles } from 'react-icons/lu'
import { EmptyNotice } from '../../../components/EmptyNotice'
import { KnowledgeAskCard } from './KnowledgeAskCard'

// Misc
import { getApiErrorMessage } from '../../../api/errors'
import { askKnowledge } from '../../../api/routes/knowledgeRoutes'
import { submitOnEnter } from '../../../components/submitOnEnter'
import { useTextDraft } from '../../../drafts/useTextDraft'
import { useKnowledgeAsksLoader } from '../../../hooks/useServerData'
import { toast } from '../../../toast/toast'

// Also the draft's name, so what is typed survives a reload until it is asked.
const QUESTION_FIELD_ID = 'knowledge-ask-question'

// The question the search caps its query at, which an ask is searched by.
const QUESTION_MAX_CHARACTERS = 500

// The Knowledge page's Ask tab: one question at a time, answered from the knowledge base with
// every claim citing where it came from, and the person's earlier questions below, newest
// first. An answer takes seconds to tens of seconds; its card shows it being written and
// `knowledgeAsk.changed` brings the answer.
export function KnowledgeAskPage() {
  const { t } = useTranslation('knowledge')
  const dispatch = useAppDispatch()
  const status = useKnowledgeAsksLoader()
  const asks = useAppSelector(selectAllKnowledgeAsks)
  const questionDraft = useTextDraft(QUESTION_FIELD_ID)
  const question = questionDraft.text ?? ''
  // Who is asking: the field, or the id of the card whose Ask again was pressed, so only that
  // button waits while the others hold back.
  const [ asking, setAsking ] = useState<string | null>(null)
  const questionRef = useRef<HTMLTextAreaElement>(null)

  // The tab is its question, so typing starts there.
  useEffect(() => {
    questionRef.current?.focus()
  }, [])

  async function ask(asked: string, from: string) {
    const trimmed = asked.trim()
    if (!trimmed || asking !== null) {
      console.debug('KnowledgeAskPage ignored an ask', { isEmpty: !trimmed, isAsking: asking !== null })
      return false
    }
    setAsking(from)
    try {
      const response = await askKnowledge(trimmed)
      dispatch(knowledgeAskUpserted(response.ask))
      return true
    }
    catch (error) {
      toast.danger(t('ask.refused'), { description: getApiErrorMessage(error) ?? undefined })
      return false
    }
    finally {
      setAsking(null)
    }
  }

  return <>
    {/* Enter adds a line; Ctrl or Cmd with Enter asks (ux.md, Hotkeys). */}
    <form
      className='relaxed flex max-w-3xl flex-wrap items-end gap-2'
      onKeyDownCapture={submitOnEnter}
      onSubmit={(event) => {
        event.preventDefault()
        void ask(question, QUESTION_FIELD_ID).then((isAsked) => {
          if (isAsked) {
            questionDraft.forget()
          }
        })
      }}
    >
      <TextField
        id={QUESTION_FIELD_ID}
        className='min-w-0 flex-1 basis-64'
        value={question}
        onChange={questionDraft.change}
        maxLength={QUESTION_MAX_CHARACTERS}
      >
        <Label>{t('ask.label')}</Label>
        <TextArea ref={questionRef} rows={2} placeholder={t('ask.placeholder')} />
        <Description className='text-xs'>{t('ask.hint')}</Description>
      </TextField>
      <div className='mb-6 shrink-0'>
        <Button
          type='submit'
          isDisabled={!question.trim() || (asking !== null && asking !== QUESTION_FIELD_ID)}
          isPending={asking === QUESTION_FIELD_ID}
        >
          <LuSparkles className='size-4' aria-hidden />
          <span>{t('ask.submit')}</span>
        </Button>
      </div>
    </form>

    <section aria-label={t('ask.list')} className='max-w-3xl'>
      {status === 'failed' && <p role='status' className='relaxed text-warning'>{t('ask.loadFailed')}</p>}
      {status === 'loaded' && !asks.length && <EmptyNotice>{t('ask.none')}</EmptyNotice>}
      {asks.map((asked) => <KnowledgeAskCard
        key={asked.id}
        ask={asked}
        isAskingAgain={asking === asked.id}
        onAskAgain={(again) => void ask(again, asked.id)}
      />)}
    </section>
  </>
}
