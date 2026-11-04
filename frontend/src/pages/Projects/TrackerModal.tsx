// Copyright © 2026 Jalapeno Labs

import type { Key, UseOverlayStateReturn } from '@heroui/react'
import type { LinkProvider } from '../../api/routes/actionItemRoutes'
import type { ProjectTracker } from '../../api/routes/projectManagerRoutes'
import type { TrackerDraft } from './trackerPresentation'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import useSWR from 'swr'

// Redux
import { useAppDispatch, useAppSelector } from '../../store/hooks'
import { selectMe } from '../../store/authSlice'
import { selectAllConnectors } from '../../store/connectorsSlice'
import { selectAllJiraCredentials } from '../../store/jiraCredentialsSlice'
import { projectManagerLoaded } from '../../store/projectManagersSlice'

// User interface
import { Button, ComboBox, Description, EmptyState, Form, Input, Label, ListBox, Modal } from '@heroui/react'
import { OptionSelect } from '../../components/OptionSelect'

// Misc
import { LINK_PROVIDERS } from '../../api/routes/actionItemRoutes'
import { getApiErrorMessage } from '../../api/errors'
import {
  clearProjectTracker,
  getTrackerDestinationsKey,
  listTrackerDestinations,
  setProjectTracker,
} from '../../api/routes/projectManagerRoutes'
import { submitOnEnter } from '../../components/submitOnEnter'
import { useConnectorsLoader, useJiraCredentialsLoader } from '../../hooks/useServerData'
import { providerLabelKeys } from '../ActionItems/linkPresentation'
import {
  credentialKindByProvider,
  destinationLabelKeys,
  draftCredential,
  toTrackerRequest,
  trackerDraftFrom,
  youtrackAccountOptions,
} from './trackerPresentation'
import { toast } from '../../toast/toast'

type Props = {
  state: UseOverlayStateReturn
  projectId: string
  // The tracker it files in now, which the picker opens on.
  tracker: ProjectTracker | null
}

