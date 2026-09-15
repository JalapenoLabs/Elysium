// Copyright © 2026 Jalapeno Labs

import type { LlmType } from '../../../api/routes/llmRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Card, Checkbox, Link } from '@heroui/react'
import { LuExternalLink } from 'react-icons/lu'

// Misc
import { llmSetupStepsByType } from './llmProviders'

const COMMAND_CLASS_NAME = 'mt-2 ml-7 block overflow-x-auto rounded-md bg-default px-3 py-2 text-xs'

type Props = {
  type: LlmType
}

// Steps for obtaining the secret token. Checking one off is only a reading aid, so
// the state lives here and resets when the page is left.
export function LlmSetupChecklist(props: Props) {
  const { t } = useTranslation('llms')
  const steps = llmSetupStepsByType[props.type]
  const [ completedSteps, setCompletedSteps ] = useState<ReadonlySet<number>>(new Set())

  function toggleStep(index: number, isCompleted: boolean) {
    setCompletedSteps((previous) => {
      const next = new Set(previous)
      if (isCompleted) {
        next.add(index)
      }
      else {
        next.delete(index)
      }
      return next
    })
  }

  return <Card>
    <Card.Header>
      <Card.Title>{t('setup.title')}</Card.Title>
      <Card.Description>{t('setup.description')}</Card.Description>
    </Card.Header>
    <Card.Content>
      <ol className='flex flex-col gap-4'>{
        steps.map((step, index) => {
          const isCompleted = completedSteps.has(index)
          return <li key={step.textKey}>
            <Checkbox
              isSelected={isCompleted}
              onChange={(isSelected) => toggleStep(index, isSelected)}
            >
              <Checkbox.Content className='items-start'>
                <Checkbox.Control className='mt-0.5 shrink-0'>
                  <Checkbox.Indicator />
                </Checkbox.Control>
                <span className={isCompleted
                  ? 'text-sm line-through opacity-60'
                  : 'text-sm'}
                >{
                  t(step.textKey)
                }</span>
              </Checkbox.Content>
            </Checkbox>
            {'command' in step && <code className={COMMAND_CLASS_NAME}>{
              step.command
            }</code>}
            {'link' in step && <Link
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
