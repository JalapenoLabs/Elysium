// Copyright © 2026 Jalapeno Labs

import type { Key } from '@heroui/react'
import type { GithubRepository } from '../../api/routes/githubRoutes'

// Core
import { useTranslation } from 'react-i18next'
import useSWR from 'swr'

// User interface
import { ComboBox, Description, EmptyState, Input, Label, ListBox } from '@heroui/react'

// Misc
import { getApiErrorMessage } from '../../api/errors'
import { listGithubRepositories } from '../../api/routes/githubRoutes'

type Props = {
  credentialId: string
  value: GithubRepository | null
  onChange: (repository: GithubRepository | null) => void
}

// One repository a GitHub token can see, found by typing its name. Shares its listing with
// the coding session form, so opening both reads GitHub once.
export function GithubRepositorySelect(props: Props) {
  const { t } = useTranslation([ 'actionItems', 'common' ])
  const listing = useSWR(
    [ 'github-repositories', props.credentialId ],
    ([ , credentialId ]) => listGithubRepositories(credentialId),
    { revalidateOnFocus: false, shouldRetryOnError: false },
  )
  const repositories = listing.data?.repositories ?? []

  function select(key: Key | null) {
    const repository = repositories.find((candidate) => candidate.fullName === key) ?? null
    props.onChange(repository)
  }

  return <ComboBox
    className='w-full'
    isDisabled={!listing.data}
    selectedKey={props.value?.fullName ?? null}
    onSelectionChange={select}
  >
    <Label>{t('links.picker.repository')}</Label>
    <ComboBox.InputGroup>
      <Input />
      <ComboBox.Trigger />
    </ComboBox.InputGroup>
    {listing.error && <Description className='text-danger'>{
      t('links.picker.loadError', {
        error: getApiErrorMessage(listing.error) ?? t('common:errors.unexpected'),
      })
    }</Description>}
    {listing.data?.truncated && <Description>{t('links.picker.truncated')}</Description>}
    <ComboBox.Popover>
      <ListBox renderEmptyState={() => <EmptyState>{t('links.picker.noResults')}</EmptyState>}>{
        repositories.map((repository) => <ListBox.Item
          key={repository.fullName}
          id={repository.fullName}
          textValue={repository.fullName}
        >
          {repository.fullName}
          <ListBox.ItemIndicator />
        </ListBox.Item>)
      }</ListBox>
    </ComboBox.Popover>
  </ComboBox>
}
