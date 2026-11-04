// Copyright © 2026 Jalapeno Labs

import type { PageAction } from '../../layout/page/pageActions'
import type { DescribeContext } from './changesetPresentation'

// Core
import { useTranslation } from 'react-i18next'
import { useParams } from 'react-router'
import { shallowEqual } from 'react-redux'

// Redux
import { useAppSelector } from '../../store/hooks'
import { selectLiveActionItemsById } from '../../store/actionItemsSlice'
import { selectChangesetById } from '../../store/changesetsSlice'
import { selectInitiativeNamesById } from '../../store/initiativesSlice'
import { selectProjectNamesById } from '../../store/projectsSlice'

// User interface
import { Button, Chip, Link, Spinner, Tooltip } from '@heroui/react'
import { LuCheckCheck, LuUndo2, LuX } from 'react-icons/lu'
import { Page } from '../../layout/page/Page'
import { ChangesetOperationCard } from './ChangesetOperationCard'

// Misc
import { formatDateTime } from '../../format/dates'
import {
  useActionItemsLoader,
  useChangesetLoader,
  useInitiativesLoader,
  useProjectsLoader,
} from '../../hooks/useServerData'
import { usePendingAction } from '../../hooks/usePendingAction'
import { useUserNames } from '../../hooks/useUserNames'
import { UrlTree } from '../../urls'
import { describeActor } from '../ActionItems/actionItemPresentation'
import { changesetStateChipColors, changesetStateLabelKeys, tallyDecisions } from './changesetPresentation'
import { useChangesetActions } from './useChangesetActions'

// `/action-items/changesets/:changesetId`: one changeset's review. While it is pending, each
// change is approved or rejected, all at once or one by one, and applied once every change
// is decided. After, each change shows how it ended, and an applied changeset can be undone.
export function ChangesetPage() {
  const { t } = useTranslation([ 'changesets', 'actionItems', 'common' ])
  const actions = useChangesetActions()
  const { changesetId = '' } = useParams()
  const status = useChangesetLoader(changesetId)
  useActionItemsLoader()
  useInitiativesLoader()
  useProjectsLoader()
  const changeset = useAppSelector((state) => selectChangesetById(state, changesetId))
  const itemsById = useAppSelector(selectLiveActionItemsById)
  const initiativeNames = useAppSelector(selectInitiativeNamesById, shallowEqual)
  const projectNames = useAppSelector(selectProjectNamesById, shallowEqual)
  const userNames = useUserNames()
  // The write in flight: 'apply', 'approved' or 'rejected' for all, or `<operation id>:<decision>`.
  // Every write button waits while one runs, so a second press cannot race the first.
  const writing = usePendingAction<string>()
  const pendingWrite = writing.pendingKey

  if (!changeset) {
    return <div className='container'>{
      status === 'loading'
        ? <div className='grid place-items-center py-16'>
          <Spinner />
        </div>
        : <div className='py-16 text-center'>
          <p className='compact text-sm opacity-70'>{t('review.loadError')}</p>
          <Link href={UrlTree.changesets} className='text-link'>{t('review.breadcrumb')}</Link>
        </div>
    }</div>
  }

  const context: DescribeContext = {
    operations: changeset.operations,
    itemsById,
    initiativeNames,
    projectNames,
    formatInstant: (instant) => formatDateTime(instant),
  }
  const proposer = describeActor(changeset.proposer)
  const tally = tallyDecisions(changeset)
  const isPending = changeset.state === 'pending'
  const write = writing.run

  const reviewActions: PageAction[] = []
  if (isPending) {
    reviewActions.push(
      {
        id: 'approveAll',
        label: t('review.approveAll'),
        icon: LuCheckCheck,
        isDisabled: writing.isOtherPending('approved'),
        isPending: pendingWrite === 'approved',
        onAction: () => void write('approved', () => actions.decide(changeset, 'approved')),
      },
      {
        id: 'rejectAll',
        label: t('review.rejectAll'),
        icon: LuX,
        isDisabled: writing.isOtherPending('rejected'),
        isPending: pendingWrite === 'rejected',
        onAction: () => void write('rejected', () => actions.decide(changeset, 'rejected')),
      },
    )
  }
  if (changeset.state === 'applied') {
    // It confirms first, saying what undo cannot take back.
    reviewActions.push({
      id: 'undo',
      label: t('review.undo'),
      icon: LuUndo2,
      onAction: () => actions.undo(changeset),
    })
  }

  return <Page
    title={changeset.summary}
    help={t('review.help')}
    badges={<Chip variant='soft' color={changesetStateChipColors[changeset.state]}>{
      t(changesetStateLabelKeys[changeset.state])
    }</Chip>}
    parents={[
      { label: t('actionItems:title'), href: UrlTree.actionItems },
      { label: t('review.breadcrumb'), href: UrlTree.changesets },
    ]}
    primaryAction={isPending && <Tooltip delay={300} isDisabled={tally.canApply}>
      <Tooltip.Trigger>
        <div>
          <Button
            isDisabled={!tally.canApply || writing.isOtherPending('apply')}
            isPending={pendingWrite === 'apply'}
            onPress={() => write('apply', () => actions.apply(changeset))}
          >
            <span>{t('review.apply')}</span>
          </Button>
        </div>
      </Tooltip.Trigger>
      <Tooltip.Content>
        <span>{t('review.applyBlocked')}</span>
      </Tooltip.Content>
    </Tooltip>}
    actions={reviewActions}
  >
    <div className='relaxed text-sm opacity-70'>
      <p>{
        t('proposedBy', {
          proposer: t(proposer.key, { ...proposer.values, ns: 'actionItems' }),
          date: context.formatInstant(changeset.createdAt),
        })
      }</p>
      {changeset.decidedBy && changeset.decidedAt && <p>{
        t('review.decidedBy', {
          name: userNames[changeset.decidedBy] ?? t('common:table.someone'),
          date: context.formatInstant(changeset.decidedAt),
        })
      }</p>}
      {changeset.undoneAt && <p>{
        t('review.undoneAt', { date: context.formatInstant(changeset.undoneAt) })
      }</p>}
      {isPending && <p>{t('review.tally', tally)}</p>}
    </div>

    <div className='relaxed'>{
      changeset.operations.map((operation) => <ChangesetOperationCard
        key={operation.id}
        operation={operation}
        context={context}
        isDeciding={isPending}
        isOtherWriting={writing.isOtherPending}
        pendingWrite={pendingWrite}
        onDecide={(decision) => write(
          `${operation.id}:${decision}`,
          () => actions.decide(changeset, decision, [ operation.id ]),
        )}
      />)
    }</div>
  </Page>
}
