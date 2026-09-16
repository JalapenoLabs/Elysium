// Copyright © 2026 Jalapeno Labs

import type { GithubCredential, GithubRepository } from '../../api/routes/githubRoutes'
import type { SessionRepositoryValue } from './createSessionFormSchema'
import type { RepositoryListingStatus } from './sessionRepositories'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Chip, Input, TextField, Tooltip } from '@heroui/react'
import { LuX } from 'react-icons/lu'
import { SessionGithubAccess } from './SessionGithubAccess'

// Misc
import { parseGithubRepository } from '../Settings/Github/githubPresentation'

type Props = {
  value: SessionRepositoryValue
  // The repository as the chosen token's list reports it, or null when the list lacks it.
  listed: GithubRepository | null
  listingStatus: RepositoryListingStatus
  credential: GithubCredential | null
  branchError?: string
  onBranchChange: (baseBranch: string) => void
  onRemove: () => void
}

// One repository the session clones: its name, the branch to start from, and what the chosen
// token can do with it. A warning never blocks the session.
export function SessionRepositoryRow(props: Props) {
  const { t } = useTranslation('coding')

  const githubName = parseGithubRepository(props.value.url)
  const title = props.listed?.fullName ?? props.value.fullName ?? githubName ?? props.value.url
  const values = { repository: title }
  const isPicked = props.value.fullName !== null

  let notice = null
  if (props.listed?.canPush === false) {
    notice = <p className='text-sm text-warning'>{t('create.github.access.readOnly', values)}</p>
  }
  else if (isPicked && props.listingStatus === 'no-token') {
    notice = <p className='text-sm text-warning'>{t('create.repositories.notice.noToken', values)}</p>
  }
  else if (isPicked && props.listingStatus === 'loaded' && !props.listed) {
    notice = <p className='text-sm text-warning'>{t('create.repositories.notice.notListed', values)}</p>
  }
  else if (!isPicked && !props.listed && props.credential && props.listingStatus !== 'loading') {
    // Added by URL and not in the token's list: ask GitHub about this one repository.
    notice = <SessionGithubAccess
      credential={props.credential}
      repositoryUrl={props.value.url}
    />
  }

  return <li className='flex flex-col gap-1 rounded-xl bg-surface-secondary px-3 py-2'>
    <div className='flex items-center gap-2'>
      <div className='flex min-w-0 flex-1 items-center gap-2'>
        <span className='truncate font-medium text-foreground' title={props.value.url}>{title}</span>
        {props.listed?.private && <Chip size='sm' variant='soft'>{
          t('create.repositories.private')
        }</Chip>}
      </div>
      <TextField
        aria-label={t('create.repositories.branchLabel', values)}
        className='w-36 shrink-0'
        isInvalid={Boolean(props.branchError)}
        value={props.value.baseBranch}
        onChange={props.onBranchChange}
      >
        <Input placeholder={props.listed?.defaultBranch ?? t('create.repositories.defaultBranch')} />
      </TextField>
      <Tooltip delay={300}>
        <Button
          isIconOnly
          size='sm'
          variant='ghost'
          aria-label={t('create.repositories.remove', values)}
          onPress={props.onRemove}
        >
          <LuX className='size-4' aria-hidden />
        </Button>
        <Tooltip.Content>
          <span>{t('create.repositories.remove', values)}</span>
        </Tooltip.Content>
      </Tooltip>
    </div>
    {props.branchError && <p className='text-sm text-danger'>{props.branchError}</p>}
    {notice}
  </li>
}
