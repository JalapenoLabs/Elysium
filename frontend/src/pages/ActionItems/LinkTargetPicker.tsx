// Copyright © 2026 Jalapeno Labs

import type { LinkTargetDraft } from './linkPresentation'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { OptionSelect } from '../../components/OptionSelect'
import { GithubIssueList } from './GithubIssueList'
import { GithubRepositorySelect } from './GithubRepositorySelect'
import { JiraIssueSearch } from './JiraIssueSearch'
import { LinkCredentialSelect } from './LinkCredentialSelect'

// Misc
import { LINK_KINDS } from '../../api/routes/actionItemRoutes'
import { EMPTY_LINK_TARGET_DRAFT, fromCredentialKey, linkKindLabelKeys } from './linkPresentation'

type Props = {
  value: LinkTargetDraft
  onChange: (draft: LinkTargetDraft) => void
}

// Picks one Jira issue, GitHub issue, or GitHub pull request, through the credential that
// reaches it. Jira is searched; GitHub lists a repository's open issues or pull requests.
export function LinkTargetPicker(props: Props) {
  const { t } = useTranslation('actionItems')
  const credential = fromCredentialKey(props.value.credentialKey)

  const kindOptions = LINK_KINDS.map((kind) => ({ id: kind, label: t(linkKindLabelKeys[kind]) }))

  return <div className='flex flex-col gap-4'>
    <LinkCredentialSelect
      value={props.value.credentialKey}
      // Everything below was found through the previous credential.
      onChange={(credentialKey) => props.onChange({ ...EMPTY_LINK_TARGET_DRAFT, credentialKey })}
    />

    {credential?.provider === 'jira' && <JiraIssueSearch
      // A new credential starts a new search.
      key={credential.credentialId}
      credentialId={credential.credentialId}
      selectedKey={props.value.reference}
      onSelect={(reference) => props.onChange({ ...props.value, reference })}
    />}

    {credential?.provider === 'github' && <>
      <OptionSelect
        label={t('links.picker.kind')}
        options={kindOptions}
        value={props.value.kind}
        onChange={(kind) => props.onChange({
          ...props.value,
          kind: LINK_KINDS.find((candidate) => candidate === kind) ?? 'issue',
          reference: null,
        })}
      />
      <GithubRepositorySelect
        credentialId={credential.credentialId}
        value={props.value.repository}
        onChange={(repository) => props.onChange({ ...props.value, repository, reference: null })}
      />
      {props.value.repository && <GithubIssueList
        key={props.value.repository.fullName}
        credentialId={credential.credentialId}
        repository={props.value.repository}
        kind={props.value.kind}
        selectedReference={props.value.reference}
        onSelect={(reference) => props.onChange({ ...props.value, reference })}
      />}
    </>}
  </div>
}
