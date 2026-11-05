// Copyright © 2026 Jalapeno Labs

import type { KnowledgePage } from '../../../api/routes/pageRoutes'
import type { PageConflict } from './PageConflictDialog'

// Core
import { useMemo, useState } from 'react'
import { useForm, useWatch } from 'react-hook-form'
import { useTranslation } from 'react-i18next'
import { Navigate, useNavigate, useParams } from 'react-router'

// Redux
import { useAppDispatch, useAppSelector } from '../../../store/hooks'
import { pageLoaded, selectOpenedPage, selectPageTree } from '../../../store/pagesSlice'

// User interface
import { Link, Spinner } from '@heroui/react'
import { LuGitCompareArrows } from 'react-icons/lu'
import { MarkdownEditor } from '../../../components/markdownEditor/MarkdownEditor'
import { Page } from '../../../layout/page/Page'
import { LearningEditFields } from './LearningEditFields'
import { PageConflictDialog } from './PageConflictDialog'

// Misc
import { getApiErrorCode } from '../../../api/errors'
import { getPage, getPageVersion, PAGE_TITLE_MAX_CHARACTERS, publishPage } from '../../../api/routes/pageRoutes'
import { useFormDraft } from '../../../drafts/useFormDraft'
import { usePageLoader } from '../../../hooks/useServerData'
import { getKnowledgePageUrl, UrlTree } from '../../../urls'
import { createLearningPageFormSchema, createPageFormSchema, describePublishRefusal } from './pageForm'
import { findPageAncestors } from './pageTree'
import { toast } from '../../../toast/toast'

type EditorProps = {
  // The page as it was opened. Later versions change nothing here: this person's text stays theirs
  // until they publish it, which is what tells them if someone got there first.
  page: KnowledgePage
}

type EditPageValues = {
  title: string
  body: string
  category: string
  evidence: string
  // The version this text was edited from, kept with the draft, so a reload still tells a publish
  // what it has not seen.
  baseVersion: number
}

