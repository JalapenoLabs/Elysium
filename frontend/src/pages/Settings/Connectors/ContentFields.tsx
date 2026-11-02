// Copyright © 2026 Jalapeno Labs

import type {
  AccountChoice,
  ConnectorKind,
  ContentField,
  SelectionOptions,
} from '../../../api/routes/connectorRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../../store/hooks'
import { selectAllStorageLocations } from '../../../store/storageLocationsSlice'

// User interface
import { Button, Checkbox, Label, ListBox, Select, Switch, toast } from '@heroui/react'

// Misc
import { getApiErrorMessage } from '../../../api/errors'
import { listConnectorOptions } from '../../../api/routes/connectorRoutes'
import { useStorageLocationsLoader } from '../../../hooks/useServerData'

// Everything a selection picks, as the API spells it.
const EVERYTHING = '*'

type Props = {
  kind: ConnectorKind
  fields: ContentField[]
  // The account the options are read through.
  account: AccountChoice | null
  value: Record<string, unknown>
  onChange: (value: Record<string, unknown>) => void
}

// What a connector reads, drawn from its kind's content fields. A selection picks from what
// the account reaches now, read from the provider on request; a storage location is one of
// the workspace's.
export function ContentFields(props: Props) {
  const { t } = useTranslation([ 'connectors', 'common' ])
  useStorageLocationsLoader()
  const storageLocations = useAppSelector(selectAllStorageLocations)
  const [ options, setOptions ] = useState<Record<string, SelectionOptions>>({})
  const [ isLoading, setIsLoading ] = useState(false)

  async function loadOptions() {
    if (!props.account) {
      console.debug('ContentFields cannot load options without an account')
      return
    }
    setIsLoading(true)
    try {
      const response = await listConnectorOptions(props.kind, props.account)
      setOptions(response.fields)
    }
    catch (error) {
      console.debug('ContentFields failed to load options', { error })
      toast.danger(getApiErrorMessage(error) ?? t('common:errors.unexpected'))
    }
    finally {
      setIsLoading(false)
    }
  }

  function set(fieldId: string, fieldValue: unknown) {
    props.onChange({ ...props.value, [fieldId]: fieldValue })
  }

  return <div className='flex flex-col gap-4'>{
    props.fields.map((field) => {
      const label = t(`fields.${field.id}`, { defaultValue: field.id })
      if (field.field.type === 'storage_location') {
        if (!storageLocations.length) {
          return <p key={field.id} className='text-sm opacity-70'>{t('fields.noStorageLocations')}</p>
        }
        return <Select
          key={field.id}
          isRequired={field.required}
          value={typeof props.value[field.id] === 'string' ? props.value[field.id] as string : null}
          onChange={(key) => set(field.id, String(key))}
        >
          <Label>{label}</Label>
          <Select.Trigger>
            <Select.Value />
            <Select.Indicator />
          </Select.Trigger>
          <Select.Popover>
            <ListBox>{
              storageLocations.map((location) => <ListBox.Item
                key={location.id}
                id={location.id}
                textValue={location.name}
              >
                {location.name}
                <ListBox.ItemIndicator />
              </ListBox.Item>)
            }</ListBox>
          </Select.Popover>
        </Select>
      }

      const current = props.value[field.id]
      const picked = Array.isArray(current) ? current.map(String) : []
      const fieldOptions = options[field.id]
      return <div key={field.id}>
        <p className='compact text-sm font-medium'>{label}</p>
        <Switch
          className='compact'
          isSelected={current === EVERYTHING}
          onChange={(isSelected) => set(field.id, isSelected ? EVERYTHING : [])}
        >
          <Switch.Content>
            <Switch.Control>
              <Switch.Thumb />
            </Switch.Control>
            <Label>{t('fields.everything')}</Label>
          </Switch.Content>
        </Switch>
        {current !== EVERYTHING && !fieldOptions && <Button
          size='sm'
          variant='outline'
          isDisabled={!props.account}
          isPending={isLoading}
          onPress={() => void loadOptions()}
        >
          <span>{t('fields.loadOptions')}</span>
        </Button>}
        {current !== EVERYTHING && fieldOptions && <div className='flex max-h-64 flex-col gap-1 overflow-y-auto'>
          {fieldOptions.truncated && <p className='text-sm text-warning'>{t('fields.truncated')}</p>}
          {!fieldOptions.options.length && <p className='text-sm opacity-70'>{t('fields.noOptions')}</p>}
          {fieldOptions.options.map((option) => {
            const id = String(option.id)
            return <Checkbox
              key={id}
              isSelected={picked.includes(id)}
              onChange={(isSelected) => {
                const next = isSelected
                  ? [ ...picked, id ]
                  : picked.filter((existing) => existing !== id)
                // Boards are numbers to the API; keep each option's own type.
                const typed = fieldOptions.options
                  .filter((candidate) => next.includes(String(candidate.id)))
                  .map((candidate) => candidate.id)
                set(field.id, typed)
              }}
            >
              <Checkbox.Content>
                <Checkbox.Control>
                  <Checkbox.Indicator />
                </Checkbox.Control>
                <span className='text-sm'>{option.label}</span>
                {option.detail && <span className='ml-2 text-xs opacity-60'>{option.detail}</span>}
              </Checkbox.Content>
            </Checkbox>
          })}
        </div>}
      </div>
    })
  }</div>
}
