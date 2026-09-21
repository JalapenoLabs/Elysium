// Copyright © 2026 Jalapeno Labs

import type { Changeset } from '../../api/routes/changesetRoutes'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { buttonVariants, Card, Chip, Link } from '@heroui/react'

// Misc
import { getChangesetViewUrl } from '../../urls'
import { describeActor } from '../ActionItems/actionItemPresentation'
import {
  changesetStateChipColors,
  changesetStateLabelKeys,
  tallyDecisions,
} from './changesetPresentation'

type Props = {
  changeset: Changeset
}

// One changeset in the list: what it is for, who proposed it and when, how many changes it
// holds, and where it stands, opening its review.
export function ChangesetListItem(props: Props) {
  const { t, i18n } = useTranslation([ 'changesets', 'actionItems' ])
  const changeset = props.changeset
  const proposer = describeActor(changeset.proposer)
  const date = new Intl.DateTimeFormat(i18n.language, { dateStyle: 'medium', timeStyle: 'short' })
    .format(new Date(changeset.createdAt))
  const tally = tallyDecisions(changeset)
  const isPending = changeset.state === 'pending'

  return <Card className='compact'>
    <Card.Content className='flex-row flex-wrap items-center justify-between gap-4'>
      <div className='min-w-0 flex-1'>
        <Link
          href={getChangesetViewUrl(changeset.id)}
          className='font-semibold text-link no-underline hover:underline'
        >{
          changeset.summary
        }</Link>
        <p className='text-sm opacity-70'>{
          t('proposedBy', { proposer: t(proposer.key, { ...proposer.values, ns: 'actionItems' }), date })
        }</p>
        <p className='text-sm opacity-70'>{isPending
          ? t('review.tally', tally)
          : t('changeCount', { count: changeset.operations.length })}</p>
      </div>
      <div className='flex shrink-0 items-center gap-2'>
        <Chip size='sm' variant='soft' color={changesetStateChipColors[changeset.state]}>{
          t(changesetStateLabelKeys[changeset.state])
        }</Chip>
        <Link
          href={getChangesetViewUrl(changeset.id)}
          className={buttonVariants({ size: 'sm', variant: isPending
            ? 'primary'
            : 'outline' })}
        >
          <span>{isPending
            ? t('list.review')
            : t('list.open')}</span>
        </Link>
      </div>
    </Card.Content>
  </Card>
}
