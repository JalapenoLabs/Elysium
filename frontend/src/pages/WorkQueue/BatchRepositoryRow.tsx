// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Button, FieldError, Input, Label, TextField, Tooltip } from '@heroui/react'
import { LuX } from 'react-icons/lu'

// Misc
import { getRepositoryDirectory } from '../Coding/sessionRepositories'
import { parseGithubRepository } from '../Settings/Github/githubPresentation'

type Props = {
  index: number
  url: string
  baseBranch: string
  errors: {
    url?: string
    baseBranch?: string
  }
  onChange: (field: 'url' | 'baseBranch', value: string) => void
  onRemove: () => void
}

// One repository a proposed batch needs: its URL and the branch its work starts from, with a
// button to take it out.
export function BatchRepositoryRow(props: Props) {
  const { t } = useTranslation('workQueue')
  const url = props.url.trim()
  // Named as people know it: owner/name on GitHub, else the directory it clones into, and by its
  // place until a URL names it.
  const repository = parseGithubRepository(url)
    ?? getRepositoryDirectory(url)
    ?? t('edit.newRepository', { position: props.index + 1 })
  const id = `work-batch-repository-${props.index}`

  return <li className='compact rounded-xl border border-separator p-3'>
    <div className='compact level gap-2'>
      <span className='min-w-0 truncate font-medium' title={url}>{repository}</span>
      <Tooltip delay={300}>
        <Button
          isIconOnly
          size='sm'
          variant='ghost'
          aria-label={t('edit.removeRepository', { repository })}
          onPress={props.onRemove}
        >
          <LuX className='size-4' aria-hidden />
        </Button>
        <Tooltip.Content>
          <span>{t('edit.removeRepository', { repository })}</span>
        </Tooltip.Content>
      </Tooltip>
    </div>
    <div className='flex flex-col gap-3 sm:flex-row'>
      <TextField
        id={`${id}-url`}
        className='min-w-0 flex-1'
        isRequired
        isInvalid={Boolean(props.errors.url)}
        value={props.url}
        onChange={(value) => props.onChange('url', value)}
      >
        <Label>{t('edit.repositoryUrl')}</Label>
        <Input placeholder='https://github.com/org/repo' autoComplete='url' spellCheck={false} />
        <FieldError>{props.errors.url}</FieldError>
      </TextField>
      <TextField
        id={`${id}-branch`}
        className='min-w-0 sm:w-56'
        isInvalid={Boolean(props.errors.baseBranch)}
        value={props.baseBranch}
        onChange={(value) => props.onChange('baseBranch', value)}
      >
        <Label>{t('edit.baseBranch')}</Label>
        <Input autoComplete='off' spellCheck={false} />
        <FieldError>{props.errors.baseBranch}</FieldError>
      </TextField>
    </div>
  </li>
}
