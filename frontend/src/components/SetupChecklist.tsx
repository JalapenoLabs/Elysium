// Copyright © 2026 Jalapeno Labs

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Card, Checkbox, Link } from '@heroui/react'
import { LuExternalLink } from 'react-icons/lu'
import { CopyButton } from './CopyButton'

// One step, already translated. Commands, file paths, and URLs are never translated.
export type SetupChecklistStep = {
  id: string
  text: string
  command?: string
  link?: string
}

type Props = {
  // Prefixes each step's checkbox id: `<id>-<step id>`.
  id: string
  title: string
  steps: SetupChecklistStep[]
}

// Steps for getting a credential from another service, shown beside the form it goes
// into. Checking one off is only a reading aid, so the state lives here and resets when
// the page is left.
export function SetupChecklist(props: Props) {
  const { t } = useTranslation('common')
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
    </Card.Header>
    <Card.Content>
      <ol className='flex flex-col gap-4'>{
        props.steps.map((step) => {
          const isCompleted = completedSteps.has(step.id)
          return <li key={step.id}>
            <Checkbox
              id={`${props.id}-${step.id}`}
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
            {step.command && <div className='mt-2 ml-7 level gap-1 rounded-md bg-default py-1 pr-1 pl-3'>
              <code className='min-w-0 flex-1 overflow-x-auto text-xs'>{step.command}</code>
              <CopyButton value={step.command} label={t('clipboard.copyCommand')} />
            </div>}
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
