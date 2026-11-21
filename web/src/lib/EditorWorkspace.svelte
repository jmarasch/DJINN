<script lang="ts" module>
  type MoveKind = 'rename' | 'move' | 'copy' | 'delete' | 'mp3'
</script>

<script lang="ts">
  // The library editor's workspace: the folders as they are on disk (left),
  // the songs of the chosen folder in a grid (middle), and the album's
  // cover, the changes waiting for approval and the history (right). The
  // toggle at the top says what is being edited: the library (the tags in
  // the files, only in folders allowed) or the database (DJINN's own copy,
  // the files left alone). Nothing changes until Approve: edited cells are
  // marked, and the list of changes shows what will happen.
  import { locale, t } from './i18n.svelte'
  import { onMount } from 'svelte'
  import { ApiError, api } from './api'
  import AlbumLayout from './AlbumLayout.svelte'
  import Art from './Art.svelte'
  import CoverWindow from './CoverWindow.svelte'
  import { COMMON_TAGS, MORE_TAGS, NUMBER_TAGS, tagLabel } from './editorTags'
  import { autoMatch } from './autoMatch'
  import { FOLDER_LAYOUTS, albumBase, folderInfo, fromFileName, type FolderInfo, type FolderLayout } from './importLayout'
  import { clearListCache } from './listCache'
  import { askToUnlock, unlocked } from './protected.svelte'
  import { toast } from './state.svelte'
  import type {
    EditorBatch,
    EditorChange,
    EditorCoreField,
    EditorField,
    EditorImport,
    EditorNode,
    EditorRenamed,
    EditorSong,
    EditTarget,
  } from './types'

  let {
    startFolder = '',
    startSearch = '',
    windowed = false,
  }: {
    startFolder?: string
    startSearch?: string
    /** In the editor's own window: fills the whole height, with room for each pane. */
    windowed?: boolean
  } = $props()

  let target = $state<EditTarget>('database')
  /** Import mode: albums brought in to work on before they go into the
   * library, listed on the left instead of the library's folders. */
  let importMode = $state(false)
  /** The import being worked on (its number). */
  let importing = $state<number | null>(null)
  /** How a folder being brought in is laid out, and whether its folder
   * names fill tags the files are missing. */
  let folderLayout = $state<FolderLayout>('album')
  let fillFromFolders = $state(true)
  /** The grid edits files: the library's, or an import's (waiting, not in the library yet). */
  const editingFiles = $derived(target === 'library' || importMode)
  let imports = $state<EditorImport[]>([])
  /** Files on their way in: done of all, in how many imports. */
  let uploading = $state<{ done: number; of: number; imports: number } | null>(null)
  let importInput = $state<HTMLInputElement | null>(null)
  let importFolderInput = $state<HTMLInputElement | null>(null)
  /** Where Add to the library puts an import's files. */
  let addFolder = $state('')
  let addPattern = $state('{album_artist}/{album}/{track} {title}')
  let addPlan = $state<EditorRenamed[] | null>(null)
  let roots = $state<EditorNode[]>([])
  let children = $state<Record<string, EditorNode[]>>({})
  let open = $state<Record<string, boolean>>({})
  let selected = $state('')
  /** Words being looked for across the library (instead of a folder). */
  let searching = $state('')
  let searchText = $state('')
  let songs = $state<EditorSong[]>([])
  let total = $state(0)
  let locked = $state(false)
  let loading = $state(false)
  let busy = $state(false)
  let chosen = $state<EditorSong | null>(null)

  /** Changes waiting for approval, by song and field ('' clears the field). */
  let pending = $state<Record<number, Partial<Record<EditorField, string>>>>({})
  /** The cell being typed in. */
  let editing = $state<{ id: number; field: EditorField } | null>(null)
  let draft = $state('')
  /** Rows picked for the tools below the grid. */
  let picked = $state<Record<number, boolean>>({})
  let lastPicked = $state<number | null>(null)
  let fillField = $state<EditorField>('album_artist')
  let fillValue = $state('')
  let pattern = $state('{track} {title}')
  let renames = $state<EditorRenamed[] | null>(null)
  /** What the list of new places is for. */
  let moveKind = $state<MoveKind>('rename')
  /** After moving: folders left with no songs go to the trash; new folders get folder.jpg. */
  let removeEmpty = $state(true)
  let folderArt = $state(false)
  let moveTo = $state('')
  let history = $state<EditorBatch[]>([])
  let coverInput = $state<HTMLInputElement | null>(null)
  let replaceInput = $state<HTMLInputElement | null>(null)
  /** The songs open in the album layout, if it's open. */
  let layoutSongs = $state<EditorSong[] | null>(null)
  let layoutBox = $state<HTMLElement | null>(null)

  const FIELDS: { key: EditorCoreField; label: string; narrow?: boolean }[] = [
    { key: 'disc_no', get label() { return t('ed.disc') }, narrow: true },
    { key: 'track_no', label: '#', narrow: true },
    { key: 'title', get label() { return t('ed.title') } },
    { key: 'artist', get label() { return t('ed.artist') } },
    { key: 'album', get label() { return t('ed.album') } },
    { key: 'album_artist', get label() { return t('ed.album_artist') } },
    { key: 'year', get label() { return t('ed.year') }, narrow: true },
    { key: 'genre', get label() { return t('ed.genre') } },
  ]
  /** A field's name in lists: its column's, or the other tag's. */
  const labelOf = (field: EditorField) =>
    field.startsWith('x:')
      ? tagLabel(field.slice(2))
      : field === 'track_no'
        ? t('ed.track')
        : (FIELDS.find((f) => f.key === field)?.label ?? field)

  // --- Columns: the grid's own (all but Title can be taken off) and the
  // file's other tags added as columns. Kept per browser.
  const COLUMNS_KEY = 'djinn.editor.columns'
  type Columns = { hidden: string[]; extra: string[] }
  type Column = { key: EditorField; label: string; narrow?: boolean; number?: boolean }

  function loadColumns(): Columns {
    try {
      const saved = JSON.parse(localStorage.getItem(COLUMNS_KEY) ?? '{}')
      const strings = (v: unknown) => (Array.isArray(v) ? v.filter((x): x is string => typeof x === 'string') : [])
      return { hidden: strings(saved.hidden), extra: strings(saved.extra) }
    } catch {
      return { hidden: [], extra: [] }
    }
  }

  let columns = $state<Columns>(loadColumns())
  $effect(() => {
    try {
      localStorage.setItem(COLUMNS_KEY, JSON.stringify(columns))
    } catch {
      // Only remembered when the browser keeps it.
    }
  })

  /** Other tags are in the files only, so their columns show on the Library side. */
  const shownColumns = $derived<Column[]>([
    ...FIELDS.filter((f) => !columns.hidden.includes(f.key)).map((f) => ({ ...f, number: f.narrow })),
    ...(editingFiles
      ? columns.extra.map((name) => ({
          key: `x:${name}` as EditorField,
          label: tagLabel(name),
          number: NUMBER_TAGS.has(name),
        }))
      : []),
  ])

  let addPick = $state('')
  /** Other tags the shown files carry that have no column yet. */
  const inFiles = $derived(
    [...new Set(songs.flatMap((s) => Object.keys(s.file?.extra ?? {})))]
      .filter((n) => !columns.extra.includes(n))
      .sort((a, b) => tagLabel(a).localeCompare(tagLabel(b))),
  )
  const notShown = (list: [string, string][]) => list.filter(([n]) => !columns.extra.includes(n) && !inFiles.includes(n))

  function addColumn() {
    if (!addPick) return
    if (addPick.startsWith('x:')) {
      const name = addPick.slice(2)
      if (!columns.extra.includes(name)) columns = { ...columns, extra: [...columns.extra, name] }
    } else {
      columns = { ...columns, hidden: columns.hidden.filter((h) => h !== addPick) }
    }
    addPick = ''
  }

  function removeColumn(field: EditorField) {
    if (field === 'title') return
    if (field.startsWith('x:')) columns = { ...columns, extra: columns.extra.filter((n) => n !== field.slice(2)) }
    else columns = { ...columns, hidden: [...columns.hidden, field] }
    if (sortBy?.field === field) sortBy = null
    if (fillField === field) fillField = 'title'
  }

  // --- Sorting and grouping. Rows sort and group by the values shown
  // (including changes waiting), or folder by folder, by disc and track.
  type SortField = EditorField | 'file'
  let sortBy = $state<{ field: SortField; desc: boolean } | null>(null)
  let groupBy = $state<'' | 'disc_no' | 'album' | 'folder'>('')
  const GROUPS: { key: typeof groupBy; label: string }[] = [
    { key: '', get label() { return t('ed.nothing') } },
    { key: 'disc_no', get label() { return t('ed.disc') } },
    { key: 'album', get label() { return t('ed.album') } },
    { key: 'folder', get label() { return t('ed.folder') } },
  ]
  const collator = new Intl.Collator(undefined, { numeric: true, sensitivity: 'base' })
  const folderOf = (song: EditorSong) => {
    const rel = song.rel_path || song.path
    // A name with no folder in it (an import's) is in no folder, not cut short.
    const cut = Math.max(rel.lastIndexOf('/'), rel.lastIndexOf('\\'))
    return cut < 0 ? '' : rel.slice(0, cut)
  }

  function sortOn(field: SortField) {
    if (editing) {
      const song = songs.find((s) => s.id === editing?.id)
      if (song) finishEdit(song, true)
    }
    if (sortBy?.field !== field) sortBy = { field, desc: false }
    else if (!sortBy.desc) sortBy = { field, desc: true }
    else sortBy = null
  }

  /** Two values in order, empty ones last whichever way it sorts. */
  function compareValues(a: string, b: string, number: boolean, desc = false): number {
    if (!a || !b) return a === b ? 0 : a ? -1 : 1
    const c = number && !Number.isNaN(Number(a)) && !Number.isNaN(Number(b)) ? Number(a) - Number(b) : collator.compare(a, b)
    return desc ? -c : c
  }

  const isNumber = (field: SortField) =>
    field === 'track_no' || field === 'disc_no' || field === 'year' || (field.startsWith('x:') && NUMBER_TAGS.has(field.slice(2)))

  function fieldOrder(a: EditorSong, b: EditorSong, field: SortField, desc = false): number {
    if (field === 'file') return compareValues(fileName(a.path), fileName(b.path), false, desc)
    return compareValues(shown(a, field), shown(b, field), isNumber(field), desc)
  }

  function groupKey(song: EditorSong): string {
    if (groupBy === 'disc_no') return shown(song, 'disc_no') || '1'
    if (groupBy === 'album') return shown(song, 'album')
    if (groupBy === 'folder') return folderOf(song)
    return ''
  }

  /** The songs in the grid's order. */
  const view = $derived.by(() => {
    const list = [...songs]
    list.sort((a, b) => {
      if (groupBy) {
        const g = compareValues(groupKey(a), groupKey(b), groupBy === 'disc_no')
        if (g) return g
      }
      if (sortBy) {
        const c = fieldOrder(a, b, sortBy.field, sortBy.desc)
        if (c) return c
      }
      return (
        collator.compare(folderOf(a), folderOf(b)) ||
        fieldOrder(a, b, 'disc_no') ||
        fieldOrder(a, b, 'track_no') ||
        fieldOrder(a, b, 'file')
      )
    })
    return list
  })
  const position = $derived(new Map(view.map((s, i) => [s.id, i])))

  const groups = $derived.by(() => {
    if (!groupBy) return [{ key: '', label: '', songs: view }]
    const out: { key: string; label: string; songs: EditorSong[] }[] = []
    for (const song of view) {
      const key = groupKey(song)
      const last = out[out.length - 1]
      if (last && last.key === key) last.songs.push(song)
      else
        out.push({
          key,
          label:
            groupBy === 'disc_no'
              ? t('ed.disc_n', { n: key })
              : groupBy === 'album'
                ? key || t('ed.no_album')
                : key || t('ed.library_folder'),
          songs: [song],
        })
    }
    return out
  })

  let coverOpen = $state(false)

  const n = (x: number) => x.toLocaleString(locale.code)

  async function loadRoots() {
    try {
      roots = (await api.editorTree('')).folders
      locked = false
    } catch (e) {
      if (e instanceof ApiError && e.status === 423) locked = true
      else toast(e instanceof Error ? e.message : t('ed.load_failed'), true)
    }
  }

  /** Reloads the folder tree on the left (and opens the folders leading to `openTo`). */
  async function refreshTree(openTo = '') {
    await loadRoots()
    const nextOpen = { ...open }
    if (openTo) {
      const root = roots.find(
        (r) => openTo === r.path || openTo.startsWith(`${r.path}/`) || openTo.startsWith(`${r.path}\\`),
      )
      if (root) {
        const sep = openTo.includes('\\') && !openTo.includes('/') ? '\\' : '/'
        const rest = openTo
          .slice(root.path.length)
          .replace(/^[\\/]+/, '')
          .split(/[\\/]/)
          .filter(Boolean)
        let cur = root.path
        if (rest.length) nextOpen[cur] = true
        for (let i = 0; i < rest.length - 1; i++) {
          cur = `${cur}${sep}${rest[i]}`
          nextOpen[cur] = true
        }
      }
    }
    const nextChildren: Record<string, EditorNode[]> = {}
    for (const path of Object.keys(nextOpen).filter((p) => nextOpen[p])) {
      try {
        nextChildren[path] = (await api.editorTree(path)).folders
      } catch {
        delete nextOpen[path]
      }
    }
    children = nextChildren
    open = nextOpen
  }

  async function toggle(node: EditorNode) {
    if (open[node.path]) {
      open = { ...open, [node.path]: false }
      return
    }
    if (!children[node.path]) {
      try {
        children = { ...children, [node.path]: (await api.editorTree(node.path)).folders }
      } catch (e) {
        toast(e instanceof Error ? e.message : t('ed.open_failed'), true)
        return
      }
    }
    open = { ...open, [node.path]: true }
  }

  const changeCount = $derived(Object.values(pending).reduce((sum, f) => sum + Object.keys(f).length, 0))

  function leaveChanges(): boolean {
    return changeCount === 0 || confirm(t('ed.throw_q', { n: changeCount }))
  }

  async function choose(node: EditorNode) {
    if (!leaveChanges()) return
    pending = {}
    searching = ''
    importing = null
    importMode = false
    addPlan = null
    selected = node.path
    noLyrics = []
    moveTo = node.path
    chosen = null
    picked = {}
    renames = null
    await loadSongs(0)
  }

  async function find(words: string) {
    const clean = words.trim()
    if (!clean || !leaveChanges()) return
    pending = {}
    selected = ''
    importing = null
    importMode = false
    addPlan = null
    searching = clean
    noLyrics = []
    searchText = clean
    chosen = null
    picked = {}
    renames = null
    await loadSongs(0)
  }

  async function loadSongs(offset: number) {
    if (!selected && !searching && importing === null) return
    loading = true
    try {
      const found =
        importing !== null
          ? await api.editorImportSongs(importing)
          : await api.editorSongs(selected, offset, target === 'library', searching)
      songs = offset === 0 ? found.songs : [...songs, ...found.songs]
      total = found.total
      if (!chosen || !songs.some((s) => s.id === chosen?.id)) chosen = songs[0] ?? null
      else chosen = songs.find((s) => s.id === chosen?.id) ?? null
    } catch (e) {
      toast(e instanceof Error ? e.message : t('ed.songs_failed'), true)
    } finally {
      loading = false
    }
  }

  async function loadHistory() {
    try {
      history = await api.editorHistory()
    } catch {
      history = []
    }
  }

  function setTarget(to: EditTarget) {
    if ((to === target && !importMode) || !leaveChanges()) return
    pending = {}
    if (importMode) {
      importMode = false
      importing = null
      addPlan = null
      songs = []
      total = 0
      chosen = null
    }
    if (fillField.startsWith('x:')) fillField = 'title'
    if (sortBy && sortBy.field.startsWith('x:')) sortBy = null
    renames = null
    target = to
    void loadSongs(0)
    if (to === 'library') void loadHistory()
  }

  async function unlock() {
    if (await askToUnlock()) await loadRoots()
  }

  // --- Imports: albums brought in to work on before they go into the library.

  let pickedImports = $state<Record<number, boolean>>({})
  let lastPickedImport = $state<number | null>(null)
  const pickedImportBatches = $derived(imports.filter((i) => pickedImports[i.batch]).map((i) => i.batch))
  const allImportsPicked = $derived(imports.length > 0 && pickedImportBatches.length === imports.length)

  function pickImport(batch: number, index: number, e: MouseEvent) {
    const on = !pickedImports[batch]
    const next = { ...pickedImports }
    if (e.shiftKey && lastPickedImport !== null) {
      const [a, b] = [Math.min(lastPickedImport, index), Math.max(lastPickedImport, index)]
      for (const imp of imports.slice(a, b + 1)) next[imp.batch] = true
    } else {
      next[batch] = on
    }
    pickedImports = next
    lastPickedImport = index
  }

  function pickAllImports(on: boolean) {
    pickedImports = on ? Object.fromEntries(imports.map((i) => [i.batch, true])) : {}
  }

  async function loadImports() {
    try {
      imports = await api.editorImports()
      const valid = new Set(imports.map((i) => i.batch))
      pickedImports = Object.fromEntries(Object.entries(pickedImports).filter(([b]) => valid.has(Number(b))))
    } catch {
      imports = []
      pickedImports = {}
    }
  }

  /** Import mode: the imports on the left; the newest opens. */
  async function enterImport() {
    if (importMode || !leaveChanges()) return
    pending = {}
    importMode = true
    importing = null
    selected = ''
    searching = ''
    songs = []
    total = 0
    chosen = null
    picked = {}
    renames = null
    await loadImports()
    if (imports.length) await openImport(imports[0].batch)
    void lookUpAll()
  }

  /** Each import's changes still waiting, kept while another is open. */
  const waitingFor: Record<number, typeof pending> = {}

  async function openImport(batch: number) {
    if (importing === batch) return
    if (importing !== null) waitingFor[importing] = pending
    else if (!leaveChanges()) return
    pending = waitingFor[batch] ?? {}
    delete waitingFor[batch]
    selected = ''
    searching = ''
    importMode = true
    importing = batch
    chosen = null
    noLyrics = []
    picked = {}
    renames = null
    addPlan = null
    autoNote = noteOf(batch)
    if (!addFolder) addFolder = roots.find((r) => r.editable)?.path ?? ''
    await loadSongs(0)
    if (importing === batch) void autoLookUp(batch, songs)
  }

  const AUDIO = /\.(mp3|flac|m4a|mp4|aac|ogg|oga|wav)$/i

  /** Brings song files in: chosen files as one import; a folder split
   * into one import per album as `folderLayout` says, its folder names
   * filling tags the files are missing when `fillFromFolders` is on. */
  async function importFiles(list: FileList | null, fromFolder: boolean) {
    const files = [...(list ?? [])].filter((f) => AUDIO.test(f.name))
    const left = (list?.length ?? 0) - files.length
    if (!files.length) {
      toast(t('ed.no_songs'), true)
      return
    }
    if (!leaveChanges()) return
    const layout: FolderLayout = fromFolder ? folderLayout : 'together'
    // In track order as far as names go (disc folders, then numbers).
    files.sort((a, b) => collator.compare(a.webkitRelativePath || a.name, b.webkitRelativePath || b.name))
    const groups = new Map<string, { file: File; info: FolderInfo }[]>()
    for (const file of files) {
      const info = folderInfo(file.webkitRelativePath || file.name, layout)
      const group = groups.get(info.group) ?? []
      group.push({ file, info })
      groups.set(info.group, group)
    }
    uploading = { done: 0, of: files.length, imports: groups.size }
    const problems: string[] = []
    const batches: number[] = []
    let filled = 0
    try {
      for (const group of groups.values()) {
        const batch = (await unlocked(() => api.editorImportStart())).batch
        const placed = new Map<number, FolderInfo>()
        for (const { file, info } of group) {
          try {
            const { id } = await unlocked(() => api.editorImportUpload(batch, file))
            placed.set(id, info)
          } catch (e) {
            problems.push(`${file.name}: ${e instanceof Error ? e.message : t('ed.upload_failed')}`)
          }
          uploading = { ...uploading, done: uploading.done + 1 }
        }
        if (!placed.size) continue
        batches.push(batch)
        if (fromFolder && fillFromFolders && layout !== 'together') {
          const rows = (await api.editorImportSongs(batch)).songs
          const changes: EditorChange[] = []
          for (const row of rows) {
            const info = placed.get(row.id)
            const tags = row.file
            if (!info || !tags) continue
            const fill = (field: EditorField, now: unknown, value: string | number | null) => {
              if (value !== null && value !== '' && (now === null || now === undefined || now === '')) {
                changes.push({ id: row.id, field, value: String(value) })
              }
            }
            // A file with no title tag shows its name as the title: take the
            // track number off the front of it (and keep it as the number).
            const named = fromFileName(row.rel_path)
            const stem = row.rel_path.replace(/\.[^.]+$/, '')
            if (tags.title === stem && named.track !== null) {
              changes.push({ id: row.id, field: 'title', value: named.title })
              fill('track_no', tags.track_no, named.track)
            }
            fill('album', tags.album, info.album)
            fill('artist', tags.artist, info.artist)
            fill('album_artist', tags.album_artist, info.artist)
            fill('disc_no', tags.disc_no, info.disc)
            fill('year', tags.year, info.year)
          }
          if (changes.length) {
            await unlocked(() => api.editorImportApply(changes))
            filled += changes.length
          }
        }
      }
    } catch (e) {
      toast(e instanceof Error ? e.message : t('ed.bring_failed'), true)
    }
    uploading = null
    await loadImports()
    const done = files.length - problems.length
    toast(
      t('ed.brought', { n: n(done) }) +
        (batches.length > 1 ? t('ed.as_imports', { n: batches.length }) : '') +
        (filled ? t('ed.filled', { n: filled }) : '') +
        (left ? t('ed.left_out', { n: n(left) }) : '') +
        (problems.length ? t('ed.couldnt', { n: problems.length, first: problems[0] }) : ''),
      problems.length > 0,
    )
    if (batches.length) {
      pending = {}
      importing = null
      await openImport(batches[0])
    }
    void lookUpAll()
  }

  async function throwAwayImports() {
    const batches = pickedImportBatches.length ? pickedImportBatches : importing !== null ? [importing] : []
    if (!batches.length) return
    const current = importing
    busy = true
    try {
      for (const batch of batches) {
        try {
          await unlocked(() => api.editorImportThrowAway(batch))
        } catch (e) {
          toast(e instanceof Error ? e.message : t('ed.throw_failed'), true)
        }
      }
      pickedImports = {}
      lastPickedImport = null
      await loadImports()
      if (current !== null && batches.includes(current)) {
        pending = {}
        importing = null
        songs = []
        total = 0
        chosen = null
        addPlan = null
        if (imports.length) await openImport(imports[0].batch)
      }
    } finally {
      busy = false
    }
  }

  /** Takes the picked songs out of the import (deleting the waiting copies). */
  async function dropPicked() {
    const list = pickedSongs
    if (!list.length || !confirm(t('ed.drop_q', { n: list.length }))) return
    for (const song of list) {
      try {
        await unlocked(() => api.editorImportDropFile(song.id))
      } catch (e) {
        toast(e instanceof Error ? e.message : t('ed.drop_failed'), true)
      }
    }
    picked = {}
    addPlan = null
    await loadImports()
    await loadSongs(0)
  }

  // --- Adding an import to the library. Where each song would go is worked
  // out as things change (folder, pattern, picked songs, waiting changes),
  // and Approve Import writes the waiting changes and moves the songs in,
  // in one step. The editor stays on Import, on the next album waiting.
  let previewTimer: ReturnType<typeof setTimeout> | undefined
  let previewing = $state(false)
  $effect(() => {
    const batch = importing
    const folder = addFolder
    const pattern = addPattern
    const ids = pickedSongs.map((s) => s.id)
    const changes = changeList()
    clearTimeout(previewTimer)
    if (batch === null || !folder || !pattern.includes('{')) {
      addPlan = null
      return
    }
    previewing = true
    previewTimer = setTimeout(async () => {
      try {
        const plan = (await unlocked(() => api.editorImportAdd(batch, folder, pattern, true, ids, changes))).files
        if (importing === batch) addPlan = plan
      } catch {
        if (importing === batch) addPlan = null
      } finally {
        previewing = false
      }
    }, 350)
    return () => clearTimeout(previewTimer)
  })

  /** A planned path inside the chosen library folder, starting with its name. */
  function inLibrary(path: string): string {
    const root = roots.find((r) => r.path === addFolder)
    if (!root || !path.startsWith(addFolder)) return path
    return `${root.name}${path.slice(addFolder.length)}`
  }

  const going = $derived(addPlan?.filter((p) => !p.problem).length ?? 0)

  async function approveImport() {
    if (importing === null || !addFolder || busy) return
    const batch = importing
    busy = true
    try {
      const changes = changeList()
      if (changes.length) {
        await unlocked(() => api.editorImportApply(changes))
        pending = {}
      }
      const ids = pickedSongs.map((s) => s.id)
      const done = await unlocked(() => api.editorImportAdd(batch, addFolder, addPattern, false, ids))
      const added = done.files.filter((f) => !f.problem)
      const failed = done.files.filter((f) => f.problem)
      toast(
        t('ed.added', { n: n(added.length) }) +
          (failed.length ? t('ed.werent', { n: failed.length, problem: failed[0].problem ?? '' }) : ''),
        failed.length > 0,
      )
      picked = {}
      await loadImports()
      void loadHistory()
      if (added.length > 0) {
        clearListCache()
        void refreshTree()
      }
      if (imports.some((i) => i.batch === batch)) {
        // Some are still waiting (not picked, or they couldn't go in).
        await loadSongs(0)
      } else {
        importing = null
        songs = []
        total = 0
        chosen = null
        addPlan = null
        if (imports.length) await openImport(imports[0].batch)
      }
    } catch (e) {
      toast(e instanceof Error ? e.message : t('ed.add_failed'), true)
    } finally {
      busy = false
    }
  }

  // --- The automatic look-up: every import is looked up on Deezer in the
  // background, one at a time, and only a complete match (see autoMatch)
  // fills its tags, as waiting changes highlighted in the grid when it's
  // opened. Matched imports are marked in the list on the left, so they can
  // be picked and approved together (approvePicked).
  type AutoState = 'looking' | 'matched' | 'none'
  let autoState = $state<Record<number, AutoState>>({})
  /** What each matched import was matched to. */
  const autoFound: Record<number, { album: string; artist: string; changed: boolean }> = {}
  let autoNote = $state<{ text: string; found: boolean } | null>(null)
  let lookingAll = false

  function noteOf(batch: number): { text: string; found: boolean } | null {
    const state = autoState[batch]
    if (state === 'looking') return { text: t('ed.auto_looking'), found: false }
    if (state === 'none') return { text: t('ed.auto_none'), found: false }
    const found = autoFound[batch]
    if (state === 'matched' && found) {
      const names = { album: found.album, artist: found.artist }
      return { text: found.changed ? t('ed.auto_found', names) : t('ed.auto_same', names), found: true }
    }
    return null
  }

  async function autoLookUp(batch: number, list: EditorSong[] | null = null) {
    if (autoState[batch]) return
    autoState = { ...autoState, [batch]: 'looking' }
    if (importing === batch) autoNote = noteOf(batch)
    let result: AutoState = 'none'
    try {
      const rows = list ?? (await api.editorImportSongs(batch)).songs
      const match = await autoMatch(rows)
      if (match) {
        // The changes go to the import's waiting changes: straight into the
        // grid when it's open, else kept for when it is. Changes made by hand
        // meanwhile stay.
        const byId = new Map(rows.map((r) => [r.id, r]))
        let changed = false
        if (importing === batch) {
          for (const c of match.changes) {
            if (pending[c.song.id]?.[c.field] !== undefined) continue
            setPending(c.song, c.field, c.value)
          }
          changed = changeCount > 0
        } else {
          const waiting = { ...(waitingFor[batch] ?? {}) }
          for (const c of match.changes) {
            const song = byId.get(c.song.id) ?? c.song
            const value = c.value.trim().replace(/\s+/g, ' ')
            const fields = { ...(waiting[song.id] ?? {}) }
            if (fields[c.field] !== undefined || value === original(song, c.field)) continue
            fields[c.field] = value
            waiting[song.id] = fields
            changed = true
          }
          waitingFor[batch] = waiting
        }
        autoFound[batch] = { album: match.release.title, artist: match.release.artist, changed }
        result = 'matched'
        // A cover only when the files have none.
        if (match.release.has_cover !== false && rows.every((s) => !s.art_hash)) {
          const res = await fetch(api.editorReleaseCover(match.hit.source, match.hit.id))
          if (res.ok) {
            const blob = await res.blob()
            const kind = blob.type === 'image/png' ? 'png' : 'jpg'
            const file = new File([blob], `cover.${kind}`, { type: blob.type })
            await unlocked(() => api.editorImportCover(rows.map((s) => s.id), file))
            if (importing === batch) await loadSongs(0)
          }
        }
      }
    } catch {
      result = autoFound[batch] ? 'matched' : 'none'
    }
    autoState = { ...autoState, [batch]: result }
    if (importing === batch) autoNote = noteOf(batch)
  }

  /** Looks up every import not looked up yet, one after another. */
  async function lookUpAll() {
    if (lookingAll) return
    lookingAll = true
    try {
      for (let next = imports.find((i) => !autoState[i.batch]); next; next = imports.find((i) => !autoState[i.batch])) {
        await autoLookUp(next.batch)
      }
    } finally {
      lookingAll = false
    }
  }

  /** Picks every import matched on Deezer. */
  function pickMatched() {
    pickedImports = Object.fromEntries(imports.filter((i) => autoState[i.batch] === 'matched').map((i) => [i.batch, true]))
  }

  /** Approves every picked import: writes its waiting changes and moves it
   * into the library folder chosen on the right, as Approve Import does for one. */
  async function approvePicked() {
    const batches = pickedImportBatches
    if (!batches.length || !addFolder || busy) return
    const unmatched = batches.filter((b) => autoState[b] !== 'matched').length
    if (!confirm(t('ed.approve_picked_q', { n: batches.length }) + (unmatched ? t('ed.approve_unmatched', { n: unmatched }) : ''))) return
    const current = importing
    if (current !== null && batches.includes(current)) waitingFor[current] = pending
    busy = true
    let albums = 0
    let added = 0
    const problems: string[] = []
    try {
      for (const batch of batches) {
        try {
          const changes: EditorChange[] = []
          for (const [id, fields] of Object.entries(waitingFor[batch] ?? {})) {
            for (const [field, value] of Object.entries(fields)) {
              changes.push({ id: Number(id), field: field as EditorField, value: value === '' ? null : (value ?? null) })
            }
          }
          if (changes.length) await unlocked(() => api.editorImportApply(changes))
          delete waitingFor[batch]
          const done = await unlocked(() => api.editorImportAdd(batch, addFolder, addPattern, false, []))
          const went = done.files.filter((f) => !f.problem).length
          if (went) albums++
          added += went
          const failed = done.files.find((f) => f.problem)
          if (failed) problems.push(failed.problem ?? '')
        } catch (e) {
          problems.push(e instanceof Error ? e.message : t('ed.add_failed'))
        }
      }
      toast(
        t('ed.approved_picked', { n: albums, songs: n(added) }) +
          (problems.length ? t('ed.werent', { n: problems.length, problem: problems[0] }) : ''),
        problems.length > 0,
      )
      pickedImports = {}
      lastPickedImport = null
      await loadImports()
      void loadHistory()
      if (added > 0) {
        clearListCache()
        void refreshTree()
      }
      if (current !== null && batches.includes(current)) {
        pending = waitingFor[current] ?? {}
        delete waitingFor[current]
        importing = null
        songs = []
        total = 0
        chosen = null
        addPlan = null
        const next = imports.find((i) => i.batch === current) ?? imports[0]
        if (next) await openImport(next.batch)
      }
    } finally {
      busy = false
    }
  }

  onMount(async () => {
    await loadRoots()
    void loadImports()
    if (startFolder) {
      selected = startFolder
      await loadSongs(0)
    } else if (startSearch) {
      await find(startSearch)
    }
  })

  /** A field as it stands now: the file's tag when editing the library, DJINN's otherwise. */
  function original(song: EditorSong, field: EditorField): string {
    if (field.startsWith('x:')) return song.file?.extra?.[field.slice(2)] ?? ''
    const own = field as EditorCoreField
    const value = editingFiles ? (song.file ? song.file[own] : undefined) : song[own]
    return value === null || value === undefined ? '' : String(value)
  }

  function shown(song: EditorSong, field: EditorField): string {
    return pending[song.id]?.[field] ?? original(song, field)
  }

  function isPending(song: EditorSong, field: EditorField) {
    return pending[song.id]?.[field] !== undefined
  }

  function setPending(song: EditorSong, field: EditorField, value: string) {
    const clean = value.trim().replace(/\s+/g, ' ')
    const fields = { ...(pending[song.id] ?? {}) }
    if (clean === original(song, field)) delete fields[field]
    else fields[field] = clean
    const next = { ...pending }
    if (Object.keys(fields).length) next[song.id] = fields
    else delete next[song.id]
    pending = next
  }

  /** Changes to the files are only allowed in folders the admin allowed. */
  const rowEditable = (song: EditorSong) =>
    importing !== null || target === 'database' || (song.editable && !!song.file)
  const canEdit = $derived(importing !== null || target === 'database' || songs.some((s) => s.editable))
  /** Songs with changes waiting that can't take them (library, view only). */
  const blocked = $derived(
    target === 'library' && importing === null ? songs.filter((s) => pending[s.id] && !s.editable).length : 0,
  )

  function startEdit(song: EditorSong, field: EditorField) {
    if (!rowEditable(song)) return
    editing = { id: song.id, field }
    draft = shown(song, field)
  }

  function finishEdit(song: EditorSong, keep: boolean) {
    if (keep && editing) setPending(song, editing.field, draft)
    editing = null
  }

  function onKey(e: KeyboardEvent, song: EditorSong) {
    if (e.key === 'Enter') finishEdit(song, true)
    if (e.key === 'Escape') finishEdit(song, false)
  }

  function focusSelect(node: HTMLInputElement) {
    node.focus()
    node.select()
  }

  function pick(song: EditorSong, index: number, e: MouseEvent) {
    const on = !picked[song.id]
    const next = { ...picked }
    if (e.shiftKey && lastPicked !== null) {
      const [a, b] = [Math.min(lastPicked, index), Math.max(lastPicked, index)]
      for (const s of view.slice(a, b + 1)) next[s.id] = true
    } else {
      next[song.id] = on
    }
    picked = next
    lastPicked = index
  }

  const pickedSongs = $derived(view.filter((s) => picked[s.id]))
  const allPicked = $derived(songs.length > 0 && pickedSongs.length === songs.length)
  /** The songs the tools work on: those picked, else every song shown. */
  const toolSongs = $derived(pickedSongs.length ? pickedSongs : view)

  function pickAll(on: boolean) {
    picked = on ? Object.fromEntries(songs.map((s) => [s.id, true])) : {}
  }

  function pickGroup(list: EditorSong[], on: boolean) {
    const next = { ...picked }
    for (const s of list) next[s.id] = on
    picked = next
  }

  /** The tools only touch songs that can be changed. */
  const changeable = $derived(toolSongs.filter(rowEditable))

  function fill() {
    for (const song of changeable) setPending(song, fillField, fillValue)
  }

  // --- Lyrics and identifying by sound, for the songs the tools work on.

  /** What the lyrics tools are doing, for the button row. */
  let lyricsBusy = $state('')
  /** Songs Find lyrics found nothing for anywhere, to offer the AI. */
  let noLyrics = $state<number[]>([])

  const aiNote = () => t('ed.ai_note')

  function aiLabel(job: string | null): string {
    if (!job) return ''
    const [kind, ...rest] = job.split(' ')
    const when = rest.join(' ')
    const what = kind === 'sync' ? 'sync' : 'make'
    if (when === 'working') return t(`ed.ai_${what}_working`)
    if (when === 'after adding') return t(`ed.ai_${what}_after`)
    return t(`ed.ai_${what}_soon`)
  }

  async function lyricsTool(kind: 'find' | 'timed' | 'generate', ids: number[] = changeable.map((s) => s.id)) {
    if (!ids.length) return
    const staged = importing !== null
    lyricsBusy =
      kind === 'find'
        ? t('ed.finding_lyrics', { n: ids.length })
        : kind === 'timed'
          ? t('ed.finding_timed', { n: ids.length })
          : t('ed.asking_ai')
    try {
      const done = await unlocked(() =>
        kind === 'find'
          ? api.editorLyricsFind(ids, staged)
          : kind === 'timed'
            ? api.editorLyricsTimed(ids, staged)
            : api.editorLyricsGenerate(ids, staged),
      )
      const count = (r: string) => done.songs.filter((s) => s.result === r).length
      const asked = count('asked')
      if (kind === 'find') {
        noLyrics = done.songs.filter((s) => s.result === 'none').map((s) => s.id)
        const failed = done.songs.find((s) => s.result === 'failed')
        toast(
          t('ed.lyr_found', { timed: count('timed'), words: count('words') }) +
            (count('had') ? t('ed.lyr_had', { n: count('had') }) : '') +
            (count('instrumental') ? t('ed.lyr_instrumental', { n: count('instrumental') }) : '') +
            (noLyrics.length ? t('ed.lyr_not_found', { n: noLyrics.length }) : '') +
            (failed ? t('ed.lyr_lrclib_down', { error: failed.error ?? '' }) : ''),
          !!failed,
        )
      } else if (kind === 'timed') {
        toast(
          t('ed.lyr_timed', { n: count('timed') }) +
            (asked ? t('ed.lyr_ai_time', { n: asked }) : '') +
            (count('skipped') ? t('ed.lyr_no_words', { n: count('skipped') }) : '') +
            (asked && !done.ai_on ? `.${aiNote()}` : ''),
        )
      } else {
        noLyrics = []
        toast(
          (asked ? t('ed.lyr_ai_make', { n: asked }) : t('ed.lyr_all_have')) +
            (count('had') && asked ? t('ed.lyr_have_some', { n: count('had') }) : '') +
            (asked && !done.ai_on ? `.${aiNote()}` : ''),
        )
      }
      await loadSongs(0)
    } catch (e) {
      toast(e instanceof Error ? e.message : t('ed.lyr_failed'), true)
    } finally {
      lyricsBusy = ''
    }
  }

  // While the AI works on songs here, follow just their lyrics and AI state
  // (a small database check, not a reload that opens every file).
  $effect(() => {
    if (!songs.some((s) => s.ai_job)) return
    const timer = setInterval(async () => {
      if (loading || busy || lyricsBusy || (!selected && !searching && importing === null)) return
      const ids = songs.filter((s) => s.ai_job).map((s) => s.id)
      if (!ids.length) return
      const asked = importing
      try {
        const found = await api.editorAiStatus(ids, asked !== null)
        // Import file ids and library song ids can match: drop an answer
        // that came back after switching between them.
        if (importing !== asked) return
        const byId = new Map(found.map((s) => [s.id, s]))
        songs = songs.map((s) => {
          const fresh = byId.get(s.id)
          return fresh ? { ...s, lyrics: fresh.lyrics, ai_job: fresh.ai_job } : s
        })
      } catch {
        // Tried again on the next tick.
      }
    }, 4000)
    return () => clearInterval(timer)
  })

  /** Looks songs up by their sound and puts what's found in as changes to approve. */
  async function identifyBySound() {
    const list = changeable
    if (!list.length) return
    lyricsBusy = t('ed.listening', { n: list.length })
    try {
      const heard = await unlocked(() =>
        api.editorIdentify(
          list.map((s) => s.id),
          importing !== null,
        ),
      )
      let found = 0
      let changes = 0
      for (const h of heard) {
        const song = songs.find((s) => s.id === h.id)
        if (!song || !h.found) continue
        found++
        const set = (field: EditorField, value: string | number | null) => {
          if (value === null || value === '' || String(value) === shown(song, field)) return
          setPending(song, field, String(value))
          changes++
        }
        set('title', h.found.title)
        set('artist', h.found.artist)
        set('album', h.found.album)
        set('year', h.found.year)
        if (!shown(song, 'genre')) set('genre', h.found.genre)
      }
      const missed = heard.length - found
      toast(
        t('ed.recognized', { found, total: heard.length }) +
          (missed ? t('ed.missed', { n: missed }) : '') +
          (changes ? t('ed.changes_waiting', { n: changes }) : t('ed.tags_agree')),
      )
    } catch (e) {
      toast(e instanceof Error ? e.message : t('ed.identify_failed'), true)
    } finally {
      lyricsBusy = ''
    }
  }

  /** Track numbers 1, 2, 3... in the grid's order, for each album and disc. */
  function numberInOrder() {
    const next: Record<string, number> = {}
    for (const song of changeable) {
      const key = `${shown(song, 'album_artist') || shown(song, 'artist')}\u0000${shown(song, 'album')}\u0000${shown(song, 'disc_no') || '1'}`
      next[key] = (next[key] ?? 0) + 1
      setPending(song, 'track_no', String(next[key]))
    }
  }

  /** The chosen song's album's track numbers from MusicBrainz, as changes to approve. */
  async function numbersFromMusicBrainz() {
    const album = chosen?.album_id
    if (album == null) return
    try {
      const list = await unlocked(() => api.editorTracklist(album))
      const multiDisc = list.suggested.some((row) => row.disc_no > 1)
      let set = 0
      for (const row of list.suggested) {
        const song = songs.find((s) => s.id === row.id)
        if (!song || !rowEditable(song)) continue
        setPending(song, 'track_no', String(row.track_no))
        if (multiDisc || shown(song, 'disc_no')) setPending(song, 'disc_no', String(row.disc_no))
        set++
      }
      toast(
        t('ed.numbered', { release: list.release, tracks: list.tracks, n: set }) +
          (list.unmatched ? t('ed.unmatched', { n: list.unmatched }) : '') +
          t('ed.check_changes'),
      )
    } catch (e) {
      toast(e instanceof Error ? e.message : t('ed.mb_failed'), true)
    }
  }

  const JUNK =
    /\s*[([](official\s+(music\s+)?video|official\s+audio|official\s+lyric\s+video|lyric\s+video|lyrics?|audio|video|hd|hq|4k|visuali[sz]er)[)\]]/gi

  /** A title without a leading track number or a video site's leftovers. */
  function cleanTitle(title: string): string {
    return title
      .replace(JUNK, '')
      .replace(/^\s*\d{1,3}\s*[-._)]\s+(?=\S)/, '')
      .replace(/\s+/g, ' ')
      .trim()
  }

  function cleanTitles() {
    let changed = 0
    for (const song of changeable) {
      const now = shown(song, 'title')
      const clean = cleanTitle(now)
      if (clean && clean !== now) {
        setPending(song, 'title', clean)
        changed++
      }
    }
    toast(changed ? t('ed.cleaned', { n: changed }) : t('ed.nothing_clean'))
  }

  /** The changes as the server takes them. */
  function changeList(): EditorChange[] {
    const out: EditorChange[] = []
    for (const [id, fields] of Object.entries(pending)) {
      for (const [field, value] of Object.entries(fields)) {
        out.push({ id: Number(id), field: field as EditorField, value: value === '' ? null : (value ?? null) })
      }
    }
    return out
  }

  const review = $derived(
    Object.entries(pending).flatMap(([id, fields]) => {
      const song = songs.find((s) => s.id === Number(id))
      return Object.entries(fields).map(([field, value]) => ({
        id: Number(id),
        song: song?.title ?? t('ed.song_n', { n: id }),
        field: field as EditorField,
        from: song ? original(song, field as EditorField) : '',
        to: value ?? '',
      }))
    }),
  )

  function undoOne(id: number, field: EditorField) {
    const fields = { ...(pending[id] ?? {}) }
    delete fields[field]
    const next = { ...pending }
    if (Object.keys(fields).length) next[id] = fields
    else delete next[id]
    pending = next
  }

  async function approve() {
    const changes = changeList()
    if (!changes.length) return
    const songCount = Object.keys(pending).length
    if (
      target === 'library' &&
      importing === null &&
      !confirm(t('ed.write_q', { changes: changes.length, songs: songCount }))
    )
      return
    busy = true
    try {
      const done = await unlocked(() =>
        importing !== null ? api.editorImportApply(changes) : api.editorApply(target, changes),
      )
      addPlan = null
      pending = {}
      report(done.songs, done.failed, target === 'library' ? 'ed.r_files_written' : 'ed.r_songs_changed')
      clearListCache()
      await loadSongs(0)
      if (target === 'library') await loadHistory()
    } catch (e) {
      toast(e instanceof Error ? e.message : t('ed.approve_failed'), true)
    } finally {
      busy = false
    }
  }

  /** Says how a change went; `what` is the language key for "{n} files written" and the like. */
  function report(count: number, failed: { id: number; error: string }[], what: string) {
    if (failed.length) {
      const first = failed[0]
      const name = songs.find((s) => s.id === first.id)?.title ?? t('ed.song_lower', { n: first.id })
      toast(t(what, { n: n(count) }) + t('ed.r_failed', { n: failed.length, name, error: first.error }), true)
    } else {
      toast(t(what, { n: n(count) }))
    }
  }

  async function undo(batch: EditorBatch) {
    if (!confirm(t('ed.undo_q', { batch: batch.batch, files: batch.files }))) return
    busy = true
    try {
      const done = await unlocked(() => api.editorUndo(batch.batch))
      report(done.songs, done.failed, 'ed.r_put_back')
      clearListCache()
      await refreshTree(selected)
      await loadSongs(0)
      await loadHistory()
    } catch (e) {
      toast(e instanceof Error ? e.message : t('ed.undo_failed'), true)
    } finally {
      busy = false
    }
  }

  /** The chosen song's album among the songs shown: by DJINN's album when
   * it has one, else (an import, a song new to DJINN) by the album as the
   * tags say, else by folder. */
  function albumMates(song: EditorSong | null): EditorSong[] {
    if (!song) return []
    if (song.album_id != null) return songs.filter((s) => s.album_id === song.album_id)
    // "Album CD 1" and "Album CD 2" are one album's discs.
    const album = albumBase(original(song, 'album'))
    if (album) return songs.filter((s) => s.album_id == null && albumBase(original(s, 'album')) === album)
    const folder = folderOf(song)
    return songs.filter((s) => s.album_id == null && !original(s, 'album') && folderOf(s) === folder)
  }

  /** The songs a cover goes to: those picked, else the chosen song's album here. */
  const coverSongsNow = $derived(
    (pickedSongs.length
      ? pickedSongs
      : chosen
        ? albumMates(chosen)
        : []
    ).filter((s) => target === 'database' || s.editable),
  )

  /** The album layout for the picked songs, or the chosen song's album. */
  function openLayout() {
    const list = pickedSongs.length
      ? pickedSongs
      : albumMates(chosen)
    if (list.length) layoutSongs = list
  }

  async function useLayout(changes: { song: EditorSong; field: EditorField; value: string }[], cover: Blob | null) {
    const list = layoutSongs ?? []
    let skipped = 0
    const showHidden = new Set<string>()
    const addExtra: string[] = []
    for (const c of changes) {
      if (rowEditable(c.song)) {
        setPending(c.song, c.field, c.value)
        if (c.field.startsWith('x:')) {
          const name = c.field.slice(2)
          if (!columns.extra.includes(name) && !addExtra.includes(name)) addExtra.push(name)
        } else if (columns.hidden.includes(c.field)) {
          showHidden.add(c.field)
        }
      } else skipped++
    }
    if (showHidden.size || addExtra.length) {
      columns = {
        hidden: columns.hidden.filter((h) => !showHidden.has(h)),
        extra: [...columns.extra, ...addExtra],
      }
    }
    layoutSongs = null
    toast(
      t('ed.from_release') + (skipped ? t('ed.left_read_only', { n: skipped }) : ''),
    )
    if (cover) {
      const kind = cover.type === 'image/png' ? 'png' : 'jpg'
      await setCover('set', new File([cover], `cover.${kind}`, { type: cover.type }), list)
    }
  }

  async function setCover(how: 'set' | 'remove' | 'reset', file: File | null = null, only: EditorSong[] | null = null) {
    const coverSongs = (only ?? coverSongsNow).filter((s) => target === 'database' || s.editable)
    if (!coverSongs.length) return
    if (importing !== null) {
      if (how === 'reset') return
      busy = true
      try {
        const done = await unlocked(() => api.editorImportCover(coverSongs.map((s) => s.id), how === 'set' ? file : null))
        report(done.songs, done.failed, 'ed.r_waiting_changed')
        await loadSongs(0)
      } catch (e) {
        toast(e instanceof Error ? e.message : t('ed.cover_failed'), true)
      } finally {
        busy = false
        if (coverInput) coverInput.value = ''
      }
      return
    }
    const where = target === 'library' ? 'files' : 'db'
    if (!confirm(t(`ed.cover_q_${how}_${where}`, { n: coverSongs.length }))) return
    busy = true
    try {
      const done = await unlocked(() => api.editorCover(target, coverSongs.map((s) => s.id), file, how))
      report(done.songs, done.failed, target === 'library' ? 'ed.r_files_written' : 'ed.r_songs_changed')
      clearListCache()
      await loadSongs(0)
      if (target === 'library') await loadHistory()
    } catch (e) {
      toast(e instanceof Error ? e.message : t('ed.cover_failed'), true)
    } finally {
      busy = false
      if (coverInput) coverInput.value = ''
    }
  }

  function relocation(kind: MoveKind, ids: number[], preview: boolean) {
    const after = { remove_empty: removeEmpty, folder_art: folderArt }
    if (kind === 'rename') return api.editorRename(ids, pattern, preview, after)
    if (kind === 'mp3') return api.editorMp3Length(ids, preview)
    if (kind === 'copy') return api.editorCopy(ids, moveTo, preview)
    if (kind === 'move') return api.editorMove(ids, { folder: moveTo }, preview, after)
    return api.editorMove(ids, { delete: true }, preview, { remove_empty: removeEmpty })
  }

  async function replaceFile(song: EditorSong, file: File) {
    if (!confirm(t('ed.replace_q', { new: file.name, old: fileName(song.path) })))
      return
    busy = true
    try {
      await unlocked(() => api.editorReplace(song.id, file))
      toast(t('ed.replaced'))
      await loadSongs(0)
      await loadHistory()
    } catch (e) {
      toast(e instanceof Error ? e.message : t('ed.replace_failed'), true)
    } finally {
      busy = false
      if (replaceInput) replaceInput.value = ''
    }
  }

  async function makeFolderArt() {
    busy = true
    try {
      const done = await unlocked(() => api.editorFolderArt(coverSongsNow.map((s) => s.id), true))
      toast(
        done.failed.length
          ? t('ed.folder_art_partial', { n: done.songs, error: done.failed[0].error })
          : t('ed.folder_art_done', { n: done.songs }),
      )
      await loadHistory()
    } catch (e) {
      toast(e instanceof Error ? e.message : t('ed.write_failed'), true)
    } finally {
      busy = false
    }
  }

  async function previewMove(kind: MoveKind) {
    if (kind === 'rename' && changeCount && !confirm(t('ed.rename_q')))
      return
    if ((kind === 'move' || kind === 'copy') && !moveTo.trim()) return
    try {
      moveKind = kind
      const found = (await unlocked(() => relocation(kind, changeable.map((s) => s.id), true))).files
      // Only the MP3s that need it are worth listing.
      renames = kind === 'mp3' ? found.filter((f) => !f.problem) : found
      if (kind === 'mp3' && !renames.length) {
        renames = null
        toast(t('ed.mp3_ok'))
      }
    } catch (e) {
      toast(e instanceof Error ? e.message : t('ed.where_failed'), true)
    }
  }

  const renamable = $derived(renames?.filter((r) => !r.problem) ?? [])
  /** The wording for each kind of file change: `ed.mv_title_<kind>`,
   * `ed.mv_<kind>` (the button), `ed.mv_q_<kind>` and `ed.mv_done_<kind>`. */
  const moveWords = (kind: MoveKind) => ({
    title: t(`ed.mv_title_${kind}`),
    verb: t(`ed.mv_${kind}`),
    ask: (count: number) => t(`ed.mv_q_${kind}`, { n: count }),
    done: (count: string) => t(`ed.mv_done_${kind}`, { n: count }),
  })

  async function rename() {
    if (!renamable.length) return
    const words = moveWords(moveKind)
    if (!confirm(words.ask(renamable.length))) return
    busy = true
    try {
      const done = await unlocked(() => relocation(moveKind, renamable.map((r) => r.id), false))
      const moved = done.files.filter((f) => !f.problem).length
      toast(words.done(n(moved)))
      renames = null
      clearListCache()
      await refreshTree(selected)
      await loadSongs(0)
      await loadHistory()
    } catch (e) {
      toast(e instanceof Error ? e.message : t('ed.rename_failed'), true)
    } finally {
      busy = false
    }
  }

  /** Two paths without the folders they share, to show what changes. */
  function afterShared(from: string, to: string): [string, string] {
    const a = from.split(/[\\/]/)
    const b = to.split(/[\\/]/)
    let i = 0
    while (i < a.length - 1 && i < b.length - 1 && a[i] === b[i]) i++
    return [a.slice(i).join('/'), b.slice(i).join('/')]
  }

  const fileName = (path: string) => path.slice(Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\')) + 1)
  /** A history entry's kinds of change, named (`ed.k_<kind>`). */
  const KINDS = ['tags', 'cover', 'rename', 'delete', 'copy', 'folder', 'folder-art', 'mp3-length', 'replace']
  const kindName = (kinds: string) =>
    kinds.startsWith('undo')
      ? t('ed.k_undo')
      : kinds
          .split(',')
          .map((k) => (KINDS.includes(k) ? t(`ed.k_${k.replace('-', '_')}`) : k))
          .join(', ')
</script>

{#snippet branch(nodes: EditorNode[], depth: number)}
  <ul class="tree" style:--depth={depth}>
    {#each nodes as node (node.path)}
      <li>
        <div class="node" class:picked={selected === node.path}>
          {#if node.has_folders}
            <button class="twisty" aria-label={open[node.path] ? t('common.close') : t('common.open')} onclick={() => toggle(node)}>
              {open[node.path] ? '−' : '+'}
            </button>
          {:else}
            <span class="twisty" aria-hidden="true"></span>
          {/if}
          <button class="label" onclick={() => choose(node)} title={node.path}>
            {node.name}
            <span class="count">{n(node.songs)}</span>
            {#if node.editable === true}<span class="mode on">{t('ed.rw')}</span>{:else if node.editable === false}<span class="mode">{t('ed.ro')}</span>{/if}
          </button>
        </div>
        {#if open[node.path] && children[node.path]}
          {@render branch(children[node.path], depth + 1)}
        {/if}
      </li>
    {/each}
  </ul>
{/snippet}

{#if locked}
  <section class="card">
    <p class="small">{t('ed.locked')} <button class="link" onclick={unlock}>{t('ed.unlock')}</button></p>
  </section>
{:else}
  <div class="editor" class:library={target === 'library' && !importMode} class:import={importMode} class:fill={windowed}>
    <div class="top">
      <div class="toggle" role="group" aria-label={t('ed.what')}>
        <button class:on={target === 'library' && !importMode} onclick={() => setTarget('library')}>{t('ed.t_library')}</button>
        <button class:on={target === 'database' && !importMode} onclick={() => setTarget('database')}>{t('ed.t_database')}</button>
        <button class:on={importMode} onclick={enterImport}>{t('ed.t_import')}</button>
      </div>
      <p class="small muted">
        {#if importMode}
          {t('ed.import_note')}
        {:else if target === 'library'}
          {t('ed.library_note')}
        {:else}
          {t('ed.database_note')}
        {/if}
      </p>
    </div>

    <div class="panes">
      <aside class="folders">
        {#if importMode}
          <div class="imports">
            <input
              type="file"
              multiple
              hidden
              accept=".mp3,.flac,.m4a,.mp4,.aac,.ogg,.oga,.wav"
              bind:this={importInput}
              onchange={(e) => {
                void importFiles(e.currentTarget.files, false)
                e.currentTarget.value = ''
              }}
            />
            <input
              type="file"
              hidden
              webkitdirectory
              bind:this={importFolderInput}
              onchange={(e) => {
                void importFiles(e.currentTarget.files, true)
                e.currentTarget.value = ''
              }}
            />
            <strong class="small">{t('ed.bring_in')}</strong>
            <div class="imports-head">
              <button class="small-button" disabled={!!uploading} onclick={() => importFolderInput?.click()}>{t('ed.a_folder')}</button>
              <button class="small-button" disabled={!!uploading} onclick={() => importInput?.click()} title={t('ed.files_title')}
                >{t('ed.song_files')}</button
              >
            </div>
            <fieldset class="layout-choice">
              <legend class="small muted">{t('ed.laid_out')}</legend>
              {#each FOLDER_LAYOUTS as l (l.key)}
                <label class="check small">
                  <input type="radio" name="folder-layout" value={l.key} bind:group={folderLayout} />
                  <span>{l.label}<span class="muted example">{l.example}</span></span>
                </label>
              {/each}
              <label class="check small">
                <input type="checkbox" bind:checked={fillFromFolders} disabled={folderLayout === 'together'} />
                <span>{t('ed.fill_folders')}</span>
              </label>
            </fieldset>
            {#if uploading}
              <p class="small muted">
                {t('ed.bringing', { done: Math.min(uploading.done + 1, uploading.of), of: uploading.of })}{uploading.imports > 1
                  ? t('ed.n_albums', { n: uploading.imports })
                  : ''}...
              </p>
            {/if}
            <div class="imports-head">
              {#if imports.length}
                <input
                  type="checkbox"
                  class="import-check"
                  aria-label={t('ed.pick_all_imports')}
                  checked={allImportsPicked}
                  onchange={(e) => pickAllImports(e.currentTarget.checked)}
                />
              {/if}
              <strong class="small">{t('ed.imports')}</strong>
              {#if pickedImportBatches.length}
                <button class="small-button danger" disabled={busy} onclick={throwAwayImports}>
                  {pickedImportBatches.length > 1 ? t('ed.throw_these') : t('ed.throw_this')}
                </button>
              {/if}
            </div>
            {#if imports.some((i) => autoState[i.batch] === 'matched') || pickedImportBatches.length}
              <div class="imports-head">
                {#if imports.some((i) => autoState[i.batch] === 'matched')}
                  <button class="small-button" disabled={busy} onclick={pickMatched} title={t('ed.pick_matched_title')}
                    >{t('ed.pick_matched')}</button
                  >
                {/if}
                {#if pickedImportBatches.length}
                  <button
                    class="small-button approve-picked"
                    disabled={busy || !addFolder}
                    onclick={approvePicked}
                    title={addFolder ? t('ed.approve_picked_title') : t('ed.approve_picked_no_folder')}
                    >{t('ed.approve_picked', { n: pickedImportBatches.length })}</button
                  >
                {/if}
              </div>
            {/if}
            {#each imports as imp, i (imp.batch)}
              <div class="node">
                <input
                  type="checkbox"
                  class="import-check"
                  aria-label={t('ed.pick', { name: imp.album || t('ed.import_n', { n: imp.batch }) })}
                  checked={!!pickedImports[imp.batch]}
                  onclick={(e) => pickImport(imp.batch, i, e)}
                />
                <button
                  class="label import-item"
                  class:picked={importing === imp.batch}
                  class:matched={autoState[imp.batch] === 'matched'}
                  title={autoState[imp.batch] === 'matched'
                    ? t('ed.import_matched')
                    : autoState[imp.batch] === 'looking'
                      ? t('ed.auto_looking')
                      : autoState[imp.batch] === 'none'
                        ? t('ed.import_unmatched')
                        : undefined}
                  onclick={() => openImport(imp.batch)}
                >
                  {#if autoState[imp.batch] === 'looking'}<span class="looking" aria-hidden="true">…</span>{/if}
                  {imp.album || t('ed.import_n', { n: imp.batch })}
                  <span class="count">{n(imp.files)}</span>
                  {#if imp.artist}<span class="mode">{imp.artist}</span>{/if}
                </button>
              </div>
            {:else}
              {#if !uploading}<p class="small muted">{t('ed.no_imports')}</p>{/if}
            {/each}
          </div>
        {:else if roots.length}
          {@render branch(roots, 0)}
        {:else}
          <p class="small muted">{t('ed.no_folders')}</p>
        {/if}
      </aside>

      <section class="grid">
        <!-- Finding is for the library; an import's songs are all shown. -->
        {#if !importMode}
        <form
          class="find pad"
          onsubmit={(e) => {
            e.preventDefault()
            void find(searchText)
          }}
        >
          <input
            type="search"
            bind:value={searchText}
            placeholder={t('ed.find_hint')}
            aria-label={t('ed.find_songs')}
          />
          <button class="small-button" type="submit" disabled={!searchText.trim()}>{t('ed.find')}</button>
        </form>
        {/if}
        {#if importMode && importing === null}
          <p class="small muted pad">{t('ed.import_pick')}</p>
        {:else if !selected && !searching && importing === null}
          <p class="small muted pad">{t('ed.folder_pick')}</p>
        {:else}
          <p class="small muted pad">
            {#if searching}{t('ed.found_for', { n: n(total) })} <b>{searching}</b>{total >= 1000 ? t('ed.first_1000') : ''}{:else}{t(
                'ed.n_of',
                { shown: n(songs.length), total: n(total) },
              )}{/if}{pickedSongs.length ? t('ed.n_picked', { n: n(pickedSongs.length) }) : ''}
            {#if target === 'library' && !canEdit}· <strong>{t('ed.ro_strong')}</strong>{t('ed.ro_here', {
                where: searching ? t('ed.these_folders') : t('ed.this_folder'),
              })}{:else if target === 'library' && searching}{t('ed.dimmed')}{:else}{t('ed.click_cell')}{/if}
          </p>
          {#if importMode && autoNote}
            <p class="auto-note small pad" class:found={autoNote.found}>
              <span class="grow">{autoNote.text}</span>
              <button class="link" aria-label={t('ed.dismiss')} title={t('ed.dismiss')} onclick={() => (autoNote = null)}>×</button>
            </p>
          {/if}
          {#if canEdit && songs.length}
            <div class="online pad">
              <div class="tool">
                <strong class="small">{t('ed.look_up')}</strong>
                <span class="small muted"
                  >{pickedSongs.length ? t('ed.for_picked', { n: changeable.length }) : t('ed.for_shown', { n: changeable.length })}</span
                >
              </div>
              <div class="tool">
                <span class="small muted">{t('ed.album_colon')}</span>
                <button
                  class="small-button"
                  disabled={!chosen && !pickedSongs.length}
                  onclick={openLayout}
                  title={t('ed.layout_title')}
                >
                  {t('ed.find_album')}
                </button>
                <button
                  class="small-button"
                  disabled={!chosen}
                  onclick={() => (coverOpen = true)}
                  title={t('ed.cover_title')}
                >
                  {t('ed.find_cover')}
                </button>
                <button
                  class="small-button"
                  disabled={!!lyricsBusy}
                  onclick={identifyBySound}
                  title={t('ed.identify_title')}
                >
                  {t('ed.identify')}
                </button>
                {#if chosen?.album_id != null}
                  <button
                    class="small-button"
                    onclick={numbersFromMusicBrainz}
                    title={t('ed.numbers_title')}
                  >
                    {t('ed.numbers')}
                  </button>
                {/if}
              </div>
              <div class="tool">
                <span class="small muted">{t('ed.lyrics_colon')}</span>
                <button
                  class="small-button"
                  disabled={!!lyricsBusy}
                  onclick={() => lyricsTool('find')}
                  title={t('ed.find_lyrics_title')}
                >
                  {t('ed.find_lyrics')}
                </button>
                <button
                  class="small-button"
                  disabled={!!lyricsBusy}
                  onclick={() => lyricsTool('generate')}
                  title={t('ed.generate_title')}
                >
                  {t('ed.generate')}
                </button>
                <button
                  class="small-button"
                  disabled={!!lyricsBusy}
                  onclick={() => lyricsTool('timed')}
                  title={t('ed.timed_title')}
                >
                  {t('ed.make_timed')}
                </button>
                {#if lyricsBusy}<span class="small muted">{lyricsBusy}</span>{/if}
              </div>
              {#if noLyrics.length}
                <div class="tool offer">
                  <span class="small">{t('ed.no_lyrics_anywhere', { n: noLyrics.length })}</span>
                  <button class="small-button" disabled={!!lyricsBusy} onclick={() => lyricsTool('generate', noLyrics)}>
                    {t('ed.generate_them')}
                  </button>
                  <button class="link small" onclick={() => (noLyrics = [])}>{t('ed.not_now')}</button>
                </div>
              {/if}
            </div>
          {/if}
          <div class="view pad">
            <label class="small"
              >{t('ed.group_by')}
              <select bind:value={groupBy}>
                {#each GROUPS as g (g.key)}<option value={g.key}>{g.label}</option>{/each}
              </select>
            </label>
            {#if sortBy}
              <button class="link small" onclick={() => (sortBy = null)}>{t('ed.folder_order')}</button>
            {/if}
            <span class="grow"></span>
            <select bind:value={addPick} aria-label={t('ed.column_to_add')}>
              <option value="">{t('ed.add_column')}</option>
              {#if columns.hidden.length}
                <optgroup label={t('ed.cols_off')}>
                  {#each FIELDS.filter((f) => columns.hidden.includes(f.key)) as f (f.key)}<option value={f.key}>{f.label}</option>{/each}
                </optgroup>
              {/if}
              {#if editingFiles && inFiles.length}
                <optgroup label={t('ed.in_files')}>
                  {#each inFiles as name (name)}<option value="x:{name}">{tagLabel(name)}</option>{/each}
                </optgroup>
              {/if}
              <optgroup label={t('ed.common_tags')}>
                {#each notShown(COMMON_TAGS) as [name] (name)}<option value="x:{name}">{tagLabel(name)}</option>{/each}
              </optgroup>
              <optgroup label={t('ed.more_tags')}>
                {#each notShown(MORE_TAGS) as [name] (name)}<option value="x:{name}">{tagLabel(name)}</option>{/each}
              </optgroup>
            </select>
            <button class="small-button" disabled={!addPick} onclick={addColumn}>{t('common.add')}</button>
          </div>
          {#if !editingFiles && columns.extra.length}
            <p class="small muted pad">
              {t('ed.other_cols', { names: columns.extra.map(tagLabel).join(', ') })}
            </p>
          {/if}
          <div class="scroll">
            <table>
              <thead>
                <tr>
                  <th class="pick">
                    <input type="checkbox" aria-label={t('ed.pick_all')} checked={allPicked} onchange={(e) => pickAll(e.currentTarget.checked)} />
                  </th>
                  {#each shownColumns as col (col.key)}
                    <th aria-sort={sortBy?.field === col.key ? (sortBy.desc ? 'descending' : 'ascending') : undefined}>
                      <span class="head">
                        <button
                          class="sort"
                          title={t('ed.sort_by', { name: col.label })}
                          onmousedown={(e) => e.preventDefault()}
                          onclick={() => sortOn(col.key)}
                        >
                          {col.label}{#if sortBy?.field === col.key}<span class="arrow">{sortBy.desc ? '▼' : '▲'}</span>{/if}
                        </button>
                        {#if col.key !== 'title'}
                          <button class="drop" aria-label={t('ed.take_col_off', { name: col.label })} title={t('ed.take_col_title')} onclick={() => removeColumn(col.key)}
                            >×</button
                          >
                        {/if}
                      </span>
                    </th>
                  {/each}
                  <th>{t('ed.lyrics_col')}</th>
                  <th aria-sort={sortBy?.field === 'file' ? (sortBy.desc ? 'descending' : 'ascending') : undefined}>
                    <button
                      class="sort"
                      title={t('ed.sort_file')}
                      onmousedown={(e) => e.preventDefault()}
                      onclick={() => sortOn('file')}
                    >
                      {t('ed.file_col')}{#if sortBy?.field === 'file'}<span class="arrow">{sortBy.desc ? '▼' : '▲'}</span>{/if}
                    </button>
                  </th>
                </tr>
              </thead>
              <tbody>
                {#each groups as group (group.key)}
                  {#if groupBy}
                    <tr class="group">
                      <td class="pick">
                        <input
                          type="checkbox"
                          aria-label={t('ed.pick_group', { name: group.label })}
                          checked={group.songs.every((s) => picked[s.id])}
                          onchange={(e) => pickGroup(group.songs, e.currentTarget.checked)}
                        />
                      </td>
                      <td colspan={shownColumns.length + 2}>
                        <strong>{group.label}</strong> <span class="muted">· {t('count.songs', { n: group.songs.length })}</span>
                      </td>
                    </tr>
                  {/if}
                {#each group.songs as song (song.id)}
                  <tr
                    class:chosen={chosen?.id === song.id}
                    class:unreadable={target === 'library' && importing === null && !rowEditable(song)}
                    title={target === 'library' && importing === null && !song.editable ? t('ed.ro_folder') : undefined}
                  >
                    <td class="pick">
                      <input
                        type="checkbox"
                        aria-label={t('ed.pick_song', { name: song.title })}
                        checked={!!picked[song.id]}
                        onclick={(e) => pick(song, position.get(song.id) ?? 0, e)}
                      />
                    </td>
                    {#each shownColumns as f (f.key)}
                      <td
                        class:narrow={f.narrow}
                        class:pending={isPending(song, f.key)}
                        title={isPending(song, f.key) ? t('ed.was', { value: original(song, f.key) || t('ed.empty') }) : undefined}
                        onclick={() => ((chosen = song), startEdit(song, f.key))}
                      >
                        {#if editing?.id === song.id && editing.field === f.key}
                          <input
                            class="cell"
                            bind:value={draft}
                            use:focusSelect
                            onkeydown={(e) => onKey(e, song)}
                            onblur={() => finishEdit(song, true)}
                            inputmode={f.number ? 'numeric' : undefined}
                          />
                        {:else}
                          {shown(song, f.key)}
                        {/if}
                      </td>
                    {/each}
                    <td class="lyrics-cell" class:muted={!song.lyrics}>
                      {song.lyrics === 2 ? t('ed.lyr_timed_cell') : song.lyrics === 1 ? t('ed.lyr_words_cell') : t('ed.lyr_none_cell')}
                      {#if song.ai_job}<span class="ai-job">{aiLabel(song.ai_job)}</span>{/if}
                    </td>
                    <td class="mono muted" title={song.rel_path}>{fileName(song.path)}</td>
                  </tr>
                {/each}
                {/each}
              </tbody>
            </table>
          </div>
          {#if songs.length < total && !searching}
            <button class="small-button pad" disabled={loading} onclick={() => loadSongs(songs.length)}>{t('common.show_more')}</button>
          {/if}

          {#if canEdit && songs.length}
            <div class="tools">
              <div class="tool">
                <span class="small muted"
                  >{pickedSongs.length ? t('ed.for_picked_cap', { n: changeable.length }) : t('ed.for_shown_cap', { n: changeable.length })}</span
                >
              </div>
              <div class="tool">
                <select bind:value={fillField} aria-label={t('ed.field_to_set')}>
                  {#each shownColumns as f (f.key)}<option value={f.key}>{labelOf(f.key)}</option>{/each}
                </select>
                <input bind:value={fillValue} placeholder={t('ed.value_hint')} aria-label={t('ed.value')} />
                <button class="small-button" onclick={fill}>{t('ed.set')}</button>
              </div>
              <div class="tool">
                <button class="small-button" onclick={numberInOrder} title={t('ed.number_title')}>
                  {t('ed.number')}
                </button>
                <button
                  class="small-button"
                  onclick={cleanTitles}
                  title={t('ed.clean_title')}
                >
                  {t('ed.clean')}
                </button>
              </div>
              {#if target === 'library' && importing === null}
                <div class="tool">
                  <input bind:value={pattern} aria-label={t('ed.pattern')} class="pattern" />
                  <button class="small-button" onclick={() => previewMove('rename')}>{t('ed.rename_files')}</button>
                  <span class="small muted"
                    >{'{disc} {track} {title} {artist} {album} {album_artist} {year}'}{t('ed.pattern_help')}
                    {'{album_artist}/{album}/{track} {title}'}</span
                  >
                </div>
                <div class="tool">
                  <input bind:value={moveTo} aria-label={t('ed.move_to')} class="pattern" placeholder={t('ed.move_to')} />
                  <button class="small-button" onclick={() => previewMove('move')} disabled={!moveTo.trim()}>{t('ed.move')}</button>
                  <button class="small-button" onclick={() => previewMove('copy')} disabled={!moveTo.trim()}>{t('ed.copy')}</button>
                  <button
                    class="small-button"
                    onclick={() => previewMove('delete')}
                    title={t('ed.delete_title')}
                  >
                    {t('ed.delete')}
                  </button>
                  <button
                    class="small-button"
                    onclick={() => previewMove('mp3')}
                    title={t('ed.mp3_title')}
                  >
                    {t('ed.fix_mp3')}
                  </button>
                </div>
                <div class="tool">
                  <label class="check small"><input type="checkbox" bind:checked={removeEmpty} /> {t('ed.remove_empty')}</label>
                  <label class="check small"><input type="checkbox" bind:checked={folderArt} /> {t('ed.folder_art')}</label>
                </div>
              {/if}
            </div>
          {/if}

          {#if renames}
            <div class="renames">
              <div>
                <strong>{moveWords(moveKind).title}</strong>
                <span class="small muted">({t('ed.mv_count', { n: renamable.length })})</span>
              </div>
              {#if moveKind === 'rename'}
                <p class="small muted">{t('ed.names_note')}</p>
              {/if}
              <ul>
                {#each renames as r (r.id)}
                  <li class="small" class:bad={!!r.problem}>
                    {#if moveKind === 'mp3'}
                      <span class="mono">{fileName(r.from)}</span>
                    {:else}
                      {@const [from, to] = afterShared(r.from, r.to)}
                      <span class="mono">{from}</span> → <span class="mono">{to}</span>
                    {/if}
                    {#if r.problem}<em>({r.problem})</em>{/if}
                  </li>
                {/each}
              </ul>
              <button class="primary" disabled={busy || !renamable.length} onclick={rename}>
                {t('ed.mv_go', { verb: moveWords(moveKind).verb, n: renamable.length })}
              </button>
              <button onclick={() => (renames = null)}>{t('common.cancel')}</button>
            </div>
          {/if}
        {/if}
      </section>

      <aside class="side">
        <div class="box art">
          <div class="small muted">{t('ed.album_art')}</div>
          {#if chosen}
            <button class="cover-open" title={t('ed.open_cover')} onclick={() => (coverOpen = true)}>
              <Art hash={chosen.art_hash} size={200} round={8} />
            </button>
            <div class="small">{chosen.album ?? ''}</div>
          {:else}
            <p class="small muted">{t('ed.choose_song')}</p>
          {/if}
        </div>
        <div class="box">
          <div class="small muted">
            {coverSongsNow.length
              ? t(pickedSongs.length ? 'ed.cover_picked' : 'ed.cover_album', { n: coverSongsNow.length })
              : t('ed.cover')}
          </div>
          <input
            type="file"
            accept="image/jpeg,image/png"
            hidden
            bind:this={coverInput}
            onchange={(e) => {
              const file = e.currentTarget.files?.[0]
              if (file) void setCover('set', file)
            }}
          />
          <button class="small-button" disabled={!canEdit || busy || !coverSongsNow.length} onclick={() => coverInput?.click()}>
            {t('ed.replace_cover')}
          </button>
          <button class="small-button" disabled={!canEdit || busy || !coverSongsNow.length} onclick={() => setCover('remove')}>
            {t('ed.remove_cover')}
          </button>
          {#if importing !== null}
            <!-- An import's covers are in its files; nothing else to go back to. -->
          {:else if target === 'database'}
            <button class="small-button" disabled={busy || !coverSongsNow.length} onclick={() => setCover('reset')}>
              {t('ed.files_cover')}
            </button>
          {:else}
            <button
              class="small-button"
              disabled={busy || !coverSongsNow.length}
              title={t('ed.folder_jpg_title')}
              onclick={makeFolderArt}
            >
              {t('ed.folder_jpg')}
            </button>
          {/if}
        </div>
        {#if importing !== null}
          <div class="box add">
            <div><strong>{t('ed.add_to_library')}</strong></div>
            <label class="small"
              >{t('ed.into')}
              <select bind:value={addFolder}>
                {#each roots.filter((r) => r.editable) as root (root.path)}<option value={root.path}>{root.name}</option>{/each}
              </select>
            </label>
            {#if !roots.some((r) => r.editable)}
              <p class="small bad">{t('ed.no_writable')}</p>
            {/if}
            <label class="small"
              >{t('ed.placed_as')}
              <input bind:value={addPattern} class="pattern" aria-label={t('ed.where_goes')} />
            </label>
            <span class="small muted">{'{disc} {track} {title} {artist} {album} {album_artist} {year}'}{t('ed.makes_folders')}</span>
            <p class="small muted">
              {pickedSongs.length ? t('ed.picked_go', { n: pickedSongs.length }) : t('ed.all_go', { n: songs.length })}
            </p>
            {#if addPlan}
              <ul class="changes" class:stale={previewing}>
                {#each addPlan as p (p.id)}
                  <li class="small" class:bad={!!p.problem}>
                    <span class="mono">{inLibrary(p.to)}</span>{#if p.problem}<em> ({p.problem})</em>{/if}
                  </li>
                {/each}
              </ul>
            {:else if previewing}
              <p class="small muted">{t('ed.preview_wait')}</p>
            {/if}
            <div class="row-buttons">
              <button class="small-button" disabled={busy || !pickedSongs.length} onclick={dropPicked}>{t('ed.take_picked')}</button>
              <button class="small-button danger" disabled={busy} onclick={throwAwayImports}>
                {pickedImportBatches.length > 1 ? t('ed.throw_these') : t('ed.throw_this')}
              </button>
            </div>
          </div>
        {/if}
        {#if target === 'library' && importing === null && chosen}
          <div class="box">
            <div class="small muted">{t('ed.this_file')}</div>
            <div class="small mono file-name" title={chosen.path}>{fileName(chosen.path)}</div>
            <input
              type="file"
              accept=".mp3,.flac,.m4a,.mp4,.aac,.ogg,.oga,.wav"
              hidden
              bind:this={replaceInput}
              onchange={(e) => {
                const file = e.currentTarget.files?.[0]
                if (file && chosen) void replaceFile(chosen, file)
              }}
            />
            <button
              class="small-button"
              disabled={busy || !chosen.editable}
              title={t('ed.replace_file_title')}
              onclick={() => replaceInput?.click()}
            >
              {t('ed.replace_file')}
            </button>
          </div>
        {/if}
        {#if !importMode}
        <div class="box review">
          <div><strong>{t('ed.preview_approve')}</strong></div>
          {#if review.length}
            <p class="small muted">
              {t('ed.changes_to', { changes: n(review.length), songs: n(Object.keys(pending).length) })}
              {importing !== null
                ? t('ed.into_waiting')
                : target === 'library'
                  ? t('ed.into_files')
                  : t('ed.into_db')}
            </p>
            <ul class="changes">
              {#each review.slice(0, 200) as c (`${c.id}-${c.field}`)}
                <li class="small">
                  <span class="muted">{c.song} · {labelOf(c.field)}:</span>
                  <s>{c.from || t('ed.empty')}</s> → <b>{c.to || t('ed.empty')}</b>
                  <button class="link" aria-label={t('ed.leave_one')} onclick={() => undoOne(c.id, c.field)}>×</button>
                </li>
              {/each}
              {#if review.length > 200}<li class="small muted">{t('ed.and_more', { n: n(review.length - 200) })}</li>{/if}
            </ul>
            {#if blocked}<p class="small bad">{t('ed.blocked', { n: blocked })}</p>{/if}
            <button class="primary" disabled={busy || !canEdit || blocked > 0} onclick={approve}>
              {t('ed.approve_n', { n: n(review.length) })}
            </button>
            <button class="small-button" disabled={busy} onclick={() => (pending = {})}>{t('ed.throw_away')}</button>
          {:else}
            <p class="small muted">{t('ed.no_changes')}</p>
            <button class="primary" disabled>{t('ed.approve')}</button>
          {/if}
        </div>
        {/if}
        {#if target === 'library' && importing === null}
          <div class="box">
            <div class="small muted">{t('ed.history')}</div>
            {#if history.length}
              <ul class="history">
                {#each history.slice(0, 12) as b (b.batch)}
                  <li class="small">
                    {new Date(b.made_at).toLocaleString(locale.code)} · {kindName(b.kinds)} · {t('ed.mv_count', { n: b.files })}{b.made_by
                      ? ` · ${b.made_by}`
                      : ''}
                    {#if b.undone_at}
                      <span class="muted">{t('ed.undone')}</span>
                    {:else if b.final_at}
                      <span class="muted">{t('ed.trash_emptied')}</span>
                    {:else if !b.kinds.startsWith('undo')}
                      <button class="link" disabled={busy} onclick={() => undo(b)}>{t('ed.undo')}</button>
                    {/if}
                  </li>
                {/each}
              </ul>
            {:else}
              <p class="small muted">{t('ed.none_yet')}</p>
            {/if}
          </div>
        {/if}
      </aside>
    </div>
    {#if importMode && importing !== null}
      <!-- Always in reach: writes the waiting changes and moves the songs in. -->
      <div class="approve-bar">
        <button class="primary" disabled={busy || previewing || !addFolder || going === 0} onclick={approveImport}>
          {t('ed.approve_import', { n: going })}
        </button>
      </div>
    {/if}
    {#if coverOpen && chosen}
      <CoverWindow
        song={chosen}
        count={coverSongsNow.length}
        target={importing !== null ? 'library' : target}
        staged={importing !== null}
        {canEdit}
        {busy}
        onset={(file) => setCover('set', file)}
        onremove={() => setCover('remove')}
        onreset={() => setCover('reset')}
        onfolderart={makeFolderArt}
        onclose={() => (coverOpen = false)}
      />
    {/if}
    {#if layoutSongs}
      <!-- Over the editor, so the grid keeps its room. -->
      <div class="layout-backdrop" role="presentation" onclick={() => (layoutSongs = null)}></div>
      <div class="layout-window" bind:this={layoutBox}>
        <AlbumLayout songs={layoutSongs} {editingFiles} onapply={useLayout} onclose={() => (layoutSongs = null)} />
      </div>
    {/if}
  </div>
{/if}

<style>
  .auto-note {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0 0 8px;
    padding: 6px 10px;
    border-radius: 8px;
    background: var(--surface);
  }

  .auto-note.found {
    border: 1px solid var(--accent-2, var(--accent));
  }

  /* Room under the panes, so Approve Import never covers the last line. */
  .editor.import {
    padding-bottom: 84px;
  }

  .approve-bar {
    position: fixed;
    right: 24px;
    bottom: 24px;
    z-index: 4;
  }

  .approve-bar button {
    min-height: 48px;
    padding: 0 22px;
    box-shadow: 0 6px 24px rgb(0 0 0 / 0.35);
  }

  .changes.stale {
    opacity: 0.6;
  }

  .editor {
    container-type: inline-size;
    --target: var(--accent);
    border: 2px solid var(--target);
    border-radius: 12px;
    padding: 12px;
    box-shadow: 0 0 14px var(--glow);
  }

  /* What's being edited, by colour: the library's files neon green, an
     import neon pink, DJINN's own copy the page's accent. */
  .editor.library {
    --target: #39ff6a;
    box-shadow: 0 0 16px rgb(57 255 106 / 0.45);
  }

  .editor.import {
    --target: #ff2e7e;
    box-shadow: 0 0 16px rgb(255 46 126 / 0.5);
  }

  .top {
    display: flex;
    align-items: center;
    gap: 16px;
    flex-wrap: wrap;
    margin-bottom: 10px;
  }

  .toggle {
    display: inline-flex;
    border: 1px solid var(--target);
    border-radius: 999px;
    overflow: hidden;
  }

  .toggle button {
    border: 0;
    border-radius: 0;
    padding: 8px 18px;
    background: none;
    box-shadow: none;
  }

  .toggle button.on {
    background: var(--target);
    color: #000;
    font-weight: 700;
  }

  .panes {
    display: grid;
    grid-template-columns: 220px minmax(0, 1fr) 240px;
    gap: 12px;
    min-height: 60vh;
  }

  /* Inside Admin's menu there's less room: the side boxes go under. */
  @container (max-width: 1150px) {
    .panes {
      grid-template-columns: 200px minmax(0, 1fr);
    }

    .side {
      grid-column: 1 / -1;
      display: grid;
      grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
      align-items: start;
    }

    /* What's waiting for approval comes first, where it's seen. */
    .review {
      order: -1;
    }
  }

  @container (max-width: 640px) {
    .panes {
      grid-template-columns: minmax(0, 1fr);
    }

    .folders {
      max-height: 30vh;
    }
  }

  /* In the editor's own window: the whole height, wide panes that each scroll. */
  .editor.fill {
    display: flex;
    flex-direction: column;
    height: 100%;
    box-sizing: border-box;
  }

  .fill .panes {
    flex: 1;
    min-height: 0;
    grid-template-columns: minmax(240px, 22%) minmax(0, 1fr) minmax(300px, 25%);
  }

  .fill .folders {
    max-height: none;
    min-height: 0;
  }

  .fill .grid {
    display: flex;
    flex-direction: column;
    min-height: 0;
  }

  .fill .scroll {
    flex: 1;
    min-height: 0;
    max-height: none;
  }

  .fill .side {
    min-height: 0;
    overflow: auto;
  }

  @container (max-width: 1150px) {
    .fill .panes {
      grid-template-columns: minmax(200px, 30%) minmax(0, 1fr);
      overflow: auto;
    }
  }

  @container (max-width: 640px) {
    .fill .panes {
      grid-template-columns: minmax(0, 1fr);
    }
  }

  /* The page's big touch controls are too tall for a dense grid. */
  .editor button:not(.primary),
  .editor input,
  .editor select {
    min-height: 0;
  }

  .editor input,
  .editor select {
    padding: 4px 8px;
  }

  .pick input,
  .import-check {
    width: auto;
    margin: 0;
    padding: 0;
  }

  .folders,
  .grid,
  .box {
    border: 1px solid var(--line);
    border-radius: 8px;
  }

  .folders {
    overflow: auto;
    max-height: 72vh;
    padding: 6px;
  }

  .tree {
    list-style: none;
    margin: 0;
    padding-left: 14px;
  }

  .folders > .tree {
    padding-left: 0;
  }

  .node {
    display: flex;
    align-items: center;
    gap: 4px;
  }

  .node.picked .label {
    background: var(--card-bg, transparent);
    outline: 1px solid var(--target);
  }

  .twisty {
    width: 22px;
    flex: none;
    padding: 0;
    border: 0;
    background: none;
    box-shadow: none;
  }

  .label {
    display: block;
    flex: 1;
    min-width: 0;
    text-align: left;
    border: 0;
    background: none;
    box-shadow: none;
    padding: 2px 6px;
    border-radius: 6px;
    font-size: 0.9rem;
    overflow-wrap: anywhere;
  }

  .count {
    color: var(--muted);
    font-size: 0.8em;
    margin-left: 6px;
  }

  .mode {
    display: inline;
    margin-left: 6px;
    font-size: 0.72rem;
    color: var(--muted);
  }

  .mode.on {
    color: var(--target);
    font-weight: 600;
  }

  .grid {
    min-width: 0;
  }

  .scroll {
    overflow: auto;
    max-height: 60vh;
  }

  table {
    border-collapse: collapse;
    width: 100%;
    font-size: 0.88rem;
  }

  th {
    position: sticky;
    top: 0;
    z-index: 1;
    background: var(--page-bg, #000);
    text-align: left;
    padding: 4px 8px;
    border-bottom: 1px solid var(--target);
  }

  td {
    padding: 3px 8px;
    border-bottom: 1px solid var(--line);
    white-space: nowrap;
    max-width: 22ch;
    overflow: hidden;
    text-overflow: ellipsis;
    cursor: text;
  }

  td.narrow {
    max-width: 7ch;
  }

  td.pick,
  th.pick {
    width: 28px;
    cursor: default;
  }

  td.pending {
    background: color-mix(in srgb, var(--target) 25%, transparent);
    outline: 1px solid var(--target);
    outline-offset: -1px;
  }

  input.cell {
    width: 100%;
    min-width: 8ch;
    padding: 1px 4px;
    font: inherit;
  }

  td.narrow input.cell {
    min-width: 0;
  }

  tr.chosen td {
    background: var(--card-bg, transparent);
  }

  tr.chosen td.pending {
    background: color-mix(in srgb, var(--target) 25%, transparent);
  }

  tr.unreadable td {
    opacity: 0.5;
  }

  .pad {
    padding: 6px 10px;
    margin: 0;
  }

  .find {
    display: flex;
    gap: 6px;
  }

  .find input {
    flex: 1;
    min-width: 0;
  }

  .tools {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 8px 10px;
    border-top: 1px solid var(--line);
  }

  .tool {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
  }

  .pattern {
    min-width: 16ch;
  }

  .check {
    display: inline-flex;
    flex-direction: row;
    align-items: center;
    gap: 6px;
  }

  .check input {
    width: auto;
    margin: 0;
    padding: 0;
  }

  .renames {
    padding: 8px 10px;
    border-top: 1px solid var(--target);
  }

  .renames ul,
  .changes,
  .history {
    list-style: none;
    margin: 6px 0;
    padding: 0;
    max-height: 30vh;
    overflow: auto;
  }

  .changes li,
  .history li {
    padding: 2px 0;
    border-bottom: 1px solid var(--line);
    overflow-wrap: anywhere;
  }

  .bad {
    color: var(--bad, #c0392b);
  }

  .side {
    display: flex;
    flex-direction: column;
    gap: 10px;
    min-width: 0;
  }

  .box {
    padding: 10px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    align-items: flex-start;
  }

  .box ul {
    align-self: stretch;
  }

  .file-name {
    overflow-wrap: anywhere;
  }

  .view {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
  }

  .view label {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }

  .grow {
    flex: 1;
  }

  .head {
    display: inline-flex;
    align-items: center;
    gap: 2px;
  }

  th button.sort,
  th button.drop {
    padding: 0;
    border: 0;
    background: none;
    box-shadow: none;
    font: inherit;
    font-weight: 700;
    color: inherit;
    white-space: nowrap;
  }

  .arrow {
    margin-left: 4px;
    font-size: 0.7em;
    color: var(--target);
  }

  th button.drop {
    width: 18px;
    color: var(--muted);
    opacity: 0;
  }

  th:hover button.drop,
  th button.drop:focus-visible {
    opacity: 1;
  }

  tr.group td {
    background: color-mix(in srgb, var(--target) 12%, transparent);
    border-bottom: 1px solid var(--target);
    cursor: default;
    max-width: none;
  }

  .lyrics-cell {
    cursor: default;
  }

  .ai-job {
    display: block;
    font-size: 0.75em;
    color: var(--target);
  }

  .online {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin: 0 10px 6px;
    border: 1px solid var(--target);
    border-radius: 8px;
  }

  .offer {
    padding: 6px 8px;
    border: 1px solid var(--target);
    border-radius: 8px;
  }

  .layout-backdrop {
    position: fixed;
    inset: 0;
    z-index: 40;
    background: rgb(0 0 0 / 0.6);
  }

  .layout-window {
    position: fixed;
    z-index: 41;
    inset: 24px;
    overflow: auto;
    border-radius: 14px;
    background: var(--page-bg, var(--surface));
    box-shadow: 0 0 24px var(--glow);
  }

  /* A short pane scrolls rather than spilling over what's below. */
  .grid {
    overflow: auto;
  }

  .cover-open {
    padding: 0;
    border: 0;
    background: none;
    box-shadow: none;
    cursor: zoom-in;
  }

  .imports {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding-bottom: 8px;
    margin-bottom: 8px;
    border-bottom: 1px solid var(--line);
  }

  .imports-head {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 6px;
  }

  .layout-choice {
    display: flex;
    flex-direction: column;
    gap: 4px;
    margin: 4px 0 8px;
    padding: 6px 8px;
    border: 1px solid var(--line);
    border-radius: 8px;
  }

  .layout-choice .check {
    align-items: flex-start;
  }

  .example {
    display: block;
    font-size: 0.85em;
  }

  .import-item.picked {
    outline: 1px solid var(--target);
  }

  /* Matched on Deezer, every song on its track: neon green, ready to approve. */
  .import-item.matched {
    --match: light-dark(#0b8a33, #39ff6a);
    color: var(--match);
    box-shadow: inset 3px 0 0 var(--match), 0 0 8px rgb(57 255 106 / 0.35);
  }

  .import-item.matched.picked {
    outline-color: var(--match);
  }

  .import-item .looking {
    opacity: 0.6;
    margin-right: 4px;
  }

  .approve-picked {
    border-color: light-dark(#0b8a33, #39ff6a);
  }

  .add label {
    display: flex;
    flex-direction: column;
    gap: 4px;
    align-self: stretch;
  }

  .row-buttons {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .review {
    border-color: var(--target);
    box-shadow: 0 0 10px var(--glow);
  }

</style>
