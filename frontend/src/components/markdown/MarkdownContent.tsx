// Copyright © 2026 Jalapeno Labs

import type { ComponentProps } from 'react'
import type { Components, ExtraProps, Options } from 'react-markdown'

// Core
import { memo } from 'react'

// User interface
import ReactMarkdown from 'react-markdown'
import { MarkdownCodeBlock } from './MarkdownCodeBlock'
import { MarkdownImage } from './MarkdownImage'
import { MarkdownLink } from './MarkdownLink'

// Utility
import rehypeHighlight from 'rehype-highlight'
import remarkGfm from 'remark-gfm'

export type MarkdownProps = {
  // What a page says, as stored: Markdown, never trusted, since agents write pages too.
  markdown: string
  className?: string
}

const remarkPlugins: Options['remarkPlugins'] = [ remarkGfm ]

// highlight.js, through lowlight, marks tokens with class names alone: no inline styles and no
// evaluated code, unlike the highlighters that need WebAssembly, so it runs under the Content
// Security Policy and takes its colors from the theme (`src/theme/markdown.css`). It highlights
// only a block that names its language, among highlight.js's common ones; any other stays plain.
const rehypePlugins: Options['rehypePlugins'] = [ rehypeHighlight ]

// Scrolls on its own, so a wide table never widens the page.
function MarkdownTable(props: ComponentProps<'table'> & ExtraProps) {
  return <div className='markdown-table'>
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
// are the costly part, and a re-render of the page around it changes neither.
export const MarkdownContent = memo(function MarkdownContent(props: MarkdownProps) {
  return <div className={`markdown ${props.className ?? ''}`}>
    <ReactMarkdown
      remarkPlugins={remarkPlugins}
      rehypePlugins={rehypePlugins}
      components={components}
    >{
        props.markdown
      }</ReactMarkdown>
  </div>
})
