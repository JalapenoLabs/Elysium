// Copyright © 2026 Jalapeno Labs

import type { ContainerTargetDraft } from './containerPresentation'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { OptionSelect } from '../../components/OptionSelect'
import { GithubRepositorySelect } from '../ActionItems/GithubRepositorySelect'
import { JiraIssueSearch } from '../ActionItems/JiraIssueSearch'
import { LinkCredentialSelect } from '../ActionItems/LinkCredentialSelect'
import { GithubContainerList } from './GithubContainerList'
import { JiraFilterList } from './JiraFilterList'

// Misc
import { fromCredentialKey } from '../ActionItems/linkPresentation'
import {
  containerKindLabelKeys,
  containerKindsByProvider,
  EMPTY_CONTAINER_TARGET_DRAFT,
} from './containerPresentation'

type Props = {
  value: ContainerTargetDraft
  onChange: (draft: ContainerTargetDraft) => void
}

// Picks one container through the credential that reaches it: a Jira epic, found by search,
// or saved filter; a GitHub repository's milestone or label.
export function ContainerTargetPicker(props: Props) {
  const { t } = useTranslation('initiatives')
  const draft = props.value
  const credential = fromCredentialKey(draft.credentialKey)

  function selectReference(reference: string | null) {
    props.onChange({ ...draft, reference })
  }

  return <div className='flex flex-col gap-4'>
    <LinkCredentialSelect
      value={draft.credentialKey}
      onChange={(credentialKey) => {
        const provider = fromCredentialKey(credentialKey)?.provider
        props.onChange({
          ...EMPTY_CONTAINER_TARGET_DRAFT,
          credentialKey,
          kind: provider
            ? containerKindsByProvider[provider][0]
            : EMPTY_CONTAINER_TARGET_DRAFT.kind,
        })
      }}
    />

    {credential && <OptionSelect
      label={t('containers.kind')}
      options={containerKindsByProvider[credential.provider].map((kind) => ({
        id: kind,
        label: t(containerKindLabelKeys[kind]),
      }))}
      value={draft.kind}
      onChange={(kind) => {
        const kinds = containerKindsByProvider[credential.provider]
        const chosen = kinds.find((candidate) => candidate === kind) ?? kinds[0]
        // The repository stays, since milestones and labels share it.
        props.onChange({ ...draft, kind: chosen, reference: null })
      }}
    />}

    {credential?.provider === 'jira' && draft.kind === 'epic' && <JiraIssueSearch
      key={credential.credentialId}
      credentialId={credential.credentialId}
      onlyEpics
      selectedKey={draft.reference}
      onSelect={selectReference}
    />}

    {credential?.provider === 'jira' && draft.kind === 'filter' && <JiraFilterList
      credentialId={credential.credentialId}
      selectedId={draft.reference}
      onSelect={selectReference}
    />}

    {credential?.provider === 'github' && <GithubRepositorySelect
      credentialId={credential.credentialId}
      value={draft.repository}
      onChange={(repository) => props.onChange({ ...draft, repository, reference: null })}
    />}

    {credential?.provider === 'github'
      && draft.repository
      && (draft.kind === 'milestone' || draft.kind === 'label')
      && <GithubContainerList
        credentialId={credential.credentialId}
        repository={draft.repository}
        kind={draft.kind}
        selectedReference={draft.reference}
        onSelect={selectReference}
      />}
  </div>
}
