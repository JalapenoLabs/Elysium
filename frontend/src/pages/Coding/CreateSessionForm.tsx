// Copyright © 2026 Jalapeno Labs

import type { ActionItem } from '../../api/routes/actionItemRoutes'
import type { CreateSessionFormValues } from './createSessionFormSchema'

// Core
import { useMemo } from 'react'
import { useForm, useWatch } from 'react-hook-form'
import { useTranslation } from 'react-i18next'

// Redux
import { codingSessionUpserted } from '../../store/codingSessionsSlice'
import { selectAllGithubCredentials, selectDefaultGithubCredential } from '../../store/githubCredentialsSlice'
import { useAppDispatch, useAppSelector } from '../../store/hooks'
import { selectAllProjects } from '../../store/projectsSlice'
import { selectAllSatellites } from '../../store/satellitesSlice'

// User interface
import {
  Button,
  Description,
  FieldError,
  Form,
  Input,
  Label,
  Link,
  ListBox,
  Modal,
  Select,
  TextArea,
  TextField,
  toast,
} from '@heroui/react'
import { LuListTodo } from 'react-icons/lu'
import { GithubTokenSelect, INHERIT_GITHUB_TOKEN, NO_GITHUB_TOKEN } from '../../components/GithubTokenSelect'
import { SessionGithubTokenExpiry } from './SessionGithubTokenExpiry'
import { SessionRepositoriesField } from './SessionRepositoriesField'

// Utility
import { zodResolver } from '@hookform/resolvers/zod'

// Misc
import { getApiErrorMessage } from '../../api/errors'
import { createCodingSession } from '../../api/routes/codingSessionRoutes'
import { useGithubCredentialsLoader, useProjectsLoader, useSatellitesLoader } from '../../hooks/useServerData'
import { getActionItemViewUrl } from '../../urls'
import { resolveProjectGithubCredentialId } from '../Settings/Github/githubPresentation'
import { useCodingActions } from './codingActionsContext'
import { createSessionFormSchema, SESSION_TITLE_MAX_CHARACTERS } from './createSessionFormSchema'
import { getSessionProjectChoices } from './sessionItemProjects'

type Props = {
  // The item the session is started from, or null for a session of its own.
  actionItem: ActionItem | null
  onCreated: () => void
}

// What the token picker's fixed choices send as `githubCredentialId`: following the project
// leaves the field out, and no token sends null. Every other choice is a token id.
const githubCredentialIdByChoice: Record<string, string | null | undefined> = {
  [INHERIT_GITHUB_TOKEN]: undefined,
  [NO_GITHUB_TOKEN]: null,
}

