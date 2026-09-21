// Copyright © 2026 Jalapeno Labs

import type { ChangesetDecision, ChangesetOperation } from '../../api/routes/changesetRoutes'
import type { DescribeContext } from './changesetPresentation'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Card, Chip } from '@heroui/react'
import { LuCheck, LuX } from 'react-icons/lu'
import { OperationProviderWrites } from './OperationProviderWrites'

// Misc
import {
  decisionLabelKeys,
  dependentsOf,
  describeOperation,
  describeUndo,
  operationKindLabelKeys,
  outcomeChipColors,
  outcomeLabelKeys,
} from './changesetPresentation'

type Props = {
  operation: ChangesetOperation
  context: DescribeContext
  // Whether the changeset still takes decisions: it is pending.
  isDeciding: boolean
  onDecide: (decision: ChangesetDecision) => void
}

// The kinds whose applying owes a provider write the review follows: a comment's post, and
// the closes a resolve owes its item's issues.
const FOLLOWED_KINDS = new Set([ 'comment', 'resolve-item' ])

// One proposed change: what it does as a readable change, why, and where it came from;
// approve and reject while the changeset is pending, and how it ended once applied or undone.
export function ChangesetOperationCard(props: Props) {
  const { t } = useTranslation([ 'changesets', 'actionItems' ])
  const operation = props.operation
  const description = describeOperation(operation, props.context, t)
  const dependents = dependentsOf(props.context.operations, operation.position)
  const itemId = typeof operation.result.itemId === 'string'
    ? operation.result.itemId
    : null
  const commentId = typeof operation.result.commentId === 'string'
    ? operation.result.commentId
    : null
  const followsWrites = operation.outcome === 'applied'
    && itemId !== null
    && FOLLOWED_KINDS.has(operation.operation.kind)
  const undoLines = operation.undo
    ? describeUndo(operation.undo, t)
    : []

  return <Card className='compact'>
    <Card.Content>
      <div className='level compact items-start gap-4'>
        <div className='min-w-0 flex-1'>
          <p className='text-xs opacity-60'>
            <span>{t('review.change', { position: operation.position })}</span>
            <span className='mx-2'>·</span>
            <span>{t(operationKindLabelKeys[operation.operation.kind])}</span>
          </p>
          <p className='font-semibold'>{description.summary}</p>
          {description.details.map((detail, index) => <p
            key={index}
            className='mt-1 line-clamp-4 text-sm whitespace-pre-line opacity-80'
          >{detail}</p>)}
        </div>
        {props.isDeciding
          ? <div className='flex shrink-0 items-center gap-2'>
            <Button
              size='sm'
              variant={operation.decision === 'approved'
                ? 'primary'
                : 'outline'}
              onPress={() => props.onDecide('approved')}
            >
              <LuCheck className='size-4' aria-hidden />
              <span>{t('review.approve')}</span>
            </Button>
            <Button
              size='sm'
              variant={operation.decision === 'rejected'
                ? 'danger'
                : 'outline'}
              onPress={() => props.onDecide('rejected')}
            >
              <LuX className='size-4' aria-hidden />
              <span>{t('review.reject')}</span>
            </Button>
          </div>
          : <div className='flex shrink-0 items-center gap-2'>
            <Chip size='sm' variant='soft'>{t(decisionLabelKeys[operation.decision])}</Chip>
            <Chip size='sm' variant='soft' color={outcomeChipColors[operation.outcome]}>{
              t(outcomeLabelKeys[operation.outcome])
            }</Chip>
          </div>}
      </div>

      <div className='compact text-sm'>
        <p>
          <span className='font-semibold'>{t('review.reason')}</span>
          <span className='ml-2 whitespace-pre-line opacity-80'>{operation.reason}</span>
        </p>
        {operation.quote && <blockquote className='mt-2 border-l-2 border-separator pl-3 italic opacity-80'>{
          operation.quote
        }</blockquote>}
        {operation.source && <p className='mt-1 text-xs opacity-60'>{
          t('review.source', { source: operation.source })
        }</p>}
      </div>

      {operation.dependsOn.length > 0 && <p className='text-xs opacity-60'>{
        t('review.dependsOn', { positions: operation.dependsOn.join(', ') })
      }</p>}
      {props.isDeciding && dependents.length > 0 && <p className='text-xs opacity-60'>{
        t('review.rejectsWith', { positions: dependents.join(', ') })
      }</p>}

      {operation.error && <p className='mt-2 text-sm text-danger'>{operation.error}</p>}
      {followsWrites && itemId && <OperationProviderWrites
        itemId={itemId}
        commentId={operation.operation.kind === 'comment'
          ? commentId
          : null}
      />}
      {undoLines.map((line) => <p key={line} className='mt-2 text-sm text-warning'>{line}</p>)}
    </Card.Content>
  </Card>
}
