// Copyright © 2026 Jalapeno Labs

import type { WorkspaceSettings } from '../../../api/routes/userRoutes'
import type { WorkerPool } from '../../../api/routes/workQueueRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch } from '../../../store/hooks'
import { workspaceSettingsUpdated } from '../../../store/workspaceSettingsSlice'

// User interface
import { Description, FieldError, Form, Label, NumberField, Switch } from '@heroui/react'

// Misc
import { getApiErrorMessage } from '../../../api/errors'
import { updateWorkspaceSettings } from '../../../api/routes/userRoutes'
import { useInitialFocus } from '../../../hooks/useInitialFocus'
import { usePageSave } from '../../../layout/page/pageSave'
import { toast } from '../../../toast/toast'

type Props = {
  settings: WorkspaceSettings
  pool: WorkerPool
}

// The pool's size: the default, one worker per CPU core the engine sees minus two, or a number an
// admin chooses, at least one and with no maximum. Saved by the page's Save, in the header.
export function WorkerPoolForm(props: Props) {
  const { t } = useTranslation([ 'workQueue', 'common' ])
  const dispatch = useAppDispatch()
  const saved = props.settings.workerPoolSize
  const [ usesDefault, setUsesDefault ] = useState(saved === null)
  // The number shown while choosing starts from the size in force, so switching the default off
  // keeps the pool as it is until the admin changes it.
  const [ size, setSize ] = useState(saved ?? props.pool.defaultSize ?? 1)
  // The number is the field to type in whenever it is in force, so it takes focus as the page
  // opens on a chosen size, and as the default is switched off.
  const sizeRef = useInitialFocus(!usesDefault)
  const isValid = usesDefault || (Number.isInteger(size) && size >= 1)
  const chosen = usesDefault
    ? null
    : size

  async function save() {
    if (!isValid) {
      console.debug('WorkerPoolForm refused to save a size below one worker', { size })
      return false
    }
    try {
      const response = await updateWorkspaceSettings({ workerPoolSize: chosen })
      dispatch(workspaceSettingsUpdated(response.settings))
      toast.success(t('poolSettings.saved'))
      return true
    }
    catch (error) {
      const message = getApiErrorMessage(error)
      if (!message) {
        console.debug('WorkerPoolForm could not save the pool size', { error })
      }
      toast.danger(message ?? t('common:errors.unexpected'))
      return false
    }
  }

  const onSubmit = usePageSave({
    save,
    isDirty: chosen !== saved,
    blockedReason: isValid
      ? null
      : t('poolSettings.sizeHint'),
  })

  const defaultDescription = props.pool.defaultSize === null || props.pool.cpuCount === null
    ? t('poolSettings.defaultUnknown')
    : t('poolSettings.defaultSize', { count: props.pool.defaultSize, cores: props.pool.cpuCount })

  return <Form className='max-w-xl' onSubmit={onSubmit}>
    <div className='relaxed'>
      <Switch id='worker-pool-default' isSelected={usesDefault} onChange={setUsesDefault}>
        <Switch.Content>
          <Switch.Control>
            <Switch.Thumb />
          </Switch.Control>
          <Label>{t('poolSettings.useDefault')}</Label>
        </Switch.Content>
        <Description>{defaultDescription}</Description>
      </Switch>
    </div>
    <NumberField
      id='worker-pool-size'
      isRequired={!usesDefault}
      isDisabled={usesDefault}
      isInvalid={!isValid}
      minValue={1}
      step={1}
      formatOptions={{ maximumFractionDigits: 0 }}
      value={size}
      onChange={setSize}
    >
      <Label>{t('poolSettings.size')}</Label>
      <NumberField.Group>
        <NumberField.DecrementButton />
        <NumberField.Input ref={sizeRef} />
        <NumberField.IncrementButton />
      </NumberField.Group>
      <FieldError>{t('poolSettings.sizeHint')}</FieldError>
    </NumberField>
  </Form>
}
