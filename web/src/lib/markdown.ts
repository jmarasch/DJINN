// A small Markdown renderer for the user guide: headings, paragraphs, lists
// (nested by indenting), tables, fenced code, quotes, and inline code, bold,
// italics, links and pictures. Any HTML in the text is shown as text, never
// run.

export interface MarkdownOptions {
  /** Where a picture's address points (the guide's `images/...`). */
  image?: (src: string) => string
}

const escapes: Record<string, string> = { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }
const escape = (s: string) => s.replace(/[&<>"']/g, (c) => escapes[c])

/** A heading's anchor, the way GitHub makes them. */
export function slug(text: string): string {
  return text
    .trim()
    .toLowerCase()
    .replace(/[^\p{L}\p{N}\- ]/gu, '')
    .replace(/ /g, '-')
}

function safeUrl(url: string): string {
  const trimmed = url.trim()
  return /^(javascript|data|vbscript):/i.test(trimmed) ? '#' : trimmed
}

function inline(text: string, options: MarkdownOptions): string {
  const codes: string[] = []
  let out = text.replace(/`([^`]+)`/g, (_, code: string) => {
    codes.push(`<code>${escape(code)}</code>`)
    return `\u0000${codes.length - 1}\u0000`
  })
  out = escape(out)
  out = out.replace(/!\[([^\]]*)\]\(([^)\s]+)\)/g, (_, alt: string, src: string) => {
    const url = options.image ? options.image(src) : src
    return `<img src="${escape(safeUrl(url))}" alt="${alt}" loading="lazy">`
  })
  out = out.replace(/\[([^\]]+)\]\(([^)\s]+)\)/g, (_, label: string, href: string) => {
    const external = /^https?:/i.test(href)
    return `<a href="${escape(safeUrl(href))}"${external ? ' target="_blank" rel="noopener noreferrer"' : ''}>${label}</a>`
  })
  out = out.replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>')
  out = out.replace(/(^|[^*\w])\*([^*\s][^*]*)\*(?!\w)/g, '$1<em>$2</em>')
  return out.replace(/\u0000(\d+)\u0000/g, (_, i: string) => codes[Number(i)])
}

interface ListItem {
  indent: number
  ordered: boolean
  text: string
}

function renderList(items: ListItem[], options: MarkdownOptions): string {
  let html = ''
  let i = 0
  const level = (base: number): string => {
    const ordered = items[i].ordered
    let out = ordered ? '<ol>' : '<ul>'
    while (i < items.length && items[i].indent >= base) {
      if (items[i].indent > base) {
        out = out.replace(/<\/li>$/, '') + level(items[i].indent) + '</li>'
        continue
      }
      out += `<li>${inline(items[i].text, options)}</li>`
      i++
    }
    return out + (ordered ? '</ol>' : '</ul>')
  }
  while (i < items.length) html += level(items[i].indent)
  return html
}

function renderTable(rows: string[], options: MarkdownOptions): string {
  const cells = (row: string) =>
    row
      .trim()
      .replace(/^\||\|$/g, '')
      .split('|')
      .map((c) => c.trim())
  const [head, , ...body] = rows
  return (
    '<div class="table"><table><thead><tr>' +
    cells(head).map((c) => `<th>${inline(c, options)}</th>`).join('') +
    '</tr></thead><tbody>' +
    body.map((r) => '<tr>' + cells(r).map((c) => `<td>${inline(c, options)}</td>`).join('') + '</tr>').join('') +
    '</tbody></table></div>'
  )
}

/** The page as HTML. */
export function markdown(source: string, options: MarkdownOptions = {}): string {
  const lines = source.replace(/\r\n?/g, '\n').split('\n')
  const out: string[] = []
  let i = 0
  const listLine = /^(\s*)([-*]|\d+\.)\s+(.*)$/
  while (i < lines.length) {
    const line = lines[i]
    if (!line.trim()) {
      i++
      continue
    }
    const fence = line.match(/^\s*```/)
    if (fence) {
      const code: string[] = []
      i++
      while (i < lines.length && !/^\s*```/.test(lines[i])) code.push(lines[i++])
      i++
      out.push(`<pre><code>${escape(code.join('\n'))}</code></pre>`)
      continue
    }
    const heading = line.match(/^(#{1,6})\s+(.*)$/)
    if (heading) {
      const level = heading[1].length
      out.push(`<h${level} id="${escape(slug(heading[2]))}">${inline(heading[2], options)}</h${level}>`)
      i++
      continue
    }
    if (/^\s*\|/.test(line) && i + 1 < lines.length && /^\s*\|?\s*:?-+/.test(lines[i + 1])) {
      const rows: string[] = []
      while (i < lines.length && /^\s*\|/.test(lines[i])) rows.push(lines[i++])
      out.push(renderTable(rows, options))
      continue
    }
    if (listLine.test(line)) {
      const items: ListItem[] = []
      while (i < lines.length) {
        const m = lines[i].match(listLine)
        if (m) {
          items.push({ indent: m[1].length, ordered: /\d/.test(m[2]), text: m[3] })
          i++
        } else if (lines[i].trim() && /^\s+/.test(lines[i]) && items.length) {
          items[items.length - 1].text += ' ' + lines[i].trim()
          i++
        } else if (!lines[i].trim() && i + 1 < lines.length && /^\s+\S/.test(lines[i + 1]) && !/^\s*```/.test(lines[i + 1])) {
          i++
        } else break
      }
      out.push(renderList(items, options))
      continue
    }
    if (/^>\s?/.test(line)) {
      const quote: string[] = []
      while (i < lines.length && /^>\s?/.test(lines[i])) quote.push(lines[i++].replace(/^>\s?/, ''))
      out.push(`<blockquote>${inline(quote.join(' '), options)}</blockquote>`)
      continue
    }
    const para: string[] = []
    while (
      i < lines.length &&
      lines[i].trim() &&
      !/^(#{1,6}\s|\s*```|>\s?)/.test(lines[i]) &&
      !listLine.test(lines[i]) &&
      !/^\s*\|/.test(lines[i])
    )
      para.push(lines[i++].trim())
    out.push(`<p>${inline(para.join(' '), options)}</p>`)
  }
  return out.join('\n')
}
