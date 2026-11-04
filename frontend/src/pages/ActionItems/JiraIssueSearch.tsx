// Copyright © 2026 Jalapeno Labs

import type { Choice } from './ChoiceList'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import useSWR from 'swr'

// User interface
import { Label, SearchField, Spinner } from '@heroui/react'
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
// list is read as the person types, once the typing rests (`~/.claude/docs/ux.md`, Search
// inputs), and the magnifying glass turns into a spinner until Jira answers.
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
  // From the keystroke, since the wait for typing to rest is part of the search.
  const isSearching = text !== settledText || search.isValidating

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
    <SearchField
      id='link-target-search'
      value={text}
      autoComplete='off'
      onChange={(value) => {
        setText(value)
        // What was picked may not be in the next list.
        props.onSelect(null)
      }}
    >
      <Label>{t('links.picker.search')}</Label>
      <SearchField.Group>
        {isSearching
          ? <Spinner size='sm' className='ml-3' aria-label={t('links.picker.searching')} />
          : <SearchField.SearchIcon />}
        <SearchField.Input className='min-w-0' placeholder={t('links.picker.searchPlaceholder')} />
        <SearchField.ClearButton />
      </SearchField.Group>
    </SearchField>
    <ChoiceList
      label={t('links.picker.results')}
      choices={choices}
      // The previous answer stays on screen while the next loads, but a failed search must
      // not pass it off as the answer to the new one.
      status={toLoadStatus(search.data !== undefined && !search.error, search.error)}
      error={search.error}
      truncated={search.data
        ? !search.data.isLast
        : false}
      selectedId={props.selectedKey}
      onSelect={props.onSelect}
    />
  </div>
}
