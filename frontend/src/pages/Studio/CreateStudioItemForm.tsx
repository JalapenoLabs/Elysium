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
import { selectAllStorageLocations } from '../../store/storageLocationsSlice'
import { studioItemUpserted } from '../../store/studioItemsSlice'

// User interface
import {
  Description,
  FieldError,
  Form,
  Input,
  Label,
  ListBox,
  Select,
  TextArea,
  TextField,
} from '@heroui/react'
import { PageCreateFooter } from '../../layout/page/PageCreateFooter'

// Utility
import { zodResolver } from '@hookform/resolvers/zod'

// Misc
import { getApiErrorMessage } from '../../api/errors'
import { createStudioItem } from '../../api/routes/studioRoutes'
import { useInitialFocus } from '../../hooks/useInitialFocus'
import { useProjectsLoader, useStorageLocationsLoader } from '../../hooks/useServerData'
import { saveForm, usePageSave } from '../../layout/page/pageSave'
import { getStudioItemUrl } from '../../urls'
import { createStudioItemFormSchema } from './studioItemFormSchema'
import { getDefaultStudioLocationId, getStudioLocationChoices } from './studioStorage'
import { toast } from '../../toast/toast'

// The project select's key for no project; the form holds an empty string.
const NO_PROJECT_KEY = 'none'

// The New item form: the first prompt, an optional title and project, and where the files
// are kept. Creating it starts the first turn and opens the item. It creates from the page's
// Save, in the header and below the form, which waits for the prompt and a location.
export function CreateStudioItemForm() {
  const { t } = useTranslation([ 'studio', 'common' ])
  const dispatch = useAppDispatch()
  const navigate = useNavigate()
  useProjectsLoader()
  useStorageLocationsLoader()
  const projects = useAppSelector(selectAllProjects)
  const locations = useAppSelector(selectAllStorageLocations)
  const promptRef = useInitialFocus()

  const resolver = useMemo(() => zodResolver(createStudioItemFormSchema(t)), [ t ])
  const form = useForm<StudioItemFormValues>({
    resolver,
    defaultValues: {
      prompt: '',
      title: '',
      projectId: '',
      storageLocationId: getDefaultStudioLocationId(getStudioLocationChoices(locations, null)),
    },
  })

  const [ prompt, title, projectId, storageLocationId ] = useWatch({
    control: form.control,
    name: [ 'prompt', 'title', 'projectId', 'storageLocationId' ],
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

  const save = saveForm(form, async (values) => {
    try {
      const { item, session } = await createStudioItem({
        prompt: values.prompt.trim(),
        title: values.title || undefined,
        projectId: values.projectId || undefined,
        storageLocationId: values.storageLocationId,
      })
      dispatch(studioItemUpserted(item))
      dispatch(codingSessionUpserted(session))
      toast.success(t('toasts.created', { title: item.title }))
      void navigate(getStudioItemUrl(item.id))
      return true
    }
    catch (error) {
      // A refused item (400), such as a location the project may not use, and an unavailable
      // workspace (502) both say what went wrong.
      const message = getApiErrorMessage(error)
      if (!message) {
        console.debug('CreateStudioItemForm failed to create an item', { error })
      }
      toast.danger(t('toasts.createFailed'), {
        description: message ?? t('common:errors.unexpected'),
      })
      return false
    }
  })

  function blockedReason() {
    if (!prompt.trim()) {
      return t('create.errors.promptRequired')
    }
    if (!storageLocationId) {
      return t('create.errors.locationRequired')
    }
    return null
  }

  const onSubmit = usePageSave({
    save,
    isDirty: form.formState.isDirty,
    blockedReason: blockedReason(),
    label: t('common:actions.create'),
  })

  const noLocationKey = projectId
    ? 'create.noLocationForProject'
    : 'create.noLocationForEveryProject'

  return <Form onSubmit={onSubmit} validationBehavior='aria' className='flex max-w-3xl flex-col gap-4'>
    {/* First prompt */}
    <TextField
      isRequired
      id='studio-item-prompt'
      isInvalid={Boolean(errors.prompt)}
      value={prompt}
      onChange={(value) => form.setValue('prompt', value, { shouldDirty: true, shouldValidate: true })}
    >
      <Label>{t('create.prompt')}</Label>
      <TextArea ref={promptRef} rows={5} />
      <Description>{t('create.promptHint')}</Description>
      <FieldError>{errors.prompt?.message}</FieldError>
    </TextField>

    {/* Title */}
    <TextField
      id='studio-item-title'
      autoComplete='off'
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
      id='studio-item-project'
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
      id='studio-item-storage-location'
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

    <PageCreateFooter />
  </Form>
}
