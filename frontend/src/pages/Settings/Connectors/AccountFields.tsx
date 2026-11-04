// Copyright © 2026 Jalapeno Labs

import type { AccountDefinition } from '../../../api/routes/connectorRoutes'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Input, Label, ListBox, Select, TextField } from '@heroui/react'
import { SecretInput } from '../../../components/SecretInput'

type Props = {
  definition: AccountDefinition
  values: Record<string, string>
  onChange: (values: Record<string, string>) => void
}

const inputTypes = {
  text: 'text',
  email: 'email',
  url: 'url',
} as const

// A new account's form, drawn from its kind's definition: text, email, URL, and secret fields,
// and choices. A secret is written once here and never comes back from the API; each has a
// toggle to show what was typed.
export function AccountFields(props: Props) {
  const { t } = useTranslation('connectors')

  function set(fieldId: string, value: string) {
    props.onChange({ ...props.values, [fieldId]: value })
  }

  return <div className='flex flex-col gap-3'>{
    props.definition.fields.map((field) => {
      const label = t(`fields.${field.id}`, { defaultValue: field.id })
      if (field.field.type === 'choice') {
        return <Select
          key={field.id}
          isRequired={field.required}
          value={props.values[field.id] || null}
          onChange={(key) => set(field.id, String(key))}
        >
          <Label>{label}</Label>
          <Select.Trigger>
            <Select.Value />
            <Select.Indicator />
          </Select.Trigger>
          <Select.Popover>
            <ListBox>{
              field.field.options.map((option) => <ListBox.Item key={option} id={option} textValue={option}>
                {t(`fields.choices.${option}`, { defaultValue: option })}
                <ListBox.ItemIndicator />
              </ListBox.Item>)
            }</ListBox>
          </Select.Popover>
        </Select>
      }
      if (field.field.type === 'secret') {
        return <SecretInput
          key={field.id}
          isRequired={field.required}
          isMonospace
          label={label}
          value={props.values[field.id] ?? ''}
          onChange={(value) => set(field.id, value)}
        />
      }
      return <TextField
        key={field.id}
        isRequired={field.required}
        type={inputTypes[field.field.type]}
        autoComplete='off'
        value={props.values[field.id] ?? ''}
        onChange={(value) => set(field.id, value)}
      >
        <Label>{label}</Label>
        <Input />
      </TextField>
    })
  }</div>
}
