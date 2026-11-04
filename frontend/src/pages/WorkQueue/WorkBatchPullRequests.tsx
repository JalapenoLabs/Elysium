// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'
import useSWR from 'swr'

// User interface
import { Chip, Link, Spinner } from '@heroui/react'
import { LuExternalLink } from 'react-icons/lu'

// Misc
import { getWorkBatchPullRequests, getWorkBatchPullRequestsKey } from '../../api/routes/workQueueRoutes'
import { pullRequestStanding } from './workQueuePresentation'

type Props = {
  batchId: string
}

// The pull requests a batch's worker recorded, each with who merged it, or why the last look
// left it open. Read from SWR alone, since no other view shows them, and refetched on every
// `workBatch.changed`.
export function WorkBatchPullRequests(props: Props) {
  const { t } = useTranslation('workQueue')
  const listing = useSWR(
    getWorkBatchPullRequestsKey(props.batchId),
    () => getWorkBatchPullRequests(props.batchId),
  )
  const pullRequests = listing.data?.pullRequests

  let body = <Spinner size='sm' />
  if (!pullRequests && listing.error) {
    body = <p className='text-sm text-danger'>{t('batch.pullRequests.loadError')}</p>
  }
  else if (pullRequests && !pullRequests.length) {
    body = <p className='text-sm opacity-70'>{t('batch.pullRequests.empty')}</p>
  }
  else if (pullRequests) {
    body = <ul className='flex flex-col gap-3 text-sm'>{
      pullRequests.map((pullRequest) => {
        const standing = pullRequestStanding(pullRequest)
        return <li key={pullRequest.id}>
          <div className='level justify-start gap-2'>
            <Link
              href={pullRequest.url}
              target='_blank'
              rel='noreferrer'
              className='gap-1 font-mono'
            >
              <span>{`${pullRequest.repository}#${pullRequest.number}`}</span>
              <LuExternalLink className='size-3.5' aria-hidden />
            </Link>
            <Chip size='sm' variant='soft' color={standing.color}>{
              t(standing.labelKey, { login: standing.login })
            }</Chip>
          </div>
          <p>{pullRequest.title}</p>
          {standing.note && <p className='opacity-70'>{standing.note}</p>}
        </li>
      })
    }</ul>
  }

  return <section className='relaxed max-w-3xl'>
    <h2 className='compact text-lg font-semibold'>{t('batch.pullRequests.title')}</h2>
    {body}
  </section>
}