// The New session form. A session started from an action item belongs to one of the item's
// projects (any project when it has none), is titled after it, and needs a prompt, which
// its first turn carries after the item's context.
export function CreateSessionForm(props: Props) {
  const { t } = useTranslation([ 'coding', 'common' ])
  const dispatch = useAppDispatch()
  const codingActions = useCodingActions()
  useProjectsLoader()
  useSatellitesLoader()
  useGithubCredentialsLoader()
  const projects = useAppSelector(selectAllProjects)
  const githubCredentials = useAppSelector(selectAllGithubCredentials)
  const workspaceDefault = useAppSelector(selectDefaultGithubCredential)
  const satellites = useAppSelector(selectAllSatellites)
  const actionItem = props.actionItem

  const activeSatellites = useMemo(
    () => satellites.filter((satellite) => satellite.isActive),
    [ satellites ],
  )
  const projectChoices = useMemo(
    () => getSessionProjectChoices(actionItem, projects),
    [ actionItem, projects ],
  )
  // An item in exactly one project decides the session's project.
  const isProjectFixed = actionItem?.projectIds.length === 1

  const resolver = useMemo(
    () => zodResolver(createSessionFormSchema(t, { isPromptRequired: Boolean(actionItem) })),
    [ t, actionItem ],
  )

  const form = useForm<CreateSessionFormValues>({
    resolver,
    defaultValues: {
      // A lone project or satellite is the only possible choice; preselect it.
      projectId: projectChoices.length === 1
        ? projectChoices[0].id
        : '',
      satelliteId: activeSatellites.length === 1
        ? activeSatellites[0].id
        : '',
      title: actionItem?.title.slice(0, SESSION_TITLE_MAX_CHARACTERS) ?? '',
      repositories: [],
      prompt: '',
      githubChoice: INHERIT_GITHUB_TOKEN,
    },
  })

  const onSubmit = form.handleSubmit(async (values) => {
    const prompt = values.prompt.trim()
    const promptFirstLine = prompt.split('\n', 1)[0].slice(0, SESSION_TITLE_MAX_CHARACTERS)
    const title = values.title || promptFirstLine || t('create.untitled')

    try {
      // The API queues the prompt as the thread's first turn, after the item's context
      // when the session is started from one.
      const { session } = await createCodingSession({
        projectId: values.projectId,
        satelliteId: values.satelliteId,
        title,
        repositories: values.repositories.map((repository) => ({
          url: repository.url,
          baseBranch: repository.baseBranch || undefined,
        })),
        // Following the project is spelled by leaving the field out.
        githubCredentialId: values.githubChoice in githubCredentialIdByChoice
          ? githubCredentialIdByChoice[values.githubChoice]
          : values.githubChoice,
        prompt: prompt || undefined,
        actionItemId: actionItem?.id,
      })
      dispatch(codingSessionUpserted(session))
      codingActions.openSession(session)
      props.onCreated()
      toast.success(t('toasts.created', { title }))
    }
    catch (error) {
      // A refused session (400), such as a repository GitHub or the API will not take, an
      // item deleted meanwhile (409), and a satellite that refused the thread (502) all say
      // what went wrong.
      const message = getApiErrorMessage(error)
      if (!message) {
        console.debug('CreateSessionForm failed to start a session', { error })
      }
      toast.danger(message ?? t('common:errors.unexpected'))
    }
  })

  const errors = form.formState.errors
  // useWatch subscribes per field and, unlike form.watch, is safe for the React Compiler.
  const [ projectId, satelliteId, title, repositories, prompt, githubChoice ] = useWatch({
    control: form.control,
    name: [ 'projectId', 'satelliteId', 'title', 'repositories', 'prompt', 'githubChoice' ],
  })

  // The token the project would hand this session, and the one the session will start with.
  const project = projects.find((candidate) => candidate.id === projectId)
  const projectCredentialId = project
    ? resolveProjectGithubCredentialId(project.github, workspaceDefault?.id ?? null)
    : workspaceDefault?.id ?? null
  const projectCredential = githubCredentials.find((credential) => credential.id === projectCredentialId)
  const sessionCredentialId = githubChoice === INHERIT_GITHUB_TOKEN
    ? projectCredentialId
    : githubChoice === NO_GITHUB_TOKEN
      ? null
      : githubChoice
  const sessionCredential = githubCredentials.find((credential) => credential.id === sessionCredentialId) ?? null
  const isMissingPrompt = Boolean(actionItem) && !prompt.trim()

  return <Form onSubmit={onSubmit} validationBehavior='aria'>
    <Modal.Body className='mt-2 flex flex-col gap-4'>
      {/* The item the session starts from */}
      {actionItem && <div className='flex items-start gap-3 rounded-xl bg-surface p-3'>
        <LuListTodo className='mt-0.5 size-4 shrink-0 opacity-70' aria-hidden />
        <div className='min-w-0 text-sm'>
          <div className='text-xs opacity-60'>{t('create.fromItem.label')}</div>
          <Link
            href={getActionItemViewUrl(actionItem.id)}
            className='font-medium text-link no-underline hover:underline'
          >{
            actionItem.title
          }</Link>
          <p className='mt-1 text-xs opacity-70'>{t('create.fromItem.hint')}</p>
        </div>
      </div>}

      {/* Project */}
      <Select
        isRequired
        isDisabled={isProjectFixed}
        isInvalid={Boolean(errors.projectId)}
        placeholder={t('create.projectPlaceholder')}
        value={projectId || null}
        onChange={(key) => form.setValue(
          'projectId',
          String(key ?? ''),
          { shouldDirty: true, shouldValidate: true },
        )}
      >
        <Label>{t('create.project')}</Label>
        <Select.Trigger>
          <Select.Value />
          <Select.Indicator />
        </Select.Trigger>
        <Select.Popover>
          <ListBox>{
            projectChoices.map((choice) => <ListBox.Item
              key={choice.id}
              id={choice.id}
              textValue={choice.name}
            >
              {choice.name}
              <ListBox.ItemIndicator />
            </ListBox.Item>)
          }</ListBox>
        </Select.Popover>
        {actionItem && actionItem.projectIds.length > 0 && <Description>{
          isProjectFixed
            ? t('create.fromItem.projectFixed')
            : t('create.fromItem.projectChoices')
        }</Description>}
        <FieldError>{errors.projectId?.message}</FieldError>
      </Select>

      {/* Satellite */}
      <Select
        isRequired
        isInvalid={Boolean(errors.satelliteId)}
        placeholder={t('create.satellitePlaceholder')}
        value={satelliteId || null}
        onChange={(key) => form.setValue(
          'satelliteId',
          String(key ?? ''),
          { shouldDirty: true, shouldValidate: true },
        )}
      >
        <Label>{t('create.satellite')}</Label>
        <Select.Trigger>
          <Select.Value />
          <Select.Indicator />
        </Select.Trigger>
        <Select.Popover>
          <ListBox>{
            activeSatellites.map((satellite) => <ListBox.Item
              key={satellite.id}
              id={satellite.id}
              textValue={satellite.name}
            >
              {satellite.name}
              <ListBox.ItemIndicator />
            </ListBox.Item>)
          }</ListBox>
        </Select.Popover>
        <FieldError>{errors.satelliteId?.message}</FieldError>
      </Select>

      {/* Title */}
      <TextField
        autoFocus={!actionItem}
        isInvalid={Boolean(errors.title)}
        value={title}
        onChange={(value) => form.setValue('title', value, { shouldDirty: true, shouldValidate: true })}
      >
        <Label>{t('create.sessionTitle')}</Label>
        <Input />
        <Description>{t('create.sessionTitleHint')}</Description>
        <FieldError>{errors.title?.message}</FieldError>
      </TextField>

      {/* GitHub token */}
      <div className='flex flex-col gap-2'>
        <GithubTokenSelect
          label={t('create.github.label')}
          description={t('create.github.hint')}
          inheritLabel={projectCredential
            ? t('create.github.inherit', { name: projectCredential.name })
            : t('create.github.inheritNone')}
          value={githubChoice}
          credentials={githubCredentials}
          onChange={(value) => form.setValue('githubChoice', value, { shouldDirty: true })}
        />
        <SessionGithubTokenExpiry credential={sessionCredential} />
      </div>

      {/* Repositories */}
      <SessionRepositoriesField
        credential={sessionCredential}
        value={repositories}
        errorMessage={errors.repositories?.message ?? errors.repositories?.root?.message}
        branchErrors={repositories.map((_repository, index) => errors.repositories?.[index]?.baseBranch?.message)}
        onChange={(value) => form.setValue('repositories', value, { shouldDirty: true, shouldValidate: true })}
      />

      {/* First prompt */}
      <TextField
        autoFocus={Boolean(actionItem)}
        isRequired={Boolean(actionItem)}
        isInvalid={Boolean(errors.prompt)}
        value={prompt}
        onChange={(value) => form.setValue('prompt', value, { shouldDirty: true })}
      >
        <Label>{t('create.prompt')}</Label>
        <TextArea rows={5} />
        <Description>{
          actionItem
            ? t('create.fromItem.promptHint')
            : t('create.promptHint')
        }</Description>
        <FieldError>{errors.prompt?.message}</FieldError>
      </TextField>
    </Modal.Body>
    <Modal.Footer>
      <Button slot='close' variant='tertiary'>
        <span>{t('common:actions.cancel')}</span>
      </Button>
      <Button
        type='submit'
        isDisabled={!projectId || !satelliteId || isMissingPrompt || Boolean(errors.repositories)}
        isPending={form.formState.isSubmitting}
      >
        <span>{t('common:actions.create')}</span>
      </Button>
    </Modal.Footer>
  </Form>
}
