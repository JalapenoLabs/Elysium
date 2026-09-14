// Copyright © 2026 Jalapeno Labs

import type { UseOverlayStateReturn } from '@heroui/react'
import type { Llm } from '../../../api/routes/llmRoutes'
import type { LlmFormValues } from './llmFormSchema'

// Core
import { useMemo } from 'react'
import { useForm, useWatch } from 'react-hook-form'
import { useTranslation } from 'react-i18next'
import { mutate } from 'swr'

// User interface
import {
  Button,
  DateField,
  Description,
  FieldError,
  Form,
  Input,
  Label,
  ListBox,
  Modal,
  NumberField,
  Select,
  Switch,
  TextArea,
  TextField,
  toast,
} from '@heroui/react'

// Utility
import { zodResolver } from '@hookform/resolvers/zod'
import { parseAbsoluteToLocal } from '@internationalized/date'
import { HTTPError } from 'ky'

// Misc
import { createLlm, updateLlm } from '../../../api/routes/llmRoutes'
import { LLMS_CACHE_KEY } from '../../../hooks/useLlms'
import { createLlmFormSchema } from './llmFormSchema'
import { LLM_TYPES, llmTypeLabelKeys } from './llmPresentation'

type Props = {
  state: UseOverlayStateReturn
  // The credential being edited, or null to create a new one.
  llm: Llm | null
}

