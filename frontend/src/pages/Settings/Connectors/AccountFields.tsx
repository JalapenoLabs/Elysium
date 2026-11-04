// Copyright © 2026 Jalapeno Labs

import type { AccountDefinition } from '../../../api/routes/connectorRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Input, InputGroup, Label, ListBox, Select, TextField, Tooltip } from '@heroui/react'
import { LuEye, LuEyeOff } from 'react-icons/lu'

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
  const [ revealed, setRevealed ] = useState<ReadonlySet<string>>(new Set())

  function toggleReveal(fieldId: string) {
    const next = new Set(revealed)
    if (!next.delete(fieldId)) {
      next.add(fieldId)
    }
    setRevealed(next)
  }

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
        const isRevealed = revealed.has(field.id)
        const toggleLabel = isRevealed
          ? t('fields.hideSecret')
          : t('fields.revealSecret')
        return <TextField
          key={field.id}
          isRequired={field.required}
          type={isRevealed ? 'text' : 'password'}
          autoComplete='off'
          value={props.values[field.id] ?? ''}
          onChange={(value) => set(field.id, value)}
        >
          <Label>{label}</Label>
          <InputGroup>
            <InputGroup.Input className='font-mono' spellCheck={false} />
            <InputGroup.Suffix>
              <Tooltip delay={400}>
                <Tooltip.Trigger>
                  <Button
                    isIconOnly
                    size='sm'
                    variant='ghost'
                    aria-label={toggleLabel}
                    aria-pressed={isRevealed}
                    onPress={() => toggleReveal(field.id)}
                  >
                    {isRevealed
                      ? <LuEyeOff className='size-4' aria-hidden />
                      : <LuEye className='size-4' aria-hidden />}
                  </Button>
                </Tooltip.Trigger>
                <Tooltip.Content>{toggleLabel}</Tooltip.Content>
              </Tooltip>
            </InputGroup.Suffix>
          </InputGroup>
        </TextField>
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
