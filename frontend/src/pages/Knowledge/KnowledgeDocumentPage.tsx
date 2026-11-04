// Copyright © 2026 Jalapeno Labs

// Core
import { useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { useParams, useSearchParams } from 'react-router'
import useSWR from 'swr'

// Redux
import { selectIsCompanyMember } from '../../store/authSlice'
import { useAppSelector } from '../../store/hooks'

// User interface
import { Chip, Link, Spinner } from '@heroui/react'
import { EmptyNotice } from '../../components/EmptyNotice'
import { Page } from '../../layout/page/Page'
import { KnowledgeOriginLink } from './KnowledgeOriginLink'

// Utility
import { HTTPError } from 'ky'

// Misc
import { getKnowledgeDocument } from '../../api/routes/knowledgeRoutes'
import { DateText } from '../../format/DateText'
import { listFormat } from '../../format/intl'
import { getProjectViewUrl, KNOWLEDGE_PASSAGE_PARAM, UrlTree } from '../../urls'
import { presentKnowledgeSource, withoutTitle } from './knowledgePresentation'

// `/knowledge/documents/:documentId`: one document of the knowledge base whole, as every
// citation opens it, whoever's record it came from (docs/knowledge.md, Who sees what). It says
// where the text came from, whose record it is, when it was said, and its projects, then its
// passages in order, scrolled to the one `?passage=` names. A record's recording and clips stay
// with its owner, so the page holds the text alone, with a link out to where the source lives.
export function KnowledgeDocumentPage() {
  const { t } = useTranslation([ 'knowledge', 'common' ])
  const { documentId = '' } = useParams()
  const [ params ] = useSearchParams()
  const isCompanyMember = useAppSelector(selectIsCompanyMember)
  // A one-off read no other view shares, so SWR alone holds it (docs/frontend.md, Data).
  const { data, error } = useSWR(
    isCompanyMember
      ? [ 'knowledge-document', documentId ]
      : null,
    () => getKnowledgeDocument(documentId),
  )
  const document = data?.document
  const citedPosition = Number(params.get(KNOWLEDGE_PASSAGE_PARAM))
  const hasCitedPassage = params.has(KNOWLEDGE_PASSAGE_PARAM) && Number.isInteger(citedPosition)

  // Once the text is there, the passage the citation named is brought into view.
  useEffect(() => {
    if (!document || !hasCitedPassage) {
      return
    }
    window.document
      .getElementById(`passage-${citedPosition}`)
      ?.scrollIntoView({ block: 'center' })
  }, [ document, hasCitedPassage, citedPosition ])

  const parents = [{ label: t('title'), href: UrlTree.knowledge }]

  if (!isCompanyMember) {
    return <Page title={t('title')}>
      <EmptyNotice>{t('membersOnly')}</EmptyNotice>
    </Page>
  }

  if (!document) {
    if (error) {
      const isGone = error instanceof HTTPError && error.response.status === 404
      return <Page title={t('title')} parents={parents}>
        <div className='py-10 text-center text-sm'>
          <p className='compact opacity-70'>{
            isGone
              ? t('document.notFound')
              : t('document.loadError')
          }</p>
          <Link href={UrlTree.knowledge} className='text-link'>{t('document.backToSearch')}</Link>
        </div>
      </Page>
    }
    return <Page title={t('title')} parents={parents}>
      <div className='grid place-items-center py-16'>
        <Spinner />
      </div>
    </Page>
  }

  const { citation } = document
  const source = presentKnowledgeSource(citation.source, t)
  const SourceIcon = source.icon

  return <Page
    title={citation.title || source.label}
    parents={parents}
    icon={<SourceIcon className={`size-6 shrink-0 ${source.className}`} aria-hidden />}
    primaryAction={<KnowledgeOriginLink document={document} sourceLabel={source.label} />}
  >
    <div className='relaxed flex flex-wrap items-center gap-2 text-sm'>
      <Chip size='sm' variant='soft'>{source.label}</Chip>
      <span className='opacity-70'>{t('document.recordedBy', { name: citation.person.name })}</span>
      <span aria-hidden className='opacity-70'>·</span>
      <DateText value={citation.occurredAt} className='opacity-70' />
      {document.projects.length > 0 && <span aria-hidden className='opacity-70'>·</span>}
      {document.projects.map((project) => <Link
        key={project.id}
        href={getProjectViewUrl(project.id)}
        className='text-link'
      >{
          project.name
        }</Link>)}
    </div>
    {document.alsoRecordedBy.length > 0 && <p className='relaxed text-sm opacity-70'>{
      t('result.alsoRecordedBy', {
        names: listFormat({ type: 'conjunction' }).format(
          document.alsoRecordedBy.map((copy) => copy.person.name),
        ),
      })
    }</p>}

    <section aria-label={t('document.passages')} className='max-w-3xl'>{
      document.passages.map((passage) => <p
        key={passage.position}
        id={`passage-${passage.position}`}
        className={[
          'relaxed rounded-lg px-3 py-2 text-sm whitespace-pre-line',
          hasCitedPassage && passage.position === citedPosition
            ? 'bg-accent-soft'
            : '',
        ].join(' ')}
      >{
          withoutTitle(passage, citation.title)
        }</p>)
    }</section>
  </Page>
}