export function LlmFormModal(props: Props) {
  const { t } = useTranslation([ 'llms', 'common' ])
  const mode = props.llm
    ? 'edit'
    : 'create'

  const resolver = useMemo(
    () => zodResolver(createLlmFormSchema(t, mode)),
    [ t, mode ],
  )

  const form = useForm<LlmFormValues>({
    resolver,
    defaultValues: {
      name: props.llm?.name ?? '',
      description: props.llm?.description ?? '',
      type: props.llm?.type ?? LLM_TYPES[0],
      secretToken: '',
      priority: props.llm?.priority ?? 0,
      isActive: props.llm?.isActive ?? true,
      expiresAt: props.llm?.expiresAt
        ? parseAbsoluteToLocal(props.llm.expiresAt)
        : null,
    },
  })

  const onSubmit = form.handleSubmit(async (values) => {
    // The server stores UTC; toAbsoluteString() emits the instant with a Z offset.
    const payload = {
      name: values.name,
      description: values.description,
      type: values.type,
      priority: values.priority,
      isActive: values.isActive,
      expiresAt: values.expiresAt?.toAbsoluteString() ?? null,
    }

    try {
      if (props.llm) {
        // A blank token means "keep the stored one", so it is only sent when typed.
        const secretToken = values.secretToken.trim()
          ? values.secretToken
          : undefined
        await updateLlm(props.llm.id, { ...payload, secretToken })
        toast.success(t('toasts.updated', { name: values.name }))
      }
      else {
        await createLlm({ ...payload, secretToken: values.secretToken })
        toast.success(t('toasts.created', { name: values.name }))
      }

      await mutate(LLMS_CACHE_KEY)
      props.state.close()
    }
    catch (error) {
      if (error instanceof HTTPError && error.response.status === 409) {
        form.setError('name', { message: t('form.errors.nameTaken') })
        return
      }

      console.debug('LlmFormModal failed to save the LLM', { error })
      toast.danger(t('common:errors.unexpected'))
    }
  })

  const errors = form.formState.errors
  // useWatch subscribes per field and, unlike form.watch, is safe for the React Compiler.
  const [ name, description, type, secretToken, priority, isActive, expiresAt ] = useWatch({
    control: form.control,
    name: [ 'name', 'description', 'type', 'secretToken', 'priority', 'isActive', 'expiresAt' ],
  })

  // Controlled overlays skip the Modal root: it is a trigger wrapper, and without a
  // pressable child React Aria warns. The backdrop takes the open state directly.
  return <Modal.Backdrop isOpen={props.state.isOpen} onOpenChange={props.state.setOpen}>
    <Modal.Container>
      <Modal.Dialog className='sm:max-w-lg'>
        <Modal.CloseTrigger />
        <Modal.Header>
          <Modal.Heading>{
            props.llm
              ? t('form.editTitle', { name: props.llm.name })
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
              <TextArea rows={2} />
              <FieldError>{errors.description?.message}</FieldError>
            </TextField>

            {/* Provider type */}
            <Select
              isRequired
              isInvalid={Boolean(errors.type)}
              placeholder={t('form.typePlaceholder')}
              value={type}
              onChange={(key) => {
                const selected = LLM_TYPES.find((type) => type === key)
                if (!selected) {
                  console.debug('LlmFormModal ignored an unknown provider type', { key })
                  return
                }
                form.setValue('type', selected, { shouldDirty: true, shouldValidate: true })
              }}
            >
              <Label>{t('form.type')}</Label>
              <Select.Trigger>
                <Select.Value />
                <Select.Indicator />
              </Select.Trigger>
              <Select.Popover>
                <ListBox>{
                  LLM_TYPES.map((type) => <ListBox.Item
                    key={type}
                    id={type}
                    textValue={t(llmTypeLabelKeys[type])}
                  >
                    {t(llmTypeLabelKeys[type])}
                    <ListBox.ItemIndicator />
                  </ListBox.Item>)
                }</ListBox>
              </Select.Popover>
              <FieldError>{errors.type?.message}</FieldError>
            </Select>

            {/* Secret token */}
            <TextField
              isRequired={mode === 'create'}
              isInvalid={Boolean(errors.secretToken)}
              type='password'
              autoComplete='off'
              value={secretToken}
              onChange={(value) => form.setValue('secretToken', value, { shouldDirty: true, shouldValidate: true })}
            >
              <Label>{t('form.secretToken')}</Label>
              <Input />
              {mode === 'edit' && <Description>{t('form.secretTokenKeep')}</Description>}
              <FieldError>{errors.secretToken?.message}</FieldError>
            </TextField>

            <div className='grid grid-cols-1 gap-4 sm:grid-cols-2'>
              {/* Priority */}
              <NumberField
                isRequired
                step={1}
                value={priority}
                onChange={(value) => form.setValue(
                  'priority',
                  Number.isNaN(value)
                    ? 0
                    : value,
                  { shouldDirty: true, shouldValidate: true },
                )}
              >
                <Label>{t('form.priority')}</Label>
                <NumberField.Group>
                  <NumberField.DecrementButton />
                  <NumberField.Input />
                  <NumberField.IncrementButton />
                </NumberField.Group>
                <Description>{t('form.priorityHint')}</Description>
              </NumberField>

              {/* Active */}
              <Switch
                className='self-center'
                isSelected={isActive}
                onChange={(isSelected) => form.setValue('isActive', isSelected, { shouldDirty: true })}
              >
                <Switch.Control>
                  <Switch.Thumb />
                </Switch.Control>
                <Switch.Content>
                  <Label>{t('form.isActive')}</Label>
                </Switch.Content>
              </Switch>
            </div>

            {/* Expiry, entered and shown in the viewer's local time zone */}
            <div>
              <DateField
                granularity='minute'
                hideTimeZone
                value={expiresAt}
                onChange={(value) => form.setValue('expiresAt', value, { shouldDirty: true })}
              >
                <Label>{t('form.expiresAt')}</Label>
                <DateField.Group>
                  <DateField.Input>{
                    (segment) => <DateField.Segment segment={segment} />
                  }</DateField.Input>
                </DateField.Group>
                <Description>{t('form.expiresAtHint')}</Description>
              </DateField>
              {expiresAt && <Button
                size='sm'
                variant='ghost'
                className='mt-1'
                onPress={() => form.setValue('expiresAt', null, { shouldDirty: true })}
              >
                <span>{t('form.clearExpiry')}</span>
              </Button>}
            </div>
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
