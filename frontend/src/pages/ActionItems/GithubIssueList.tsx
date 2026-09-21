// Copyright © 2026 Jalapeno Labs

import type { GithubRepository } from '../../api/routes/githubRoutes'
import type { LinkKind } from '../../api/routes/actionItemRoutes'
import type { Choice } from './ChoiceList'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import useSWR from 'swr'

// User interface
import { Input, Label, TextField } from '@heroui/react'
import { ChoiceList } from './ChoiceList'

// Misc
import { listGithubRepositoryIssues } from '../../api/routes/githubRoutes'
import { toLoadStatus } from '../../hooks/useServerData'

type Props = {
  credentialId: string
  repository: GithubRepository
  kind: LinkKind
  selectedReference: string | null
  onSelect: (reference: string | null) => void
}

// A repository's open issues or open pull requests, narrowed by typing. GitHub answers the
// most recently updated in one read, so the narrowing happens here rather than per keystroke.
export function GithubIssueList(props: Props) {
  const { t } = useTranslation('actionItems')
  const [ filter, setFilter ] = useState('')

  const listing = useSWR(
    [ 'github-repository-issues', props.credentialId, props.repository.owner, props.repository.name, props.kind ],
    ([ , credentialId, owner, name, kind ]) => listGithubRepositoryIssues(credentialId, { owner, name }, kind),
    { revalidateOnFocus: false, shouldRetryOnError: false },
  )

  const needle = filter.trim().toLowerCase()
  const choices: Choice[] = []
  for (const issue of listing.data?.issues ?? []) {
    const label = `#${issue.number} ${issue.title}`
    if (needle && !label.toLowerCase().includes(needle)) {
      continue
    }
    choices.push({
      id: issue.reference,
      label,
      detail: issue.assignee ?? undefined,
    })
  }

  return <div className='flex flex-col gap-3'>
    <TextField value={filter} autoComplete='off' onChange={setFilter}>
      <Label>{t('links.picker.filter')}</Label>
      <Input />
    </TextField>
    <ChoiceList
      label={t('links.picker.results')}
      choices={choices}
      status={toLoadStatus(listing.data !== undefined, listing.error)}
      error={listing.error}
      truncated={listing.data?.truncated ?? false}
      selectedId={props.selectedReference}
      onSelect={props.onSelect}
    />
  </div>
}
