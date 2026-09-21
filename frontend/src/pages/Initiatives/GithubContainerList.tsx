// Copyright © 2026 Jalapeno Labs

import type { GithubRepository } from '../../api/routes/githubRoutes'
import type { Choice } from '../ActionItems/ChoiceList'

// Core
import { useTranslation } from 'react-i18next'
import useSWR from 'swr'

// User interface
import { ChoiceList } from '../ActionItems/ChoiceList'

// Misc
import { listGithubRepositoryLabels, listGithubRepositoryMilestones } from '../../api/routes/githubRoutes'
import { toLoadStatus } from '../../hooks/useServerData'

type Props = {
  credentialId: string
  repository: GithubRepository
  kind: 'milestone' | 'label'
  selectedReference: string | null
  onSelect: (reference: string | null) => void
}

type Listing = {
  choices: Choice[]
  truncated: boolean
}

// How each kind is read, as choices a picker lists.
const listByKind = {
  milestone: async (credentialId: string, repository: GithubRepository): Promise<Listing> => {
    const response = await listGithubRepositoryMilestones(credentialId, repository)
    return {
      choices: response.milestones.map((milestone) => ({ id: milestone.reference, label: milestone.title })),
      truncated: response.truncated,
    }
  },
  label: async (credentialId: string, repository: GithubRepository): Promise<Listing> => {
    const response = await listGithubRepositoryLabels(credentialId, repository)
    return {
      choices: response.labels.map((label) => ({ id: label.reference, label: label.name })),
      truncated: response.truncated,
    }
  },
} as const satisfies Record<Props['kind'], (credentialId: string, repository: GithubRepository) => Promise<Listing>>

// A repository's milestones or labels, picked by name.
export function GithubContainerList(props: Props) {
  const { t } = useTranslation('actionItems')
  const listing = useSWR(
    [ 'github-repository-containers', props.credentialId, props.repository.fullName, props.kind ],
    () => listByKind[props.kind](props.credentialId, props.repository),
    { revalidateOnFocus: false, shouldRetryOnError: false },
  )

  return <ChoiceList
    label={t('links.picker.results')}
    choices={listing.data?.choices ?? []}
    status={toLoadStatus(listing.data !== undefined, listing.error)}
    error={listing.error}
    truncated={listing.data?.truncated ?? false}
    selectedId={props.selectedReference}
    onSelect={props.onSelect}
  />
}
