// Copyright © 2026 Jalapeno Labs

import type { UseOverlayStateReturn } from '@heroui/react'
import type { Project } from '../../api/routes/projectRoutes'
import type { ProjectFormValues } from './projectFormSchema'

// Core
import { useMemo } from 'react'
import { useForm, useWatch } from 'react-hook-form'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch } from '../../store/hooks'
import { projectUpserted } from '../../store/projectsSlice'

// User interface
import { Button, FieldError, Form, Input, Label, Modal, TextArea, TextField, toast } from '@heroui/react'

// Utility
import { zodResolver } from '@hookform/resolvers/zod'
import { HTTPError } from 'ky'

// Misc
import { createProject, updateProject } from '../../api/routes/projectRoutes'
import { createProjectFormSchema } from './projectFormSchema'

type Props = {
  state: UseOverlayStateReturn
  // The project being edited, or null to create one.
  project: Project | null
}

export function ProjectFormModal(props: Props) {
  const { t } = useTranslation([ 'projects', 'common' ])
  const dispatch = useAppDispatch()

  const resolver = useMemo(
    () => zodResolver(createProjectFormSchema(t)),
    [ t ],
  )

  const form = useForm<ProjectFormValues>({
    resolver,
    defaultValues: {
      name: props.project?.name ?? '',
      description: props.project?.description ?? '',
    },
  })

  const onSubmit = form.handleSubmit(async (values) => {
    try {
      if (props.project) {
        const response = await updateProject(props.project.id, values)
        dispatch(projectUpserted(response.project))
        toast.success(t('toasts.updated', { name: values.name }))
      }
      else {
        const response = await createProject(values)
        dispatch(projectUpserted(response.project))
        toast.success(t('toasts.created', { name: values.name }))
      }

      props.state.close()
    }
    catch (error) {
      if (error instanceof HTTPError && error.response.status === 409) {
        form.setError('name', { message: t('form.errors.nameTaken') })
        return
      }

      console.debug('ProjectFormModal failed to save the project', { error })
      toast.danger(t('common:errors.unexpected'))
    }
  })

  const errors = form.formState.errors
  // useWatch subscribes per field and, unlike form.watch, is safe for the React Compiler.
  const [ name, description ] = useWatch({
    control: form.control,
    name: [ 'name', 'description' ],
  })

  // Controlled overlays skip the Modal root: it is a trigger wrapper, and without a
  // pressable child React Aria warns. The backdrop takes the open state directly.
  return <Modal.Backdrop isOpen={props.state.isOpen} onOpenChange={props.state.setOpen}>
    <Modal.Container>
      <Modal.Dialog className='sm:max-w-lg'>
        <Modal.CloseTrigger />
        <Modal.Header>
          <Modal.Heading>{
            props.project
              ? t('form.editTitle', { name: props.project.name })
              : t('form.createTitle')
          }</Modal.Heading>
        </Modal.Header>
        <Form onSubmit={onSubmit} validationBehavior='aria'>
          <Modal.Body className='mt-2 flex flex-col gap-4'>
            {/* Name */}
            <TextField
              autoFocus
              isRequired
              isInvalid={Boolean(errors.name)}
              value={name}
              onChange={(value) => form.setValue('name', value, { shouldDirty: true, shouldValidate: true })}
            >
              <Label>{t('form.name')}</Label>
              <Input />
              <FieldError>{errors.name?.message}</FieldError>
            </TextField>

            {/* Description */}
            <TextField
              isInvalid={Boolean(errors.description)}
              value={description}
              onChange={(value) => form.setValue('description', value, { shouldDirty: true, shouldValidate: true })}
            >
              <Label>{t('form.description')}</Label>
              <TextArea rows={3} />
              <FieldError>{errors.description?.message}</FieldError>
            </TextField>
          </Modal.Body>
          <Modal.Footer>
            <Button slot='close' variant='tertiary'>
              <span>{t('common:actions.cancel')}</span>
            </Button>
            <Button
              type='submit'
              isPending={form.formState.isSubmitting}
            >
              <span>{t('common:actions.save')}</span>
            </Button>
          </Modal.Footer>
        </Form>
      </Modal.Dialog>
    </Modal.Container>
  </Modal.Backdrop>
}
