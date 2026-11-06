// Copyright © 2026 Jalapeno Labs

import type { ComponentProps } from 'react'
import type { Components, ExtraProps, Options } from 'react-markdown'
import type { MarkdownRange } from './rehypeMarkPassage'

// Core
import { memo, useEffect, useMemo, useRef } from 'react'

// User interface
import ReactMarkdown from 'react-markdown'
import { MarkdownCodeBlock } from './MarkdownCodeBlock'
import { MarkdownImage } from './MarkdownImage'
import { MarkdownLink } from './MarkdownLink'

// Utility
import rehypeHighlight from 'rehype-highlight'
import remarkGfm from 'remark-gfm'

// Misc
import { MARKED_PASSAGE_CLASS, rehypeMarkPassage } from './rehypeMarkPassage'

export type MarkdownProps = {
  // What a page says, as stored: Markdown, never trusted, since agents write pages too.
  markdown: string
  className?: string
  // A stretch of `markdown` to mark and bring into view, once: a key on this component starts over.
  markedRange?: MarkdownRange | null
}

const remarkPlugins: Options['remarkPlugins'] = [ remarkGfm ]

// highlight.js, through lowlight, marks tokens with class names alone: no inline styles and no
// evaluated code, unlike the highlighters that need WebAssembly, so it runs under the Content
// Security Policy and takes its colors from the theme (`src/theme/markdown.css`). It highlights
// only a block that names its language, among highlight.js's common ones; any other stays plain.
const rehypePlugins: Options['rehypePlugins'] = [ rehypeHighlight ]

// Scrolls on its own, so a wide table never widens the page.
function MarkdownTable(props: ComponentProps<'table'> & ExtraProps) {
  return <div className={`markdown-table ${props.className ?? ''}`}>
    <table>{props.children}</table>
  </div>
}

// A heading steps down one level, since the page's own title is the one `h1`.
const components: Components = {
  h1: 'h2',
  h2: 'h3',
  h3: 'h4',
  h4: 'h5',
  h5: 'h6',
  a: MarkdownLink,
  img: MarkdownImage,
  pre: MarkdownCodeBlock,
  table: MarkdownTable,
}

// Reads Markdown to React elements, so every page, editor preview, and version shows it alike
// (docs/frontend.md, Markdown). Nothing is set as HTML: raw HTML in the text stays visible text,
// so `<script>` is shown rather than run, and react-markdown's default `urlTransform` blanks any
// address but http, https, mailto, and relative ones. Memoized because parsing and highlighting
// are the costly part, and a re-render of the page around it changes neither. A marked range is
// drawn from the syntax tree (`rehypeMarkPassage`); when nothing it reaches is drawn, such as a
// footnote, the text simply stays where it is.
export const MarkdownContent = memo(function MarkdownContent(props: MarkdownProps) {
  const { markedRange } = props
  const containerRef = useRef<HTMLDivElement>(null)
  const plugins = useMemo<Options['rehypePlugins']>(
    () => markedRange
      ? [ ...(rehypePlugins ?? []), [ rehypeMarkPassage, { range: markedRange }]]
      : rehypePlugins,
    [ markedRange ],
  )

  // Once, as the range first arrives: a page republished while it is read moves the range, and must
  // not pull the reader back from wherever they scrolled.
  const hasMarkedRange = Boolean(markedRange)
  useEffect(() => {
    if (!hasMarkedRange) {
      return undefined
    }
    // A frame later, since a parent's effect in this same commit may put the area back at its top
    // for a new address (`KnowledgeFrame`), and a parent's effects run after a child's.
    const frame = window.requestAnimationFrame(() => {
      const marked = containerRef.current?.querySelector(`.${MARKED_PASSAGE_CLASS}`)
      if (!marked) {
        console.debug('MarkdownContent drew nothing for its marked range, so the text stays at its top')
        return
      }
      marked.scrollIntoView({ block: 'start' })
    })
    return () => window.cancelAnimationFrame(frame)
  }, [ hasMarkedRange ])

  return <div ref={containerRef} className={`markdown ${props.className ?? ''}`}>
    <ReactMarkdown
      remarkPlugins={remarkPlugins}
      rehypePlugins={plugins}
      components={components}
    >{
        props.markdown
      }</ReactMarkdown>
  </div>
})
