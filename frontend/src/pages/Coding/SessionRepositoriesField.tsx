// Copyright © 2026 Jalapeno Labs

import type { GithubCredential, GithubRepository } from '../../api/routes/githubRoutes'
import type { SessionRepositoryValue } from './createSessionFormSchema'
import type { RepositoryListingStatus } from './sessionRepositories'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import useSWR from 'swr'

// User interface
import { Button, Description, FieldError, Input, Label, Link, Spinner, TextField } from '@heroui/react'
import { GithubRepositoryPicker } from './GithubRepositoryPicker'
import { SessionRepositoryRow } from './SessionRepositoryRow'

// Utility
import { HTTPError } from 'ky'

// Misc
import { getApiErrorMessage } from '../../api/errors'
import { listGithubRepositories } from '../../api/routes/githubRoutes'
import { UrlTree } from '../../urls'
import { createRepositoryUrlSchema } from './createSessionFormSchema'
import { findRepositoryConflict, getRepositoryIdentity, MAX_SESSION_REPOSITORIES } from './sessionRepositories'

type Props = {
  // The token the session would start with, or null for none.
  credential: GithubCredential | null
  value: SessionRepositoryValue[]
  // A problem with the list as a whole, such as two repositories sharing a directory.
  errorMessage?: string
  // Each row's base branch problem, by position.
  branchErrors: (string | undefined)[]
  onChange: (repositories: SessionRepositoryValue[]) => void
}

