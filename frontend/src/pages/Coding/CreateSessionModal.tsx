// Copyright © 2026 Jalapeno Labs

import type { UseOverlayStateReturn } from '@heroui/react'
import type { CreateSessionFormValues } from './createSessionFormSchema'

// Core
import { useMemo } from 'react'
import { useForm, useWatch } from 'react-hook-form'
import { useTranslation } from 'react-i18next'

// Redux
import { codingSessionUpserted } from '../../store/codingSessionsSlice'
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
  ListBox,
  Modal,
  Select,
  TextArea,
  TextField,
  toast,
} from '@heroui/react'

// Utility
import { zodResolver } from '@hookform/resolvers/zod'

// Misc
import { getUpstreamErrorMessage } from '../../api/errors'
import { createCodingSession, startTurn } from '../../api/routes/codingSessionRoutes'
import { useProjectsLoader, useSatellitesLoader } from '../../hooks/useServerData'
import { useCodingActions } from './codingActionsContext'
import { createSessionFormSchema, SESSION_TITLE_MAX_CHARACTERS } from './createSessionFormSchema'

type Props = {
  state: UseOverlayStateReturn
}

export function CreateSessionModal(props: Props) {
  const { t } = useTranslation([ 'coding', 'common' ])
  const dispatch = useAppDispatch()
  const codingActions = useCodingActions()
  useProjectsLoader()
  useSatellitesLoader()
  const projects = useAppSelector(selectAllProjects)
  const satellites = useAppSelector(selectAllSatellites)

  const activeSatellites = useMemo(
    () => satellites.filter((satellite) => satellite.isActive),
    [ satellites ],
  )

  const resolver = useMemo(
    () => zodResolver(createSessionFormSchema(t)),
    [ t ],
  )

  const form = useForm<CreateSessionFormValues>({
    resolver,
    defaultValues: {
      // A lone project or satellite is the only possible choice; preselect it.
      projectId: projects.length === 1
        ? projects[0].id
        : '',
      satelliteId: activeSatellites.length === 1
        ? activeSatellites[0].id
        : '',
      title: '',
      repositoryUrl: '',
      baseBranch: '',
      prompt: '',
    },
  })

  const onSubmit = form.handleSubmit(async (values) => {
    const prompt = values.prompt.trim()
    const promptFirstLine = prompt.split('\n', 1)[0].slice(0, SESSION_TITLE_MAX_CHARACTERS)
    const title = values.title || promptFirstLine || t('create.untitled')

    try {
      const { session } = await createCodingSession({
        projectId: values.projectId,
        satelliteId: values.satelliteId,
        title,
        repositoryUrl: values.repositoryUrl || undefined,
        baseBranch: values.baseBranch || undefined,
      })
      dispatch(codingSessionUpserted(session))
      codingActions.openSession(session)
      props.state.close()
      toast.success(t('toasts.created', { title }))

      if (prompt) {
        await startTurn(session.id, prompt)
      }
    }
    catch (error) {
      const message = getUpstreamErrorMessage(error)
      if (!message) {
        console.debug('CreateSessionModal failed to start a session', { error })
      }
      toast.danger(message ?? t('common:errors.unexpected'))
    }
  })

  const errors = form.formState.errors
  // useWatch subscribes per field and, unlike form.watch, is safe for the React Compiler.
  const [ projectId, satelliteId, title, repositoryUrl, baseBranch, prompt ] = useWatch({
    control: form.control,
    name: [ 'projectId', 'satelliteId', 'title', 'repositoryUrl', 'baseBranch', 'prompt' ],
  })

  // Controlled overlays skip the Modal root: it is a trigger wrapper, and without a
  // pressable child React Aria warns. The backdrop takes the open state directly.
  return <Modal.Backdrop isOpen={props.state.isOpen} onOpenChange={props.state.setOpen}>
    <Modal.Container>
      <Modal.Dialog className='sm:max-w-xl'>
        <Modal.CloseTrigger />
        <Modal.Header>
          <Modal.Heading>{
            t('create.title')
          }</Modal.Heading>
        </Modal.Header>
        <Form onSubmit={onSubmit} validationBehavior='aria'>
          <Modal.Body className='mt-2 flex flex-col gap-4'>
            {/* Project */}
            <Select
              isRequired
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
                  projects.map((project) => <ListBox.Item
                    key={project.id}
                    id={project.id}
                    textValue={project.name}
                  >
                    {project.name}
                    <ListBox.ItemIndicator />
                  </ListBox.Item>)
                }</ListBox>
              </Select.Popover>
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
              autoFocus
              isInvalid={Boolean(errors.title)}
              value={title}
              onChange={(value) => form.setValue('title', value, { shouldDirty: true, shouldValidate: true })}
            >
              <Label>{t('create.sessionTitle')}</Label>
              <Input />
              <Description>{t('create.sessionTitleHint')}</Description>
              <FieldError>{errors.title?.message}</FieldError>
            </TextField>

            <div className='grid grid-cols-1 gap-4 sm:grid-cols-[2fr_1fr]'>
              {/* Repository */}
              <TextField
                isInvalid={Boolean(errors.repositoryUrl)}
                value={repositoryUrl}
                onChange={(value) => form.setValue('repositoryUrl', value, { shouldDirty: true, shouldValidate: true })}
              >
                <Label>{t('create.repositoryUrl')}</Label>
                <Input placeholder='https://github.com/org/repo.git' />
                <Description>{t('create.repositoryUrlHint')}</Description>
                <FieldError>{errors.repositoryUrl?.message}</FieldError>
              </TextField>

              {/* Base branch */}
              <TextField
                isDisabled={!repositoryUrl}
                isInvalid={Boolean(errors.baseBranch)}
                value={baseBranch}
                onChange={(value) => form.setValue('baseBranch', value, { shouldDirty: true, shouldValidate: true })}
              >
                <Label>{t('create.baseBranch')}</Label>
                <Input placeholder='main' />
                <FieldError>{errors.baseBranch?.message}</FieldError>
              </TextField>
            </div>

            {/* First prompt */}
            <TextField
              value={prompt}
              onChange={(value) => form.setValue('prompt', value, { shouldDirty: true })}
            >
              <Label>{t('create.prompt')}</Label>
              <TextArea rows={5} />
              <Description>{t('create.promptHint')}</Description>
            </TextField>
          </Modal.Body>
          <Modal.Footer>
            <Button slot='close' variant='tertiary'>
              <span>{t('common:actions.cancel')}</span>
            </Button>
            <Button
              type='submit'
              isDisabled={!projectId || !satelliteId}
              isPending={form.formState.isSubmitting}
            >
              <span>{t('common:actions.create')}</span>
            </Button>
          </Modal.Footer>
        </Form>
      </Modal.Dialog>
    </Modal.Container>
  </Modal.Backdrop>
}
