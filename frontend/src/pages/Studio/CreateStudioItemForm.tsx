// Copyright © 2026 Jalapeno Labs

import type { Key } from '@heroui/react'
import type { StudioItemFormValues } from './studioItemFormSchema'

// Core
import { useMemo } from 'react'
import { useForm, useWatch } from 'react-hook-form'
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'

// Redux
import { codingSessionUpserted } from '../../store/codingSessionsSlice'
import { useAppDispatch, useAppSelector } from '../../store/hooks'
import { selectAllProjects } from '../../store/projectsSlice'
import { selectAllSatellites } from '../../store/satellitesSlice'
import { selectAllStorageLocations } from '../../store/storageLocationsSlice'
import { studioItemUpserted } from '../../store/studioItemsSlice'

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
import { getApiErrorMessage } from '../../api/errors'
import { createStudioItem } from '../../api/routes/studioRoutes'
import { useProjectsLoader, useSatellitesLoader, useStorageLocationsLoader } from '../../hooks/useServerData'
import { getStudioItemUrl } from '../../urls'
import { createStudioItemFormSchema } from './studioItemFormSchema'
import { getDefaultStudioLocationId, getStudioLocationChoices } from './studioStorage'

// The project select's key for no project; the form holds an empty string.
const NO_PROJECT_KEY = 'none'

type Props = {
  onCreated: () => void
}

