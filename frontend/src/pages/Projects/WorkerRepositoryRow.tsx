// Copyright © 2026 Jalapeno Labs

import type { IconType } from 'react-icons'
import type { WorkerRepositoryValue } from './workerSetupFormSchema'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Description, FieldError, Input, Label, TextArea, TextField, Tooltip } from '@heroui/react'
import { LuArrowDown, LuArrowUp, LuX } from 'react-icons/lu'

// Misc
import { getRepositoryDirectory } from '../Coding/sessionRepositories'
import { parseGithubRepository } from '../Settings/Github/githubPresentation'

type Props = {
  index: number
  // How many rows the list holds, so the last one cannot move down.
  count: number
  value: WorkerRepositoryValue
  errors: Partial<Record<keyof WorkerRepositoryValue, string>>
  onChange: (field: keyof WorkerRepositoryValue, value: string) => void
  onMove: (to: number) => void
  onRemove: () => void
}

// One repository the project declares: its URL, the branch work starts from, and the script that
// readies it once it is cloned, with buttons to move it in the list or take it out.
export function WorkerRepositoryRow(props: Props) {
  const { t } = useTranslation('projects')
  const url = props.value.url.trim()
  // Named as people know it: owner/name on GitHub, else the directory it clones into, and by its
  // place until a URL names it.
  const repository = parseGithubRepository(url)
    ?? getRepositoryDirectory(url)
    ?? t('workers.newRepository', { position: props.index + 1 })
  const id = `worker-setup-repository-${props.index}`

  return <li className='compact rounded-xl border border-separator p-3'>
    <div className='compact level gap-2'>
      <span className='min-w-0 truncate font-medium' title={url}>{repository}</span>
      <div className='level-right shrink-0 gap-1'>
        <RowButton
          label={t('workers.moveUp', { repository })}
          icon={LuArrowUp}
          isDisabled={props.index === 0}
          onPress={() => props.onMove(props.index - 1)}
        />
        <RowButton
          label={t('workers.moveDown', { repository })}
          icon={LuArrowDown}
          isDisabled={props.index === props.count - 1}
          onPress={() => props.onMove(props.index + 1)}
        />
        <RowButton
          label={t('workers.remove', { repository })}
          icon={LuX}
          onPress={props.onRemove}
        />
      </div>
    </div>

    <div className='compact flex flex-col gap-3 sm:flex-row'>
      <TextField
        id={`${id}-url`}
        className='min-w-0 flex-1'
        isRequired
        isInvalid={Boolean(props.errors.url)}
        value={props.value.url}
        onChange={(value) => props.onChange('url', value)}
      >
        <Label>{t('workers.url')}</Label>
        <Input
          className='font-mono'
          placeholder='https://github.com/org/repo.git'
          autoComplete='url'
        />
        <FieldError>{props.errors.url}</FieldError>
      </TextField>
      <TextField
        id={`${id}-branch`}
        className='sm:w-56'
        isInvalid={Boolean(props.errors.baseBranch)}
        value={props.value.baseBranch}
        onChange={(value) => props.onChange('baseBranch', value)}
      >
        <Label>{t('workers.baseBranch')}</Label>
        <Input
          className='font-mono'
          placeholder={t('workers.defaultBranch')}
          autoComplete='off'
        />
        <FieldError>{props.errors.baseBranch}</FieldError>
      </TextField>
    </div>

    <TextField
      id={`${id}-setup-script`}
      isInvalid={Boolean(props.errors.setupScript)}
      value={props.value.setupScript}
      onChange={(value) => props.onChange('setupScript', value)}
    >
      <Label>{t('workers.setupScript')}</Label>
      <TextArea
        className='font-mono text-sm'
        rows={4}
        autoComplete='off'
        spellCheck={false}
      />
      <Description>{t('workers.setupScriptHint')}</Description>
      <FieldError>{props.errors.setupScript}</FieldError>
    </TextField>
  </li>
}

type RowButtonProps = {
  label: string
  icon: IconType
  isDisabled?: boolean
  onPress: () => void
}

// An icon-only button of the row, named in its tooltip.
function RowButton(props: RowButtonProps) {
  return <Tooltip delay={300}>
    <Button
      isIconOnly
      size='sm'
      variant='ghost'
      aria-label={props.label}
      isDisabled={props.isDisabled}
      onPress={props.onPress}
    >
      <props.icon className='size-4' aria-hidden />
    </Button>
    <Tooltip.Content>
      <span>{props.label}</span>
    </Tooltip.Content>
  </Tooltip>
}
