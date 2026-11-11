// Copyright © 2026 Jalapeno Labs

import type { MeetingComment } from '../../../api/routes/meetingRoutes'
import type { Thread } from './meetingThreads'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { shallowEqual } from 'react-redux'

// Redux
import { useAppSelector } from '../../../store/hooks'
import { selectUserEntities, selectUserNamesById } from '../../../store/usersSlice'

// User interface
import { Button, Tooltip } from '@heroui/react'
import { LuCheck, LuRotateCcw, LuTrash2 } from 'react-icons/lu'
import { CommentForm } from './CommentForm'

// Misc
import { DateText } from '../../../format/DateText'
import { authorOf } from './meetingReviewPresentation'
import { awaitsElysia, mentionPieces } from './meetingThreads'

export type ThreadActions = {
  onReply: (threadId: string, body: string, mentions: string[]) => Promise<boolean>
  onSetResolved: (threadId: string, resolved: boolean) => Promise<void>
  onRemove: (threadId: string) => Promise<void>
}

type Props = {
  thread: Thread
  // What can be done in it; null where the meeting takes no change, which reads it alone.
  actions: ThreadActions | null
}

type CommentProps = {
  comment: MeetingComment
}

// One comment: who wrote it, an agent saying so, when, and its text with its mentions drawn.
function Comment(props: CommentProps) {
  const { t } = useTranslation([ 'meetings', 'common' ])
  const users = useAppSelector(selectUserEntities)
  const namesById = useAppSelector(selectUserNamesById, shallowEqual)
  const { comment } = props
  const author = authorOf(comment.createdBy, comment.agent, users)
  const pieces = mentionPieces(comment.body, comment.mentions.map((userId) => namesById[userId] ?? ''))

  return <li className='rounded-lg bg-surface p-2'>
    <p className='mb-1 text-xs'>
      <span className='font-semibold'>{author.name ?? t('common:table.someone')}</span>
      <DateText value={comment.createdAt} className='ml-2 opacity-60' />
    </p>
    <p className='text-sm whitespace-pre-line'>{
      pieces.map((piece, index) => piece.isMention
        ? <strong key={index} className='text-link'>{piece.text}</strong>
        : <span key={index}>{piece.text}</span>)
    }</p>
  </li>
}

// A comment thread, open in its lane as the mark selected (docs/meeting-review.md, Comments):
// every comment oldest first, a reply, and Resolve or Reopen and Remove, each with Undo in its
// toast. A thread that asked Elysia says she answers here until she does. Without actions it is
// read alone.
export function CommentThread(props: Props) {
  const { t } = useTranslation('meetings')
  const [ pending, setPending ] = useState<'resolve' | 'remove' | null>(null)
  const { thread, actions } = props
  const threadId = thread.placed.mark.id

  async function act(threadActions: ThreadActions, action: 'resolve' | 'remove') {
    setPending(action)
    try {
      if (action === 'remove') {
        await threadActions.onRemove(threadId)
        return
      }
      await threadActions.onSetResolved(threadId, !thread.isResolved)
    }
    finally {
      setPending(null)
    }
  }

  return <div className='flex flex-col gap-2'>
    <ol className='flex flex-col gap-2'>{
      thread.comments.map((comment) => <Comment key={comment.id} comment={comment} />)
    }</ol>
    {awaitsElysia(thread.comments) && <p className='text-xs opacity-70'>{t('review.comments.elysiaAsked')}</p>}

    {actions && !thread.isResolved && <CommentForm
      draftName={`meeting-thread-reply.${threadId}`}
      label={t('review.comments.replyLabel')}
      submitLabel={t('review.comments.reply')}
      onSubmit={(body, mentions) => actions.onReply(threadId, body, mentions)}
    />}

    {actions && <div className='level gap-2'>
      <Button
        size='sm'
        variant='outline'
        isPending={pending === 'resolve'}
        isDisabled={pending === 'remove'}
        onPress={() => void act(actions, 'resolve')}
      >
        {thread.isResolved
          ? <LuRotateCcw className='size-4' aria-hidden />
          : <LuCheck className='size-4' aria-hidden />}
        <span>{thread.isResolved
          ? t('review.comments.reopen')
          : t('review.comments.resolve')}</span>
      </Button>
      <Tooltip delay={300}>
        <Button
          isIconOnly
          size='sm'
          variant='ghost'
          className='text-danger'
          aria-label={t('review.comments.remove')}
          isPending={pending === 'remove'}
          isDisabled={pending === 'resolve'}
          onPress={() => void act(actions, 'remove')}
        >
          <LuTrash2 className='size-4' aria-hidden />
        </Button>
        <Tooltip.Content>{t('review.comments.remove')}</Tooltip.Content>
      </Tooltip>
    </div>}
  </div>
}