// The New item form: the first prompt, an optional title and project, where the files are
// kept, and the satellite the agent runs on. Creating it starts the first turn.
export function CreateStudioItemForm(props: Props) {
  const { t } = useTranslation([ 'studio', 'common' ])
  const dispatch = useAppDispatch()
  const navigate = useNavigate()
  useProjectsLoader()
  useSatellitesLoader()
  useStorageLocationsLoader()
  const projects = useAppSelector(selectAllProjects)
  const satellites = useAppSelector(selectAllSatellites)
  const locations = useAppSelector(selectAllStorageLocations)

  const activeSatellites = useMemo(
    () => satellites.filter((satellite) => satellite.isActive),
    [ satellites ],
  )

  const resolver = useMemo(() => zodResolver(createStudioItemFormSchema(t)), [ t ])
  const form = useForm<StudioItemFormValues>({
    resolver,
    defaultValues: {
      prompt: '',
      title: '',
      projectId: '',
      storageLocationId: getDefaultStudioLocationId(getStudioLocationChoices(locations, null)),
      // A lone satellite is the only possible choice; preselect it.
      satelliteId: activeSatellites.length === 1
        ? activeSatellites[0].id
        : '',
    },
  })

  const [ prompt, title, projectId, storageLocationId, satelliteId ] = useWatch({
    control: form.control,
    name: [ 'prompt', 'title', 'projectId', 'storageLocationId', 'satelliteId' ],
  })
  const errors = form.formState.errors
  const locationChoices = getStudioLocationChoices(locations, projectId || null)

  // The project bounds the locations, so a location the new project cannot use gives way to
  // the default among those it can.
  function changeProject(key: Key | null) {
    const nextProjectId = key === null || key === NO_PROJECT_KEY
      ? ''
      : String(key)
    form.setValue('projectId', nextProjectId, { shouldDirty: true })

    const nextChoices = getStudioLocationChoices(locations, nextProjectId || null)
    const isStillAllowed = nextChoices.some((location) => location.id === storageLocationId)
    if (!isStillAllowed) {
      form.setValue('storageLocationId', getDefaultStudioLocationId(nextChoices), { shouldDirty: true })
    }
  }

  const onSubmit = form.handleSubmit(async (values) => {
    try {
      const { item, session } = await createStudioItem({
        prompt: values.prompt.trim(),
        title: values.title || undefined,
        projectId: values.projectId || undefined,
        storageLocationId: values.storageLocationId,
        satelliteId: values.satelliteId,
      })
      dispatch(studioItemUpserted(item))
      dispatch(codingSessionUpserted(session))
      props.onCreated()
      toast.success(t('toasts.created', { title: item.title }))
      navigate(getStudioItemUrl(item.id))
    }
    catch (error) {
      // A refused item (400), such as a location the project may not use, and a satellite
      // that refused the thread (502) both say what went wrong.
      const message = getApiErrorMessage(error)
      if (!message) {
        console.debug('CreateStudioItemForm failed to create an item', { error })
      }
      toast.danger(t('toasts.createFailed'), {
        description: message ?? t('common:errors.unexpected'),
      })
    }
  })

  const noLocationKey = projectId
    ? 'create.noLocationForProject'
    : 'create.noLocationForEveryProject'

  return <Form onSubmit={onSubmit} validationBehavior='aria'>
    <Modal.Body className='mt-2 flex flex-col gap-4'>
      {/* First prompt */}
      <TextField
        autoFocus
        isRequired
        isInvalid={Boolean(errors.prompt)}
        value={prompt}
        onChange={(value) => form.setValue('prompt', value, { shouldDirty: true, shouldValidate: true })}
      >
        <Label>{t('create.prompt')}</Label>
        <TextArea rows={5} />
        <Description>{t('create.promptHint')}</Description>
        <FieldError>{errors.prompt?.message}</FieldError>
      </TextField>

      {/* Title */}
      <TextField
        isInvalid={Boolean(errors.title)}
        value={title}
        onChange={(value) => form.setValue('title', value, { shouldDirty: true, shouldValidate: true })}
      >
        <Label>{t('create.itemTitle')}</Label>
        <Input />
        <Description>{t('create.itemTitleHint')}</Description>
        <FieldError>{errors.title?.message}</FieldError>
      </TextField>

      {/* Project */}
      <Select
        value={projectId || NO_PROJECT_KEY}
        onChange={changeProject}
      >
        <Label>{t('create.project')}</Label>
        <Select.Trigger>
          <Select.Value />
          <Select.Indicator />
        </Select.Trigger>
        <Select.Popover>
          <ListBox>
            <ListBox.Item id={NO_PROJECT_KEY} textValue={t('create.noProject')}>
              {t('create.noProject')}
              <ListBox.ItemIndicator />
            </ListBox.Item>
            {projects.map((project) => <ListBox.Item
              key={project.id}
              id={project.id}
              textValue={project.name}
            >
              {project.name}
              <ListBox.ItemIndicator />
            </ListBox.Item>)}
          </ListBox>
        </Select.Popover>
        <Description>{t('create.projectHint')}</Description>
      </Select>

      {/* Storage location */}
      <Select
        isRequired
        isDisabled={!locationChoices.length}
        isInvalid={Boolean(errors.storageLocationId)}
        placeholder={t('create.locationPlaceholder')}
        value={storageLocationId || null}
        onChange={(key) => form.setValue(
          'storageLocationId',
          String(key ?? ''),
          { shouldDirty: true, shouldValidate: true },
        )}
      >
        <Label>{t('create.location')}</Label>
        <Select.Trigger>
          <Select.Value />
          <Select.Indicator />
        </Select.Trigger>
        <Select.Popover>
          <ListBox>{
            locationChoices.map((location) => <ListBox.Item
              key={location.id}
              id={location.id}
              textValue={location.name}
            >
              {location.name}
              <ListBox.ItemIndicator />
            </ListBox.Item>)
          }</ListBox>
        </Select.Popover>
        <Description>{
          locationChoices.length
            ? t('create.locationHint')
            : t(noLocationKey)
        }</Description>
        <FieldError>{errors.storageLocationId?.message}</FieldError>
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
    </Modal.Body>
    <Modal.Footer>
      <Button slot='close' variant='tertiary'>
        <span>{t('common:actions.cancel')}</span>
      </Button>
      <Button
        type='submit'
        isDisabled={!prompt.trim() || !storageLocationId || !satelliteId}
        isPending={form.formState.isSubmitting}
      >
        <span>{t('common:actions.create')}</span>
      </Button>
    </Modal.Footer>
  </Form>
}
