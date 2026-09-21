// Copyright © 2026 Jalapeno Labs

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
import { Alert, Breadcrumbs, Button, Chip, Link, Spinner, Tooltip } from '@heroui/react'
import { LuCheckCheck, LuUndo2, LuX } from 'react-icons/lu'
import { ChangesetOperationCard } from './ChangesetOperationCard'

// Misc
import {
  useActionItemsLoader,
  useChangesetLoader,
  useInitiativesLoader,
  useProjectsLoader,
} from '../../hooks/useServerData'
import { UrlTree } from '../../urls'
import { describeActor } from '../ActionItems/actionItemPresentation'
import { changesetStateChipColors, changesetStateLabelKeys, tallyDecisions } from './changesetPresentation'
import { useChangesetActions } from './useChangesetActions'

// `/action-items/changesets/:changesetId`: one changeset's review. While it is pending, each
// change is approved or rejected, all at once or one by one, and applied once every change
// is decided. After, each change shows how it ended, and an applied changeset can be undone.
export function ChangesetPage() {
  const { t, i18n } = useTranslation([ 'changesets', 'actionItems' ])
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

  const dateFormatter = new Intl.DateTimeFormat(i18n.language, { dateStyle: 'medium', timeStyle: 'short' })
  const context: DescribeContext = {
    operations: changeset.operations,
    itemsById,
    initiativeNames,
    projectNames,
    formatInstant: (instant) => dateFormatter.format(new Date(instant)),
  }
  const proposer = describeActor(changeset.proposer)
  const tally = tallyDecisions(changeset)
  const isPending = changeset.state === 'pending'

  return <div className='container'>
    <Breadcrumbs className='compact'>
      <Breadcrumbs.Item href={UrlTree.actionItems}>{t('actionItems:title')}</Breadcrumbs.Item>
      <Breadcrumbs.Item href={UrlTree.changesets}>{t('review.breadcrumb')}</Breadcrumbs.Item>
      <Breadcrumbs.Item>{changeset.summary}</Breadcrumbs.Item>
    </Breadcrumbs>

    <div className='level relaxed items-start gap-4'>
      <div className='min-w-0 flex-1'>
        <h1 className='text-3xl font-bold'>{changeset.summary}</h1>
        <p className='mt-1 text-sm opacity-70'>{
          t('proposedBy', {
            proposer: t(proposer.key, { ...proposer.values, ns: 'actionItems' }),
            date: context.formatInstant(changeset.createdAt),
          })
        }</p>
        {changeset.undoneAt && <p className='text-sm opacity-70'>{
          t('review.undoneAt', { date: context.formatInstant(changeset.undoneAt) })
        }</p>}
      </div>
      <Chip variant='soft' color={changesetStateChipColors[changeset.state]}>{
        t(changesetStateLabelKeys[changeset.state])
      }</Chip>
    </div>

    {isPending && <div className='relaxed'>
      <Alert status='accent' className='compact'>
        <Alert.Indicator />
        <Alert.Content>
          <Alert.Description>{t('review.pendingBody')}</Alert.Description>
        </Alert.Content>
      </Alert>
      <div className='flex flex-wrap items-center gap-2'>
        <Button size='sm' variant='outline' onPress={() => actions.decide(changeset, 'approved')}>
          <LuCheckCheck className='size-4' aria-hidden />
          <span>{t('review.approveAll')}</span>
        </Button>
        <Button size='sm' variant='outline' onPress={() => actions.decide(changeset, 'rejected')}>
          <LuX className='size-4' aria-hidden />
          <span>{t('review.rejectAll')}</span>
        </Button>
        <span className='ml-auto text-sm opacity-70'>{t('review.tally', tally)}</span>
        <Tooltip delay={300} isDisabled={tally.canApply}>
          <Tooltip.Trigger>
            <div>
              <Button size='sm' isDisabled={!tally.canApply} onPress={() => actions.apply(changeset)}>
                <span>{t('review.apply')}</span>
              </Button>
            </div>
          </Tooltip.Trigger>
          <Tooltip.Content>
            <span>{t('review.applyBlocked')}</span>
          </Tooltip.Content>
        </Tooltip>
      </div>
    </div>}

    {changeset.state === 'applied' && <div className='relaxed flex flex-wrap items-center gap-3'>
      <Button size='sm' variant='outline' onPress={() => actions.undo(changeset)}>
        <LuUndo2 className='size-4' aria-hidden />
        <span>{t('review.undo')}</span>
      </Button>
      <p className='text-sm opacity-70'>{t('review.undoLimits')}</p>
    </div>}

    <div className='relaxed'>{
      changeset.operations.map((operation) => <ChangesetOperationCard
        key={operation.id}
        operation={operation}
        context={context}
        isDeciding={isPending}
        onDecide={(decision) => actions.decide(changeset, decision, [ operation.id ])}
      />)
    }</div>
  </div>
}
