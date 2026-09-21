// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'
import useSWR from 'swr'

// User interface
import { ChoiceList } from '../ActionItems/ChoiceList'

// Misc
import { listJiraFilters } from '../../api/routes/jiraRoutes'
import { toLoadStatus } from '../../hooks/useServerData'

type Props = {
  credentialId: string
  selectedId: string | null
  onSelect: (filterId: string | null) => void
}

// The saved Jira filters a credential's token can see, picked by name.
export function JiraFilterList(props: Props) {
  const { t } = useTranslation('actionItems')
  const listing = useSWR(
    [ 'jira-filters', props.credentialId ],
    ([ , credentialId ]) => listJiraFilters(credentialId),
    { revalidateOnFocus: false, shouldRetryOnError: false },
  )

  const choices = (listing.data?.filters ?? []).map((filter) => ({ id: filter.id, label: filter.name }))

  return <ChoiceList
    label={t('links.picker.results')}
    choices={choices}
    status={toLoadStatus(listing.data !== undefined, listing.error)}
    error={listing.error}
    truncated={listing.data?.truncated ?? false}
    selectedId={props.selectedId}
    onSelect={props.onSelect}
  />
}
