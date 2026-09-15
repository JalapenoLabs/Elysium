// Copyright © 2026 Jalapeno Labs

// Core
import { useState } from 'react'

// User interface
import { Card, Checkbox, Link } from '@heroui/react'
import { LuExternalLink } from 'react-icons/lu'

const COMMAND_CLASS_NAME = 'mt-2 ml-7 block overflow-x-auto rounded-md bg-default px-3 py-2 text-xs'

// One step, already translated. Commands, file paths, and URLs are never translated.
export type SetupChecklistStep = {
  id: string
  text: string
  command?: string
  link?: string
}

type Props = {
  title: string
  description: string
  steps: SetupChecklistStep[]
}

// Steps for getting a credential from another service, shown beside the form it goes
// into. Checking one off is only a reading aid, so the state lives here and resets when
// the page is left.
export function SetupChecklist(props: Props) {
  const [ completedSteps, setCompletedSteps ] = useState<ReadonlySet<string>>(new Set())

  function toggleStep(id: string, isCompleted: boolean) {
    setCompletedSteps((previous) => {
      const next = new Set(previous)
      if (isCompleted) {
        next.add(id)
      }
      else {
        next.delete(id)
      }
      return next
    })
  }

  return <Card>
    <Card.Header>
      <Card.Title>{props.title}</Card.Title>
      <Card.Description>{props.description}</Card.Description>
    </Card.Header>
    <Card.Content>
      <ol className='flex flex-col gap-4'>{
        props.steps.map((step) => {
          const isCompleted = completedSteps.has(step.id)
          return <li key={step.id}>
            <Checkbox
              isSelected={isCompleted}
              onChange={(isSelected) => toggleStep(step.id, isSelected)}
            >
              <Checkbox.Content className='items-start'>
                <Checkbox.Control className='mt-0.5 shrink-0'>
                  <Checkbox.Indicator />
                </Checkbox.Control>
                <span className={isCompleted
                  ? 'text-sm line-through opacity-60'
                  : 'text-sm'}
                >{
                  step.text
                }</span>
              </Checkbox.Content>
            </Checkbox>
            {step.command && <code className={COMMAND_CLASS_NAME}>{
              step.command
            }</code>}
            {step.link && <Link
              href={step.link}
              target='_blank'
              rel='noreferrer'
              className='mt-1 ml-7 inline-flex items-center gap-1 text-xs text-link'
            >
              <span>{new URL(step.link).host}</span>
              <LuExternalLink className='size-3' aria-hidden />
            </Link>}
          </li>
        })
      }</ol>
    </Card.Content>
  </Card>
}