// The repositories a session clones: picked from the chosen token's list, or added by URL for
// any other host or a repository the list does not hold. Rows stay when the token changes, and
// their warnings follow the new token.
export function SessionRepositoriesField(props: Props) {
  const { t } = useTranslation('coding')
  const [ manualUrl, setManualUrl ] = useState('')
  const [ manualError, setManualError ] = useState<string | null>(null)
  const credentialId = props.credential?.id

  const listing = useSWR(
    credentialId
      ? [ 'github-repositories', credentialId ]
      : null,
    ([ , id ]) => listGithubRepositories(id),
    { revalidateOnFocus: false, shouldRetryOnError: false },
  )
  const repositories = listing.data?.repositories ?? []

  let listingStatus: RepositoryListingStatus = 'loaded'
  if (!credentialId) {
    listingStatus = 'no-token'
  }
  else if (listing.error) {
    listingStatus = 'failed'
  }
  else if (!listing.data) {
    listingStatus = 'loading'
  }

  const listedByIdentity = new Map<string, GithubRepository>()
  for (const repository of repositories) {
    listedByIdentity.set(getRepositoryIdentity(repository.cloneUrl), repository)
  }

  // A row added by URL for a listed repository counts as picked, so the list shows it chosen.
  const selectedFullNames: string[] = []
  for (const row of props.value) {
    const listed = listedByIdentity.get(getRepositoryIdentity(row.url))
    if (listed) {
      selectedFullNames.push(listed.fullName)
    }
  }

  function pick(fullNames: string[]) {
    const chosen = new Set(fullNames)
    const kept: SessionRepositoryValue[] = []
    const keptIdentities = new Set<string>()
    for (const row of props.value) {
      const identity = getRepositoryIdentity(row.url)
      const listed = listedByIdentity.get(identity)
      if (listed && !chosen.has(listed.fullName)) {
        continue
      }
      kept.push(row)
      keptIdentities.add(identity)
    }

    const added: SessionRepositoryValue[] = []
    for (const repository of repositories) {
      const isNewlyChosen = chosen.has(repository.fullName)
        && !keptIdentities.has(getRepositoryIdentity(repository.cloneUrl))
      if (isNewlyChosen) {
        added.push({ url: repository.cloneUrl, baseBranch: '', fullName: repository.fullName })
      }
    }
    props.onChange([ ...kept, ...added ])
  }

  function addManualUrl() {
    const parsed = createRepositoryUrlSchema(t).safeParse(manualUrl)
    if (!parsed.success) {
      setManualError(parsed.error.issues[0]?.message ?? t('create.errors.repositoryUrlInvalid'))
      return
    }
    if (props.value.length >= MAX_SESSION_REPOSITORIES) {
      setManualError(t('create.repositories.errors.tooMany', { max: MAX_SESSION_REPOSITORIES }))
      return
    }

    const url = parsed.data
    const conflict = findRepositoryConflict([ ...props.value.map((row) => row.url), url ])
    if (conflict) {
      setManualError(conflict.kind === 'duplicate'
        ? t('create.repositories.errors.duplicate', conflict)
        : t('create.repositories.errors.directory', conflict))
      return
    }

    props.onChange([ ...props.value, { url, baseBranch: '', fullName: null }])
    setManualUrl('')
    setManualError(null)
  }

  let pickerDescription = null
  if (listingStatus === 'no-token') {
    pickerDescription = <>
      {t('create.repositories.picker.noToken')}{' '}
      <Link href={UrlTree.settingsGithub} className='text-link'>{
        t('create.repositories.picker.settingsLink')
      }</Link>
    </>
  }
  else if (listingStatus === 'loading') {
    pickerDescription = <span className='flex items-center gap-2'>
      <Spinner size='sm' />
      <span>{t('create.repositories.picker.loading')}</span>
    </span>
  }
  else if (listingStatus === 'failed') {
    // A refused token answers 400 with what to check; anything else is GitHub unreachable.
    const refusal = listing.error instanceof HTTPError && listing.error.response.status === 400
      ? getApiErrorMessage(listing.error)
      : null
    if (!refusal) {
      console.debug('SessionRepositoriesField could not list repositories', { error: listing.error })
    }
    pickerDescription = <span className='text-danger'>{
      refusal ?? t('create.repositories.picker.failed')
    }</span>
  }
  else if (!repositories.length) {
    pickerDescription = t('create.repositories.picker.empty')
  }
  else if (listing.data?.truncated) {
    pickerDescription = t('create.repositories.picker.truncated', { shown: repositories.length })
  }
  else {
    pickerDescription = t('create.repositories.picker.hint')
  }

  return <div className='flex flex-col gap-3'>
    <GithubRepositoryPicker
      repositories={repositories}
      selectedFullNames={selectedFullNames}
      isDisabled={listingStatus !== 'loaded' || !repositories.length}
      description={pickerDescription}
      onChange={pick}
    />

    {props.value.length > 0 && <ul className='flex flex-col gap-2'>{
      props.value.map((row, index) => <SessionRepositoryRow
        key={row.url}
        value={row}
        listed={listedByIdentity.get(getRepositoryIdentity(row.url)) ?? null}
        listingStatus={listingStatus}
        credential={props.credential}
        branchError={props.branchErrors[index]}
        onBranchChange={(baseBranch) => props.onChange(props.value.map((candidate) => candidate === row
          ? { ...candidate, baseBranch }
          : candidate))}
        onRemove={() => props.onChange(props.value.filter((candidate) => candidate !== row))}
      />)
    }</ul>}
    {props.errorMessage && <p className='text-sm text-danger'>{props.errorMessage}</p>}

    {/* Add by URL */}
    <div className='flex items-start gap-2'>
      <TextField
        className='flex-1'
        isInvalid={Boolean(manualError)}
        value={manualUrl}
        onChange={(value) => {
          setManualUrl(value)
          setManualError(null)
        }}
      >
        <Label>{t('create.repositories.manual.label')}</Label>
        <Input
          placeholder='https://github.com/org/repo.git'
          onKeyDown={(event) => {
            // Enter adds the URL here rather than submitting the whole form.
            if (event.key === 'Enter') {
              event.preventDefault()
              addManualUrl()
            }
          }}
        />
        <Description>{t('create.repositories.manual.hint')}</Description>
        <FieldError>{manualError}</FieldError>
      </TextField>
      <Button
        className='mt-6'
        variant='secondary'
        isDisabled={!manualUrl.trim()}
        onPress={addManualUrl}
      >
        <span>{t('create.repositories.manual.add')}</span>
      </Button>
    </div>
  </div>
}
