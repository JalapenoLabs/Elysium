// Copyright © 2026 Jalapeno Labs

// User interface
import { ComboBox, FieldError, Input, Label, ListBox } from '@heroui/react'

// Misc
import { isModelName, modelProductNames } from './modelSettingsPresentation'

type Props = {
  // The input's id, stable so the field can be found and labelled.
  id: string
  // Read by screen readers; the column header says it to everyone else.
  label: string
  value: string
  // The current Claude models, offered first. Any other id or alias the CLI takes may be typed.
  suggestions: string[]
  invalidMessage: string
  onChange: (model: string) => void
}

// A model for one step or agent: picked from the suggestions, named by product, or typed, such
// as `opus[1m]`. What is typed is the value, so a pick and a typed id read alike.
export function ModelField(props: Props) {
  const isInvalid = !isModelName(props.value.trim())

  return <ComboBox
    id={props.id}
    aria-label={props.label}
    allowsCustomValue
    menuTrigger='focus'
    isInvalid={isInvalid}
    inputValue={props.value}
    onInputChange={props.onChange}
    onSelectionChange={(key) => {
      if (key === null) {
        // Clearing a pick leaves the text where it was.
        return
      }
      props.onChange(String(key))
    }}
  >
    <Label className='sr-only'>{props.label}</Label>
    <ComboBox.InputGroup>
      <Input autoComplete='off' className='font-mono text-sm' />
      <ComboBox.Trigger />
    </ComboBox.InputGroup>
    <FieldError>{props.invalidMessage}</FieldError>
    <ComboBox.Popover>
      <ListBox>{
        props.suggestions.map((model) => <ListBox.Item key={model} id={model} textValue={model}>
          <div className='flex flex-col'>
            <span>{modelProductNames.get(model) ?? model}</span>
            <code className='font-mono text-xs opacity-60'>{model}</code>
          </div>
          <ListBox.ItemIndicator />
        </ListBox.Item>)
      }</ListBox>
    </ComboBox.Popover>
  </ComboBox>
}