function PageEditor(props: EditorProps) {
  const { t } = useTranslation('knowledge')
  const dispatch = useAppDispatch()
  const navigate = useNavigate()
  const { page } = props
  const tree = useAppSelector(selectPageTree)
  const [ conflict, setConflict ] = useState<PageConflict | null>(null)
  const [ isConflictOpen, setIsConflictOpen ] = useState(false)
  const isLearning = page.category !== null

  const form = useForm<EditPageValues>({
    defaultValues: {
      title: page.title,
      body: page.body,
      category: page.category ?? '',
      evidence: page.evidence ?? '',
      baseVersion: page.currentVersion,
    },
  })
  const forgetDraft = useFormDraft(form, {
    name: `knowledge-page.${page.id}`,
    fields: [ 'title', 'body', 'evidence' ],
    choices: [ 'category', 'baseVersion' ],
  })
  const values = useWatch({ control: form.control })
  const title = values.title ?? ''
  const body = values.body ?? ''
  const evidence = values.evidence ?? ''

  const schema = useMemo(
    () => isLearning
      ? createLearningPageFormSchema(t)
      : createPageFormSchema(t),
    [ isLearning, t ],
  )
  const parsed = schema.safeParse({ title, body, evidence, category: values.category })

  async function showConflict() {
    const baseVersion = form.getValues('baseVersion')
    try {
      const [ latest, opened ] = await Promise.all([ getPage(page.id), getPageVersion(page.id, baseVersion) ])
      dispatch(pageLoaded(latest.page))
      setConflict({ latest: latest.page, opened: opened.version })
      setIsConflictOpen(true)
    }
    catch (error) {
      console.debug('PageEditor could not load what changed', { error, pageId: page.id })
      toast.danger(t('pages.conflict.title'), { description: t('pages.conflict.loadFailed') })
    }
  }

  async function publish() {
    if (!parsed.success) {
      console.debug('PageEditor kept the form: its values are invalid')
      return false
    }
    try {
      const response = await publishPage(page.id, {
        ...parsed.data,
        baseVersion: form.getValues('baseVersion'),
      })
      dispatch(pageLoaded(response.page))
      forgetDraft()
      toast.success(t(response.outcome === 'published'
        ? 'pages.editor.published'
        : 'pages.editor.unchanged'))
      void navigate(getKnowledgePageUrl(page.id), { replace: true })
      return true
    }
    catch (error) {
      if (getApiErrorCode(error) === 'page_changed') {
        await showConflict()
        return false
      }
      console.debug('PageEditor could not publish', { error, pageId: page.id })
      toast.danger(t('pages.editor.publishFailed'), { description: describePublishRefusal(error, t) ?? undefined })
      return false
    }
  }

  const parents = [
    { label: t('title'), href: UrlTree.knowledge },
    ...findPageAncestors(tree, page.id).map((ancestor) => ({
      label: ancestor.title,
      href: getKnowledgePageUrl(ancestor.id),
    })),
    { label: page.title, href: getKnowledgePageUrl(page.id) },
  ]

  return <Page
    title={title}
    crumb={t('pages.edit')}
    parents={parents}
    // A fixed page is one Elysium relies on, which keeps its name.
    rename={page.fixed
      ? undefined
      : {
        label: t('pages.editor.title'),
        maxLength: PAGE_TITLE_MAX_CHARACTERS,
        onChange: (value) => form.setValue('title', value, { shouldDirty: true }),
      }}
    save={{
      save: publish,
      isDirty: form.formState.isDirty,
      blockedReason: parsed.success
        ? null
        : parsed.error.issues[0].message,
      label: t('pages.publish'),
      onDiscard: forgetDraft,
    }}
    // Once they have seen what changed, they can look again while they work it in.
    actions={conflict
      ? [{
        id: 'whatChanged',
        label: t('pages.conflict.seeAgain'),
        icon: LuGitCompareArrows,
        onAction: () => setIsConflictOpen(true),
      }]
      : undefined}
  >
    <div className='flex max-w-4xl flex-col gap-4'>
      <MarkdownEditor
        id='page-body'
        label={t('pages.editor.body')}
        placeholder={t('pages.editor.placeholder')}
        markdown={body}
        onChange={(value) => form.setValue('body', value, { shouldDirty: true })}
        isFocused
      />
      {isLearning && <LearningEditFields
        category={values.category ?? ''}
        evidence={evidence}
        onCategoryChange={(value) => form.setValue('category', value, { shouldDirty: true })}
        onEvidenceChange={(value) => form.setValue('evidence', value, { shouldDirty: true })}
      />}
    </div>

    {conflict && <PageConflictDialog
      conflict={conflict}
      draft={{ title, body, evidence }}
      isOpen={isConflictOpen}
      onKeepEditing={() => {
        setIsConflictOpen(false)
        // They have seen it now, so the next publish knowingly goes on top of it.
        form.setValue('baseVersion', conflict.latest.currentVersion)
      }}
    />}
  </Page>
}

type LoaderProps = {
  pageId: string
}

function PageEditorLoader(props: LoaderProps) {
  const { t } = useTranslation('knowledge')
  const status = usePageLoader(props.pageId)
  const page = useAppSelector((state) => selectOpenedPage(state, props.pageId))

  if (page?.deletedAt) {
    console.debug('EditKnowledgePage went back to a deleted page, which changes no further', { pageId: props.pageId })
    return <Navigate to={getKnowledgePageUrl(props.pageId)} replace />
  }
  if (page) {
    return <PageEditor page={page} />
  }

  return <Page
    title={status === 'failed'
      ? t('pages.unavailableTitle')
      : t('pages.loading')}
    parents={[{ label: t('title'), href: UrlTree.knowledge }]}
  >
    {status === 'failed'
      ? <div className='py-10 text-center text-sm'>
        <p className='compact opacity-70'>{t('pages.unavailable')}</p>
        <Link href={UrlTree.knowledge} className='text-link'>{t('pages.backToKnowledge')}</Link>
      </div>
      : <div className='grid place-items-center py-16'>
        <Spinner />
      </div>}
  </Page>
}

// `/knowledge/pages/:pageId/edit`: one page in the rich editor, published from the header. Its own
// address, so a reload comes back here, and a person reading never loads the editor. Keyed by the
// page, so nothing of one carries to the next.
export function EditKnowledgePage() {
  const { pageId = '' } = useParams()

  return <PageEditorLoader key={pageId} pageId={pageId} />
}
