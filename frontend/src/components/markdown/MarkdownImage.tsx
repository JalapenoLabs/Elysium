// Copyright © 2026 Jalapeno Labs

import type { ComponentProps } from 'react'
import type { ExtraProps } from 'react-markdown'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Link } from '@heroui/react'
import { LuExternalLink, LuImageOff } from 'react-icons/lu'

// Misc
import { readMarkdownImage } from './markdownUrls'

type Props = ComponentProps<'img'> & ExtraProps

// Sits inside a paragraph, so it is a span, and it wraps, so a long name fits a phone.
const NOTICE_CLASS_NAME = 'markdown-image-notice'

// An image in a page's text. Elysium's own loads. The Content Security Policy keeps every other
// origin's images out, which also keeps a page from telling a third party who read it, so one
// never tries to load: it is a link that opens the image in a new tab. An image that cannot load
// says so with its description, so no one reads past a blank space.
export function MarkdownImage(props: Props) {
  const { t } = useTranslation('common')
  const [ hasFailed, setHasFailed ] = useState(false)
  const source = readMarkdownImage(props.src)

  if (source.kind === 'remote') {
    return <span className={NOTICE_CLASS_NAME}>
      <LuExternalLink className='size-4 shrink-0' aria-hidden />
      <Link href={source.href} target='_blank' rel='noreferrer'>{
        t('markdown.remoteImage', { name: props.alt || source.host })
      }</Link>
    </span>
  }

  if (source.kind === 'none' || hasFailed) {
    return <span className={NOTICE_CLASS_NAME}>
      <LuImageOff className='size-4 shrink-0' aria-hidden />
      <span>{
        props.alt
          ? t('markdown.imageUnavailableNamed', { name: props.alt })
          : t('markdown.imageUnavailable')
      }</span>
    </span>
  }

  return <img
    src={source.src}
    alt={props.alt}
    title={props.title}
    loading='lazy'
    onError={() => setHasFailed(true)}
  />
}
