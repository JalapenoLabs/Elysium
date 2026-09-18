// Copyright © 2026 Jalapeno Labs

import type { Key } from '@heroui/react'
import type { Ref } from 'react'
import type { ActionItem, ActionItemComment } from '../../api/routes/actionItemRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch, useAppSelector } from '../../store/hooks'
import {
  actionItemCommentDeleted,
  actionItemCommentUpserted,
  selectActionItemComments,
} from '../../store/actionItemCommentsSlice'

// User interface
import { Button, Description, Dropdown, Form, Label, Spinner, TextArea, TextField, toast } from '@heroui/react'
import { LuEllipsis } from 'react-icons/lu'

// Misc
import {
  createActionItemComment,
  deleteActionItemComment,
  updateActionItemComment,
} from '../../api/routes/actionItemRoutes'
import { useConfirm } from '../../hooks/useConfirm'
import { usePrompt } from '../../hooks/usePrompt'
import { useActionItemCommentsLoader } from '../../hooks/useServerData'
import { describeActor } from './actionItemPresentation'

// A comment's length, as the API bounds it.
const COMMENT_MAX_CHARACTERS = 20_000

type Props = {
  item: ActionItem
  // Lets a page focus the composer, such as from a keyboard shortcut.
  composerRef?: Ref<HTMLTextAreaElement>
}

// An item's conversation, oldest first, with a composer below. Only the user's own
// comments can be edited or deleted; the API refuses anyone else's.
export function ActionItemComments(props: Props) {
  const { t, i18n } = useTranslation([ 'actionItems', 'common' ])
  const dispatch = useAppDispatch()
  const confirm = useConfirm()
  const prompt = usePrompt()
  const status = useActionItemCommentsLoader(props.item.id)
  const comments = useAppSelector((state) => selectActionItemComments(state, props.item.id))
  const [ draft, setDraft ] = useState('')
  const [ isPosting, setIsPosting ] = useState(false)
  const isDeleted = Boolean(props.item.deletedAt)
  const dateFormatter = new Intl.DateTimeFormat(i18n.language, { dateStyle: 'medium', timeStyle: 'short' })

  async function post() {
    const body = draft.trim()
    if (!body) {
      console.debug('ActionItemComments ignored an empty comment')
      return
    }
    setIsPosting(true)
    try {
      const response = await createActionItemComment(props.item.id, body)
      dispatch(actionItemCommentUpserted(response.comment))
      setDraft('')
    }
    catch (error) {
      console.debug('ActionItemComments failed to post a comment', { error, itemId: props.item.id })
      toast.danger(t('common:errors.unexpected'))
    }
    finally {
      setIsPosting(false)
    }
  }

  function edit(comment: ActionItemComment) {
    prompt({
      title: t('comments.editTitle'),
      label: t('comments.label'),
      defaultValue: comment.body,
      isMultiline: true,
      maxLength: COMMENT_MAX_CHARACTERS,
      onSubmit: async (body) => {
        try {
          const response = await updateActionItemComment(props.item.id, comment.id, body)
          dispatch(actionItemCommentUpserted(response.comment))
        }
        catch (error) {
          console.debug('ActionItemComments failed to edit a comment', { error, commentId: comment.id })
          toast.danger(t('common:errors.unexpected'))
          // Keeps the dialog open to try again.
          throw error
        }
      },
    })
  }

  function remove(comment: ActionItemComment) {
    confirm({
      title: t('comments.deleteTitle'),
      message: t('comments.deleteBody'),
      tone: 'danger',
      confirmText: t('common:actions.delete'),
      onConfirm: async () => {
        try {
          await deleteActionItemComment(props.item.id, comment.id)
        }
        catch (error) {
          console.debug('ActionItemComments failed to delete a comment', { error, commentId: comment.id })
          toast.danger(t('common:errors.unexpected'))
          throw error
        }
        dispatch(actionItemCommentDeleted(comment.id))
      },
    })
  }

  if (status === 'loading') {
    return <div className='grid place-items-center py-6'>
      <Spinner size='sm' />
    </div>
  }

  return <div>
    {status === 'failed' && <p className='compact text-sm text-danger'>{t('comments.loadError')}</p>}
    {!comments.length && status === 'loaded' && <p className='compact text-sm opacity-70'>{
      t('comments.empty')
    }</p>}
    <ol className='compact flex flex-col gap-3'>{
      comments.map((comment) => {
        const author = describeActor(comment.author)
        const isOwn = comment.author === 'user'
        const isEdited = comment.updatedAt !== comment.createdAt
        return <li key={comment.id} className='rounded-xl bg-surface p-3'>
          <div className='level mb-1 text-xs'>
            <span>
              <span className='font-semibold'>{t(author.key, author.values)}</span>
              <span className='ml-2 opacity-60'>{dateFormatter.format(new Date(comment.createdAt))}</span>
              {isEdited && <span className='ml-2 opacity-60'>{t('comments.edited')}</span>}
            </span>
            {isOwn && !isDeleted && <Dropdown>
              <Button isIconOnly size='sm' variant='ghost' aria-label={t('common:actions.moreActions')}>
                <LuEllipsis className='size-4' aria-hidden />
              </Button>
              <Dropdown.Popover placement='bottom end'>
                <Dropdown.Menu onAction={(key: Key) => {
                  if (key === 'edit') {
                    edit(comment)
                    return
                  }
                  remove(comment)
                }}>
                  <Dropdown.Item id='edit' textValue={t('common:actions.edit')}>
                    <Label>{t('common:actions.edit')}</Label>
                  </Dropdown.Item>
                  <Dropdown.Item id='delete' textValue={t('common:actions.delete')} variant='danger'>
                    <Label>{t('common:actions.delete')}</Label>
                  </Dropdown.Item>
                </Dropdown.Menu>
              </Dropdown.Popover>
            </Dropdown>}
          </div>
          <p className='text-sm whitespace-pre-line'>{comment.body}</p>
        </li>
      })
    }</ol>

    {!isDeleted && <Form
      validationBehavior='aria'
      onSubmit={(event) => {
        event.preventDefault()
        void post()
      }}
    >
      <TextField
        className='w-full'
        value={draft}
        maxLength={COMMENT_MAX_CHARACTERS}
        onChange={setDraft}
      >
        <Label className='sr-only'>{t('comments.label')}</Label>
        <TextArea
          ref={props.composerRef}
          rows={2}
          placeholder={t('comments.placeholder')}
          onKeyDown={(event) => {
            // Enter adds a line; Ctrl or Cmd with Enter posts, as in the inline editors.
            if (event.key === 'Enter' && (event.ctrlKey || event.metaKey)) {
              event.preventDefault()
              void post()
            }
          }}
        />
        <Description>{t('comments.hint')}</Description>
      </TextField>
      <div className='mt-2 flex justify-end'>
        <Button type='submit' size='sm' isDisabled={!draft.trim()} isPending={isPosting}>
          <span>{t('comments.post')}</span>
        </Button>
      </div>
    </Form>}
  </div>
}
