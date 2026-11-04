// Copyright © 2026 Jalapeno Labs

import type { KnowledgeAsk } from '../../../api/routes/knowledgeRoutes'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Card, Spinner } from '@heroui/react'
import { LuRotateCcw } from 'react-icons/lu'
import { KnowledgeAnswerView } from './KnowledgeAnswerView'

// Misc
import { DateText } from '../../../format/DateText'
import { waitingLabelKey } from './knowledgeAskPresentation'

type Props = {
  ask: KnowledgeAsk
  // Asks the same question again.
  onAskAgain: (question: string) => void
  isAskingAgain: boolean
}

// One question and what came of it: a spinner while its answer is written, then the answer, or
// why there is none with Ask again.
export function KnowledgeAskCard(props: Props) {
  const { t } = useTranslation('knowledge')
  const { ask } = props
  const waitingKey = waitingLabelKey(ask.state)

  return <Card className='relaxed p-4'>
    <div className='flex flex-wrap items-baseline justify-between gap-x-3'>
      <h3 className='font-medium'>{ask.question}</h3>
      <DateText value={ask.createdAt} className='text-xs opacity-70' />
    </div>

    {/* The spinner announces itself as a status, so the words beside it are plain text. */}
    {waitingKey && <div className='flex items-center gap-2 text-sm opacity-70'>
      <Spinner size='sm' aria-label={t(waitingKey)} />
      <span aria-hidden>{t(waitingKey)}</span>
    </div>}

    {ask.state === 'failed' && <div className='flex flex-wrap items-center gap-3'>
      <p className='text-sm text-warning'>{ask.error ?? t('ask.failed')}</p>
      <Button
        size='sm'
        variant='outline'
        isPending={props.isAskingAgain}
        onPress={() => props.onAskAgain(ask.question)}
      >
        <LuRotateCcw className='size-4' aria-hidden />
        <span>{t('ask.again')}</span>
      </Button>
    </div>}

    {ask.answer && <KnowledgeAnswerView
      answer={ask.answer}
      searchedByMeaning={ask.searchedByMeaning}
    />}
  </Card>
}
