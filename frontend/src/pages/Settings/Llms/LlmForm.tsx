// Copyright © 2026 Jalapeno Labs

import type { Llm, LlmType } from '../../../api/routes/llmRoutes'
import type { LlmFormValues } from './llmFormSchema'

// Core
import { useMemo, useState } from 'react'
import { useForm, useWatch } from 'react-hook-form'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch } from '../../../store/hooks'
import { llmUpserted } from '../../../store/llmsSlice'

// User interface
import {
  Button,
  Calendar,
  DateField,
  DatePicker,
  Description,
  FieldError,
  Form,
  Input,
  InputGroup,
  Label,
  NumberField,
  Switch,
  TextArea,
  TextField,
  toast,
} from '@heroui/react'
import { LuEye, LuEyeOff } from 'react-icons/lu'

// Utility
import { zodResolver } from '@hookform/resolvers/zod'
import { fromDate, getLocalTimeZone, parseAbsoluteToLocal } from '@internationalized/date'
import { HTTPError } from 'ky'

// Misc
import { createLlm, updateLlm } from '../../../api/routes/llmRoutes'
import { createLlmFormSchema } from './llmFormSchema'

// 365 days to the millisecond, not "the same date next year", so the expiry matches a
// token that is valid for exactly that long regardless of leap years or clock changes.
const ONE_YEAR_MS = 365 * 24 * 60 * 60 * 1000

type Props = {
  type: LlmType
  // The credential being edited, or null to create one.
  llm: Llm | null
  onSaved: (llm: Llm) => void
  onCancel: () => void
}

export function LlmForm(props: Props) {
  const { t } = useTranslation([ 'llms', 'common' ])
  const dispatch = useAppDispatch()
  const [ isTokenRevealed, setIsTokenRevealed ] = useState(false)
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
      type: props.type,
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
        const response = await updateLlm(props.llm.id, { ...payload, secretToken })
        dispatch(llmUpserted(response.llm))
        toast.success(t('toasts.updated', { name: values.name }))
        props.onSaved(response.llm)
        return
      }

      const response = await createLlm({ ...payload, secretToken: values.secretToken })
      dispatch(llmUpserted(response.llm))
      toast.success(t('toasts.created', { name: values.name }))
      props.onSaved(response.llm)
    }
    catch (error) {
      if (error instanceof HTTPError && error.response.status === 409) {
        form.setError('name', { message: t('form.errors.nameTaken') })
        return
      }

      console.debug('LlmForm failed to save the LLM', { error })
      toast.danger(t('common:errors.unexpected'))
    }
  })

  const errors = form.formState.errors
  // useWatch subscribes per field and, unlike form.watch, is safe for the React Compiler.
  const [ name, description, secretToken, priority, isActive, expiresAt ] = useWatch({
    control: form.control,
    name: [ 'name', 'description', 'secretToken', 'priority', 'isActive', 'expiresAt' ],
  })

  return <Form onSubmit={onSubmit} validationBehavior='aria' className='flex flex-col gap-5'>
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

    {/* Secret token */}
    <TextField
      isRequired={mode === 'create'}
      isInvalid={Boolean(errors.secretToken)}
      type={isTokenRevealed
        ? 'text'
        : 'password'}
      autoComplete='off'
      value={secretToken}
      onChange={(value) => form.setValue('secretToken', value, { shouldDirty: true, shouldValidate: true })}
    >
      <Label>{t('form.secretToken')}</Label>
      <InputGroup>
        <InputGroup.Input
          className='font-mono'
          spellCheck={false}
          placeholder={t('form.secretTokenPlaceholder')}
        />
        <InputGroup.Suffix>
          <Button
            isIconOnly
            size='sm'
            variant='ghost'
            aria-label={isTokenRevealed
              ? t('form.hideToken')
              : t('form.revealToken')}
            aria-pressed={isTokenRevealed}
            onPress={() => setIsTokenRevealed((revealed) => !revealed)}
          >
            {isTokenRevealed
              ? <LuEyeOff className='size-4' aria-hidden />
              : <LuEye className='size-4' aria-hidden />}
          </Button>
        </InputGroup.Suffix>
      </InputGroup>
      {mode === 'edit' && <Description>{t('form.secretTokenKeep')}</Description>}
      <FieldError>{errors.secretToken?.message}</FieldError>
    </TextField>

    {/* Expiry, entered and shown in the viewer's local time zone */}
    <div>
      <DatePicker
        granularity='minute'
        hideTimeZone
        value={expiresAt}
        onChange={(value) => form.setValue('expiresAt', value, { shouldDirty: true })}
      >
        <Label>{t('form.expiresAt')}</Label>
        <DateField.Group fullWidth>
          <DateField.Input>{
            (segment) => <DateField.Segment segment={segment} />
          }</DateField.Input>
          <DateField.Suffix>
            <DatePicker.Trigger>
              <DatePicker.TriggerIndicator />
            </DatePicker.Trigger>
          </DateField.Suffix>
        </DateField.Group>
        <Description>{t('form.expiresAtHint')}</Description>
        <DatePicker.Popover>
          <Calendar aria-label={t('form.expiresAtPicker')}>
            <Calendar.Header>
              <Calendar.YearPickerTrigger>
                <Calendar.YearPickerTriggerHeading />
                <Calendar.YearPickerTriggerIndicator />
              </Calendar.YearPickerTrigger>
              <Calendar.NavButton slot='previous' />
              <Calendar.NavButton slot='next' />
            </Calendar.Header>
            <Calendar.Grid>
              <Calendar.GridHeader>{
                (day) => <Calendar.HeaderCell>{day}</Calendar.HeaderCell>
              }</Calendar.GridHeader>
              <Calendar.GridBody>{
                (date) => <Calendar.Cell date={date} />
              }</Calendar.GridBody>
            </Calendar.Grid>
            <Calendar.YearPickerGrid>
              <Calendar.YearPickerGridBody>{
                ({ year }) => <Calendar.YearPickerCell year={year} />
              }</Calendar.YearPickerGridBody>
            </Calendar.YearPickerGrid>
          </Calendar>
        </DatePicker.Popover>
      </DatePicker>
      <div className='mt-2 flex flex-wrap gap-2'>
        <Button
          size='sm'
          variant='outline'
          onPress={() => form.setValue(
            'expiresAt',
            fromDate(new Date(Date.now() + ONE_YEAR_MS), getLocalTimeZone()),
            { shouldDirty: true },
          )}
        >
          <span>{t('form.oneYearFromNow')}</span>
        </Button>
        {expiresAt && <Button
          size='sm'
          variant='ghost'
          onPress={() => form.setValue('expiresAt', null, { shouldDirty: true })}
        >
          <span>{t('form.clearExpiry')}</span>
        </Button>}
      </div>
    </div>

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

    <div className='flex justify-end gap-2'>
      <Button variant='tertiary' onPress={props.onCancel}>
        <span>{t('common:actions.cancel')}</span>
      </Button>
      <Button
        type='submit'
        isPending={form.formState.isSubmitting}
      >
        <span>{t('common:actions.save')}</span>
      </Button>
    </div>
  </Form>
}
