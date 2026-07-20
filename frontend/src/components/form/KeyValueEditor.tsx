// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Input, Tooltip } from '@heroui/react'
import { FiPlus, FiX } from 'react-icons/fi'
import { SecretInput } from './SecretInput'

// A controlled key/value config editor for connection forms and provider
// settings (headers, S3 options, environment-style pairs). The caller owns the
// entries; this component only edits the array it is given.
export type KeyValueEntry = {
  key: string
  value: string
}

type Props = {
  entries: KeyValueEntry[]
  onChange: (entries: KeyValueEntry[]) => void
  keyLabel?: string
  valueLabel?: string
  // Mask the value column (credentials, tokens) behind SecretInput.
  isSecretValues?: boolean
  isDisabled?: boolean
}

export function KeyValueEditor(props: Props) {
  const { t } = useTranslation()

  const keyLabel = props.keyLabel ?? t('editor.keyLabel')
  const valueLabel = props.valueLabel ?? t('editor.valueLabel')

  function updateEntry(index: number, patch: Partial<KeyValueEntry>) {
    props.onChange(props.entries.map((entry, entryIndex) => {
      if (entryIndex !== index) {
        return entry
      }
      return { ...entry, ...patch }
    }))
  }

  return <div>
    {props.entries.map((entry, index) => (
      <div key={index} className='compact flex items-start gap-2'>
        <Input
          size='sm'
          label={keyLabel}
          value={entry.key}
          isDisabled={props.isDisabled}
          onValueChange={(key) => updateEntry(index, { key })}
        />
        {props.isSecretValues
          ? <SecretInput
            label={valueLabel}
            value={entry.value}
            isDisabled={props.isDisabled}
            onValueChange={(value) => updateEntry(index, { value })}
          />
          : <Input
            size='sm'
            label={valueLabel}
            value={entry.value}
            isDisabled={props.isDisabled}
            onValueChange={(value) => updateEntry(index, { value })}
          />}
        <Tooltip content={t('editor.removeRow')}>
          <div className='pt-1'>
            <Button
              isIconOnly
              size='sm'
              variant='light'
              aria-label={t('editor.removeRow')}
              isDisabled={props.isDisabled}
              onPress={() => {
                props.onChange(props.entries.filter((_entry, entryIndex) => entryIndex !== index))
              }}
            >
              <FiX className='h-4 w-4 opacity-60' />
            </Button>
          </div>
        </Tooltip>
      </div>
    ))}
    <Button
      size='sm'
      variant='bordered'
      startContent={<FiPlus className='h-4 w-4' />}
      isDisabled={props.isDisabled}
      onPress={() => {
        props.onChange([ ...props.entries, { key: '', value: '' }])
      }}
    >
      <span>{
        t('editor.addRow')
      }</span>
    </Button>
  </div>
}