// Names the tracker a project's issues file in: a provider, the account that files there, and
// the place, listed from what that account reaches. A named tracker can also go back to
// following where the project's issues already are.
export function TrackerModal(props: Props) {
  const { t } = useTranslation([ 'projects', 'actionItems', 'common' ])
  const dispatch = useAppDispatch()
  useJiraCredentialsLoader()
  useConnectorsLoader()
  const jiraCredentials = useAppSelector(selectAllJiraCredentials)
  const connectors = useAppSelector(selectAllConnectors)
  const me = useAppSelector(selectMe)
  const [ draft, setDraft ] = useState<TrackerDraft>(() => trackerDraftFrom(props.tracker))
  const [ pending, setPending ] = useState<'save' | 'clear' | null>(null)

  const credential = draftCredential(draft)
  const destinations = useSWR(
    props.state.isOpen && credential
      ? getTrackerDestinationsKey(props.projectId, draft.provider, credential.credentialId)
      : null,
    ([ , projectId, provider, credentialId ]) => listTrackerDestinations(projectId, provider, credentialId || null),
    { revalidateOnFocus: false, shouldRetryOnError: false },
  )
  const request = toTrackerRequest(draft)

  const credentialKind = credentialKindByProvider[draft.provider]
  const accountOptions = credentialKind === 'jira-credential'
    ? jiraCredentials.map((jiraCredential) => ({ id: jiraCredential.id, label: jiraCredential.name }))
    : youtrackAccountOptions(connectors, me?.user.id ?? null)

  function close() {
    // The next opening starts from the tracker the project has; one saved since remounts this.
    setDraft(trackerDraftFrom(props.tracker))
    props.state.close()
  }

  async function save() {
    if (!request || pending) {
      console.debug('TrackerModal ignored a save', { hasRequest: Boolean(request), pending })
      return
    }
    setPending('save')
    try {
      const response = await setProjectTracker(props.projectId, request)
      dispatch(projectManagerLoaded(response.projectManager))
      toast.success(t('manager.writes.trackerSaved'))
      close()
    }
    catch (error) {
      console.debug('TrackerModal failed to name a tracker', { error })
      toast.danger(getApiErrorMessage(error) ?? t('common:errors.unexpected'))
    }
    finally {
      setPending(null)
    }
  }

  async function followPrecedent() {
    setPending('clear')
    try {
      const response = await clearProjectTracker(props.projectId)
      dispatch(projectManagerLoaded(response.projectManager))
      toast.success(t('manager.writes.trackerCleared'))
      close()
    }
    catch (error) {
      console.debug('TrackerModal failed to stop naming a tracker', { error })
      toast.danger(getApiErrorMessage(error) ?? t('common:errors.unexpected'))
    }
    finally {
      setPending(null)
    }
  }

  return <Modal.Backdrop
    isOpen={props.state.isOpen}
    onOpenChange={(isOpen) => {
      if (!isOpen) {
        close()
      }
    }}
  >
    <Modal.Container>
      <Modal.Dialog className='sm:max-w-lg'>
        <Modal.CloseTrigger />
        <Modal.Header>
          <Modal.Heading>{t('manager.writes.modalTitle')}</Modal.Heading>
        </Modal.Header>
        <Form
          validationBehavior='aria'
          onKeyDownCapture={submitOnEnter}
          onSubmit={(event) => {
            event.preventDefault()
            void save()
          }}
        >
          <Modal.Body className='mt-2 flex flex-col gap-4'>
            <OptionSelect
              id='tracker-provider'
              label={t('manager.writes.provider')}
              options={LINK_PROVIDERS.map((provider) => ({
                id: provider,
                label: t(providerLabelKeys[provider], { ns: 'actionItems' }),
              }))}
              value={draft.provider}
              onChange={(value) => {
                const provider = LINK_PROVIDERS.find((candidate) => candidate === value)
                if (!provider) {
                  console.debug('TrackerModal received a provider it does not know', { value })
                  return
                }
                setDraft({ provider, credentialId: '', destination: '' })
              }}
            />

            {credentialKind === 'project-token'
              ? <p className='text-sm opacity-70'>{t('manager.writes.projectToken')}</p>
              : <OptionSelect
                id='tracker-account'
                label={t('manager.writes.account')}
                placeholder={accountOptions.length
                  ? undefined
                  : t('manager.writes.noAccounts')}
                options={accountOptions}
                value={draft.credentialId}
                isDisabled={!accountOptions.length}
                onChange={(credentialId) => setDraft({ ...draft, credentialId, destination: '' })}
              />}

            {credential && <DestinationSelect
              value={draft.destination}
              options={destinations.data?.destinations ?? []}
              isLoading={destinations.isLoading}
              error={destinations.error
                ? getApiErrorMessage(destinations.error) ?? t('common:errors.unexpected')
                : null}
              provider={draft.provider}
              onChange={(destination) => setDraft({ ...draft, destination })}
            />}
          </Modal.Body>
          <Modal.Footer>
            {props.tracker?.source === 'named' && <Button
              variant='tertiary'
              className='mr-auto'
              isPending={pending === 'clear'}
              isDisabled={pending !== null}
              onPress={() => void followPrecedent()}
            >
              <span>{t('manager.writes.followPrecedent')}</span>
            </Button>}
            <Button slot='close' variant='tertiary'>
              <span>{t('manager.writes.neverMind')}</span>
            </Button>
            <Button type='submit' isDisabled={!request || pending !== null} isPending={pending === 'save'}>
              <span>{t('common:actions.save')}</span>
            </Button>
          </Modal.Footer>
        </Form>
      </Modal.Dialog>
    </Modal.Container>
  </Modal.Backdrop>
}

type DestinationSelectProps = {
  value: string
  options: { reference: string, name: string }[]
  isLoading: boolean
  error: string | null
  provider: LinkProvider
  onChange: (destination: string) => void
}

// One place the account can file, found by typing its name or key.
function DestinationSelect(props: DestinationSelectProps) {
  const { t } = useTranslation([ 'projects', 'actionItems' ])
  return <ComboBox
    id='tracker-destination'
    className='w-full'
    isDisabled={props.isLoading || Boolean(props.error)}
    selectedKey={props.value || null}
    onSelectionChange={(key: Key | null) => props.onChange(key === null
      ? ''
      : String(key))}
  >
    <Label>{t(destinationLabelKeys[props.provider])}</Label>
    <ComboBox.InputGroup>
      <Input />
      <ComboBox.Trigger />
    </ComboBox.InputGroup>
    {props.error && <Description className='text-danger'>{props.error}</Description>}
    <ComboBox.Popover>
      <ListBox renderEmptyState={() => <EmptyState>{t('manager.writes.noDestinations')}</EmptyState>}>{
        props.options.map((option) => <ListBox.Item
          key={option.reference}
          id={option.reference}
          textValue={`${option.reference} ${option.name}`}
        >
          {option.reference === option.name
            ? option.name
            : `${option.name} (${option.reference})`}
          <ListBox.ItemIndicator />
        </ListBox.Item>)
      }</ListBox>
    </ComboBox.Popover>
  </ComboBox>
}
