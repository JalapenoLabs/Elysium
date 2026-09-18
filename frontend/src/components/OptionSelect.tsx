// Copyright © 2026 Jalapeno Labs

import type { Key } from '@heroui/react'
import type { PickerOption } from './MultiPicker'

// User interface
import { Label, ListBox, Select } from '@heroui/react'

type Props = {
  label: string
  options: PickerOption[]
  value: string
  onChange: (value: string) => void
  className?: string
  isDisabled?: boolean
}

// A labelled select over a short, fixed list, such as a filter's Any, Yes, and No, or an
// item's priority.
export function OptionSelect(props: Props) {
  return <Select
    className={props.className}
    isDisabled={props.isDisabled}
    value={props.value}
    onChange={(key: Key | null) => {
      if (key === null) {
        console.debug('OptionSelect ignored an empty selection', { label: props.label })
        return
      }
      props.onChange(String(key))
    }}
  >
    <Label>{props.label}</Label>
    <Select.Trigger>
      <Select.Value />
      <Select.Indicator />
    </Select.Trigger>
    <Select.Popover>
      <ListBox>{
        props.options.map((option) => <ListBox.Item key={option.id} id={option.id} textValue={option.label}>
          {option.label}
          <ListBox.ItemIndicator />
        </ListBox.Item>)
      }</ListBox>
    </Select.Popover>
  </Select>
}
