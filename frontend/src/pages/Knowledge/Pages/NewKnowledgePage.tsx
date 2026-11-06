// Copyright © 2026 Jalapeno Labs

// Core
import { useEffect, useMemo } from 'react'
import { useForm, useWatch } from 'react-hook-form'
import { useTranslation } from 'react-i18next'
import { useNavigate, useSearchParams } from 'react-router'

// Redux
import { useAppDispatch, useAppSelector } from '../../../store/hooks'
import { pageLoaded, selectPageTree } from '../../../store/pagesSlice'

// User interface
import { Form, Input, Label, TextField } from '@heroui/react'
import { MarkdownEditor } from '../../../components/markdownEditor/MarkdownEditor'
import { Page } from '../../../layout/page/Page'
import { PageCreateFooter } from '../../../layout/page/PageCreateFooter'
import { usePageSave } from '../../../layout/page/pageSave'
import { PageParentPicker } from './PageParentPicker'

// Misc
import { createPage, PAGE_TITLE_MAX_CHARACTERS } from '../../../api/routes/pageRoutes'
import { useFormDraft } from '../../../drafts/useFormDraft'
import { useInitialFocus } from '../../../hooks/useInitialFocus'
import { usePagesLoader } from '../../../hooks/useServerData'
import { getKnowledgePageUrl, NEW_PAGE_PARENT_PARAM, UrlTree } from '../../../urls'
import { createPageFormSchema, describePublishRefusal } from './pageForm'
import { toast } from '../../../toast/toast'

type NewPageValues = {
  title: string
  body: string
  // The page it goes under, or '' for the top of the tree.
  parentId: string
}

// A page, under any other or at the top, published as its version 1. What is typed stays for the
// tab until it is published. `?parent=` names where it goes, which a draft kept from another time
// never overrides.
function NewPageForm() {
  const { t } = useTranslation('knowledge')
  const dispatch = useAppDispatch()
  const navigate = useNavigate()
  const [ searchParams ] = useSearchParams()
  const presetParent = searchParams.get(NEW_PAGE_PARENT_PARAM)
  const treeStatus = usePagesLoader()
  const tree = useAppSelector(selectPageTree)
  const titleRef = useInitialFocus()

  const form = useForm<NewPageValues>({
    defaultValues: { title: '', body: '', parentId: presetParent ?? '' },
  })
  const forgetDraft = useFormDraft(form, {
    name: 'knowledge-page',
    fields: [ 'title', 'body' ],
    choices: presetParent
      ? []
      : [ 'parentId' ],
  })
  const values = useWatch({ control: form.control })
  const parentId = values.parentId ?? ''

  // A parent that is gone, such as one a draft kept, gives way to the top of the tree, so the form
  // never holds a place the API would refuse.
  const isParentGone = treeStatus === 'loaded' && Boolean(parentId) && !tree.byId.has(parentId)
  useEffect(() => {
    if (isParentGone) {
      console.debug('NewKnowledgePage dropped a parent that no longer exists', { parentId })
      form.setValue('parentId', '', { shouldDirty: true })
    }
  }, [ form, isParentGone, parentId ])

  const schema = useMemo(() => createPageFormSchema(t), [ t ])
  const parsed = schema.safeParse({ title: values.title, body: values.body })

  async function publish() {
    if (!parsed.success) {
      console.debug('NewKnowledgePage kept the form: its values are invalid')
      return false
    }
    try {
      const response = await createPage({
        title: parsed.data.title,
        body: parsed.data.body,
        parentId: parentId || undefined,
      })
      dispatch(pageLoaded(response.page))
      forgetDraft()
      toast.success(t('pages.editor.published'))
      void navigate(getKnowledgePageUrl(response.page.id), { replace: true })
      return true
    }
    catch (error) {
      console.debug('NewKnowledgePage could not create the page', { error })
      toast.danger(t('pages.editor.publishFailed'), { description: describePublishRefusal(error, t) ?? undefined })
      return false
    }
  }

  const onSubmit = usePageSave({
    save: publish,
    isDirty: form.formState.isDirty,
    blockedReason: parsed.success
      ? null
      : parsed.error.issues[0].message,
    label: t('pages.publish'),
    onDiscard: forgetDraft,
  })

  return <Form onSubmit={onSubmit} validationBehavior='aria' className='flex max-w-4xl flex-col gap-4'>
    <div className='grid gap-4 sm:grid-cols-2'>
      <TextField
        id='page-title'
        isRequired
        value={values.title ?? ''}
        onChange={(value) => form.setValue('title', value, { shouldDirty: true })}
      >
        <Label>{t('pages.editor.title')}</Label>
        <Input ref={titleRef} autoComplete='off' maxLength={PAGE_TITLE_MAX_CHARACTERS} />
      </TextField>
      <PageParentPicker
        id='page-parent'
        tree={tree}
        value={parentId}
        onChange={(value) => form.setValue('parentId', value, { shouldDirty: true })}
      />
    </div>
    <MarkdownEditor
      id='page-body'
      label={t('pages.editor.body')}
      placeholder={t('pages.editor.placeholder')}
      markdown={values.body ?? ''}
      onChange={(value) => form.setValue('body', value, { shouldDirty: true })}
      // A file belongs to a page, and this one does not exist until it is published.
      attachments={{ canAttach: false, reason: t('attachments.publishFirst') }}
    />
    <PageCreateFooter />
  </Form>
}

// `/knowledge/pages/new`.
export function NewKnowledgePage() {
  const { t } = useTranslation('knowledge')

  return <Page
    title={t('pages.newTitle')}
    parents={[{ label: t('title'), href: UrlTree.knowledge }]}
  >
    <NewPageForm />
  </Page>
}
