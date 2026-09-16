// Copyright © 2026 Jalapeno Labs

import type { Key } from '@heroui/react'
import type { GithubCredential } from '../api/routes/githubRoutes'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Description, Label, ListBox, Select } from '@heroui/react'

// The two choices that name no token of their own. Every other key is a token's id.
export const INHERIT_GITHUB_TOKEN = 'inherit'
export const NO_GITHUB_TOKEN = 'none'

type Props = {
  label: string
  description?: string
  // What following the level above means here, such as "Workspace default (Work)".
  inheritLabel: string
  // `inherit`, `none`, or a token id.
  value: string
  credentials: GithubCredential[]
  isDisabled?: boolean
  onChange: (value: string) => void
}

// Picks the GitHub token a project or a session works with: follow the level above, no
// token at all, or one token by name.
export function GithubTokenSelect(props: Props) {
  const { t } = useTranslation('github')

  function choose(key: Key | null) {
    if (key === null) {
      console.debug('GithubTokenSelect ignored an empty selection')
      return
    }
    props.onChange(String(key))
  }

  return <Select
    isDisabled={props.isDisabled}
    value={props.value}
    onChange={choose}
  >
    <Label>{props.label}</Label>
    <Select.Trigger>
      <Select.Value />
      <Select.Indicator />
    </Select.Trigger>
    <Select.Popover>
      <ListBox>
        <ListBox.Item id={INHERIT_GITHUB_TOKEN} textValue={props.inheritLabel}>
          {props.inheritLabel}
          <ListBox.ItemIndicator />
        </ListBox.Item>
        <ListBox.Item id={NO_GITHUB_TOKEN} textValue={t('choice.none')}>
          {t('choice.none')}
          <ListBox.ItemIndicator />
        </ListBox.Item>
        {props.credentials.map((credential) => <ListBox.Item
          key={credential.id}
          id={credential.id}
          textValue={credential.name}
        >
          <span className='min-w-0 truncate'>
            {credential.name}
            <span className='ml-2 text-xs opacity-60'>{credential.login}</span>
          </span>
          <ListBox.ItemIndicator />
        </ListBox.Item>)}
      </ListBox>
    </Select.Popover>
    {props.description && <Description>{props.description}</Description>}
  </Select>
}
