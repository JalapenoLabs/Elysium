// Copyright © 2026 Jalapeno Labs

import type { Choice } from './ChoiceList'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import useSWR from 'swr'

// User interface
import { Input, Label, TextField } from '@heroui/react'
import { ChoiceList } from './ChoiceList'

// Misc
import { searchJiraIssues } from '../../api/routes/jiraRoutes'
import { LINK_SEARCH_DELAY_MS, LINK_SEARCH_RESULTS } from '../../constants'
import { useDebouncedValue } from '../../hooks/useDebouncedValue'
import { toLoadStatus } from '../../hooks/useServerData'
import { buildIssueSearchJql } from './linkPresentation'

type Props = {
  credentialId: string
  // Lists epics only, for an initiative's containers.
  onlyEpics?: boolean
  selectedKey: string | null
  onSelect: (key: string | null) => void
}

// Finds a Jira issue by the words in its title or by its key, through one credential. The
// list is read as the person types, once the typing rests.
export function JiraIssueSearch(props: Props) {
  const { t } = useTranslation('actionItems')
  const [ text, setText ] = useState('')
  const settledText = useDebouncedValue(text, LINK_SEARCH_DELAY_MS)
  const jql = buildIssueSearchJql(settledText, props.onlyEpics)

  const search = useSWR(
    [ 'jira-issue-search', props.credentialId, jql ],
    ([ , credentialId, query ]) => searchJiraIssues(credentialId, query, LINK_SEARCH_RESULTS),
    { revalidateOnFocus: false, shouldRetryOnError: false, keepPreviousData: true },
  )

  const choices: Choice[] = []
  for (const issue of search.data?.issues ?? []) {
    choices.push({
      id: issue.key,
      label: `${issue.key}: ${issue.summary}`,
      detail: [ issue.issueType, issue.status?.name ]
        .filter(Boolean)
        .join(' · '),
    })
  }

  return <div className='flex flex-col gap-3'>
    <TextField
      value={text}
      autoComplete='off'
      onChange={(value) => {
        setText(value)
        // What was picked may not be in the next list.
        props.onSelect(null)
      }}
    >
      <Label>{t('links.picker.search')}</Label>
      <Input placeholder={t('links.picker.searchPlaceholder')} />
    </TextField>
    <ChoiceList
      label={t('links.picker.results')}
      choices={choices}
      status={toLoadStatus(search.data !== undefined, search.error)}
      error={search.error}
      truncated={search.data
        ? !search.data.isLast
        : false}
      selectedId={props.selectedKey}
      onSelect={props.onSelect}
    />
  </div>
}
