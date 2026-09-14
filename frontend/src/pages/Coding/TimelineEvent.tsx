// Copyright © 2026 Jalapeno Labs

import type { TFunction } from 'i18next'
import type { ReactNode } from 'react'
import type { SessionEvent, SessionEventPayload } from '../../api/routes/codingSessionRoutes'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import {
  LuBrain,
  LuCircleAlert,
  LuCircleCheck,
  LuCircleX,
  LuListChecks,
  LuMessageCircleQuestion,
  LuWrench,
} from 'react-icons/lu'

// Misc
import { turnStateLabelKeys } from './sessionPresentation'

type Props = {
  event: SessionEvent
  // Tool names from earlier `toolStarted` events, for completions that omit theirs.
  toolNamesByCallId: ReadonlyMap<string, string>
}

type PayloadKind = SessionEventPayload['kind']

type RenderContext = {
  t: TFunction<'coding'>
  toolNamesByCallId: ReadonlyMap<string, string>
}

type Renderers = {
  [Kind in PayloadKind]: (
    payload: Extract<SessionEventPayload, { kind: Kind }>,
    context: RenderContext,
  ) => ReactNode
}

const BUBBLE_CLASS_NAME = 'max-w-[85%] rounded-2xl px-4 py-2'
const CODE_BLOCK_CLASS_NAME = 'mt-1 max-h-60 overflow-auto rounded-md bg-surface-secondary p-2 font-mono'

// Formats a tool's elapsed time for a sentence: "850 ms", "2.4 s".
function formatElapsed(milliseconds: number) {
  if (milliseconds < 1000) {
    return `${milliseconds} ms`
  }
  return `${(milliseconds / 1000).toFixed(1)} s`
}

// One renderer per payload kind. Prompts and replies read as a chat; tool calls and
// lifecycle events are compact lines, with details folded away.
const renderers: Renderers = {
  turnStarted: (payload, { t }) => <div className={`ml-auto rounded-br-sm bg-accent/15 ${BUBBLE_CLASS_NAME}`}>
    <div className='mb-0.5 text-xs font-semibold opacity-60'>{t('conversation.you')}</div>
    <div className='text-sm whitespace-pre-wrap break-words'>{payload.prompt}</div>
  </div>,

  agentMessage: (payload, { t }) => <div className={`rounded-bl-sm bg-surface-secondary ${BUBBLE_CLASS_NAME}`}>
    <div className='mb-0.5 text-xs font-semibold opacity-60'>{
      payload.author?.role ?? t('conversation.agent')
    }</div>
    <div className='text-sm whitespace-pre-wrap break-words'>{payload.text}</div>
  </div>,

  agentThinking: (payload, { t }) => <details className='text-xs opacity-60'>
    <summary className='level-left cursor-pointer gap-1.5'>
      <LuBrain className='size-3.5' aria-hidden />
      <span>{t('conversation.thinking')}</span>
    </summary>
    <p className='mt-1 pl-5 whitespace-pre-wrap italic'>{payload.text}</p>
  </details>,

  toolStarted: (payload, { t }) => <details className='text-xs'>
    <summary className='level-left cursor-pointer gap-1.5 opacity-70'>
      <LuWrench className='size-3.5' aria-hidden />
      <span>{t('conversation.toolRunning', { tool: payload.toolName })}</span>
    </summary>
    {payload.input !== null && <pre className={CODE_BLOCK_CLASS_NAME}>{
      JSON.stringify(payload.input, null, 2)
    }</pre>}
  </details>,

  toolCompleted: (payload, { t, toolNamesByCallId }) => {
    const tool = payload.toolName || toolNamesByCallId.get(payload.toolCallId) || t('conversation.unnamedTool')

    let label = t('conversation.toolFailed', { tool })
    if (payload.ok && payload.elapsedMilliseconds === null) {
      label = t('conversation.toolSucceededUntimed', { tool })
    }
    else if (payload.ok && payload.elapsedMilliseconds !== null) {
      label = t('conversation.toolSucceeded', {
        tool,
        duration: formatElapsed(payload.elapsedMilliseconds),
      })
    }

    return <details className='text-xs'>
      <summary className={`level-left cursor-pointer gap-1.5 ${payload.ok ? 'opacity-70' : 'text-danger'}`}>
        {payload.ok
          ? <LuCircleCheck className='size-3.5' aria-hidden />
          : <LuCircleX className='size-3.5' aria-hidden />}
        <span>{label}</span>
      </summary>
      {payload.outputPreview && <pre className={`${CODE_BLOCK_CLASS_NAME} whitespace-pre-wrap`}>{
        payload.outputPreview
      }</pre>}
    </details>
  },

  turnCompleted: (payload, { t }) => <div className='py-1'>
    <div className='level gap-3 text-xs opacity-60'>
      <span className='h-px flex-1 bg-separator' />
      <span>{
        t('conversation.turnFinished', { status: t(turnStateLabelKeys[payload.status]).toLowerCase() })
      }</span>
      <span className='h-px flex-1 bg-separator' />
    </div>
    {payload.summary && <p className='mt-2 text-sm whitespace-pre-wrap opacity-80'>{payload.summary}</p>}
    {payload.error && <p className='mt-2 text-sm text-danger'>{payload.error}</p>}
  </div>,

  planProposed: (payload, { t }) => <div className='rounded-xl border border-separator p-3'>
    <div className='level-left compact gap-1.5 text-xs font-semibold'>
      <LuListChecks className='size-3.5' aria-hidden />
      <span>{t('conversation.plan')}</span>
    </div>
    <div className='text-sm whitespace-pre-wrap'>{payload.body}</div>
  </div>,

  questionAsked: (payload, { t }) => <div className='rounded-xl border border-warning/50 p-3'>
    <div className='level-left compact gap-1.5 text-xs font-semibold text-warning'>
      <LuMessageCircleQuestion className='size-3.5' aria-hidden />
      <span>{t('conversation.question')}</span>
    </div>
    <ul className='flex flex-col gap-2 text-sm'>{
      payload.questions.map((question) => <li key={question.title}>
        <div className='font-medium'>{question.title}</div>
        {question.detail && <div className='opacity-70'>{question.detail}</div>}
        {question.options.length > 0 && <div className='mt-1 text-xs opacity-70'>{
          question.options.join(' · ')
        }</div>}
      </li>)
    }</ul>
  </div>,

  budgetWarning: (payload, { t }) => <p className='level-left gap-1.5 text-xs text-warning'>
    <LuCircleAlert className='size-3.5' aria-hidden />
    <span>{t('conversation.budgetWarning', { percent: payload.percentUsed })}</span>
  </p>,

  incident: (payload, { t }) => <p className='level-left gap-1.5 text-xs text-danger'>
    <LuCircleAlert className='size-3.5 shrink-0' aria-hidden />
    <span>{t('conversation.incident', { message: payload.message || payload.code })}</span>
  </p>,
}

export function TimelineEvent(props: Props) {
  const { t } = useTranslation('coding')
  const payload = props.event.payload

  // The API passes through event types it does not render; show their name quietly.
  if (!payload) {
    return <p className='text-center font-mono text-[11px] opacity-40'>{
      t('conversation.otherEvent', { type: props.event.type })
    }</p>
  }

  const render = renderers[payload.kind] as (payload: SessionEventPayload, context: RenderContext) => ReactNode
  return render(payload, { t, toolNamesByCallId: props.toolNamesByCallId })
}
