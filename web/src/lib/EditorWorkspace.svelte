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
  import { onMount } from 'svelte'
  import { ApiError, api } from './api'
  import AlbumLayout from './AlbumLayout.svelte'
  import Art from './Art.svelte'
  import CoverWindow from './CoverWindow.svelte'
  import { COMMON_TAGS, MORE_TAGS, NUMBER_TAGS, tagLabel } from './editorTags'
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
  /** An import being worked on (its number), instead of a library folder. */
  let importing = $state<number | null>(null)
  /** The grid edits files: the library's, or an import's (waiting, not in the library yet). */
  const editingFiles = $derived(target === 'library' || importing !== null)
  let imports = $state<EditorImport[]>([])
  /** Files on their way into a new import: done of all. */
  let uploading = $state<{ done: number; of: number } | null>(null)
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
  $effect(() => {
    if (layoutBox) layoutBox.scrollIntoView({ behavior: 'smooth', block: 'start' })
  })

  const FIELDS: { key: EditorCoreField; label: string; narrow?: boolean }[] = [
    { key: 'disc_no', label: 'Disc', narrow: true },
    { key: 'track_no', label: '#', narrow: true },
    { key: 'title', label: 'Title' },
    { key: 'artist', label: 'Artist' },
    { key: 'album', label: 'Album' },
    { key: 'album_artist', label: 'Album artist' },
    { key: 'year', label: 'Year', narrow: true },
    { key: 'genre', label: 'Genre' },
  ]
  const LABEL: Record<string, string> = Object.fromEntries(FIELDS.map((f) => [f.key, f.label]))
  LABEL.track_no = 'Track'
  /** A field's name in lists: its column's, or the other tag's. */
  const labelOf = (field: EditorField) => (field.startsWith('x:') ? tagLabel(field.slice(2)) : (LABEL[field] ?? field))

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

  // --- Sorting and grouping. Rows sort by the values as they are (not the
  // changes waiting), so a row doesn't jump away while it's being edited.
  // With no column chosen they come folder by folder, by disc and track.
  type SortField = EditorField | 'file'
  let sortBy = $state<{ field: SortField; desc: boolean } | null>(null)
  let groupBy = $state<'' | 'disc_no' | 'album' | 'folder'>('')
  const GROUPS: { key: typeof groupBy; label: string }[] = [
    { key: '', label: 'Nothing' },
    { key: 'disc_no', label: 'Disc' },
    { key: 'album', label: 'Album' },
    { key: 'folder', label: 'Folder' },
  ]
  const collator = new Intl.Collator(undefined, { numeric: true, sensitivity: 'base' })
  const folderOf = (song: EditorSong) => {
    const rel = song.rel_path || song.path
    return rel.slice(0, Math.max(rel.lastIndexOf('/'), rel.lastIndexOf('\\')))
  }

  function sortOn(field: SortField) {
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
    return compareValues(original(a, field), original(b, field), isNumber(field), desc)
  }

  function groupKey(song: EditorSong): string {
    if (groupBy === 'disc_no') return original(song, 'disc_no') || '1'
    if (groupBy === 'album') return original(song, 'album')
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
            groupBy === 'disc_no' ? `Disc ${key}` : groupBy === 'album' ? key || 'No album' : key || 'The library folder',
          songs: [song],
        })
    }
    return out
  })

  let coverOpen = $state(false)

  const n = (x: number) => x.toLocaleString()

  async function loadRoots() {
    try {
      roots = (await api.editorTree('')).folders
      locked = false
    } catch (e) {
      if (e instanceof ApiError && e.status === 423) locked = true
      else toast(e instanceof Error ? e.message : 'Could not load', true)
    }
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
        toast(e instanceof Error ? e.message : 'Could not open it', true)
        return
      }
    }
    open = { ...open, [node.path]: true }
  }

  const changeCount = $derived(Object.values(pending).reduce((sum, f) => sum + Object.keys(f).length, 0))

  function leaveChanges(): boolean {
    return changeCount === 0 || confirm(`Throw away ${changeCount} changes that weren't approved?`)
  }

  async function choose(node: EditorNode) {
    if (!leaveChanges()) return
    pending = {}
    searching = ''
    importing = null
    addPlan = null
    selected = node.path
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
    addPlan = null
    searching = clean
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
      toast(e instanceof Error ? e.message : 'Could not load the songs', true)
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
    if ((to === target && importing === null) || !leaveChanges()) return
    pending = {}
    if (importing !== null) {
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

  async function loadImports() {
    try {
      imports = await api.editorImports()
    } catch {
      imports = []
    }
  }

  async function openImport(batch: number) {
    if (importing === batch || !leaveChanges()) return
    pending = {}
    selected = ''
    searching = ''
    importing = batch
    chosen = null
    picked = {}
    renames = null
    addPlan = null
    if (!addFolder) addFolder = roots.find((r) => r.editable)?.path ?? ''
    await loadSongs(0)
  }

  const AUDIO = /\.(mp3|flac|m4a|mp4|aac|ogg|oga|wav)$/i

  /** Brings song files in as a new import, one at a time, then opens it. */
  async function importFiles(list: FileList | null) {
    const files = [...(list ?? [])].filter((f) => AUDIO.test(f.name))
    const left = (list?.length ?? 0) - files.length
    if (!files.length) {
      toast('No song files there', true)
      return
    }
    if (!leaveChanges()) return
    let batch: number
    try {
      batch = (await unlocked(() => api.editorImportStart())).batch
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not start the import', true)
      return
    }
    // In track order as far as names go (disc folders, then numbers).
    files.sort((a, b) => collator.compare(a.webkitRelativePath || a.name, b.webkitRelativePath || b.name))
    uploading = { done: 0, of: files.length }
    const problems: string[] = []
    for (const file of files) {
      try {
        await unlocked(() => api.editorImportUpload(batch, file))
      } catch (e) {
        problems.push(`${file.name}: ${e instanceof Error ? e.message : 'failed'}`)
      }
      uploading = { done: uploading.done + 1, of: files.length }
    }
    uploading = null
    await loadImports()
    const done = files.length - problems.length
    toast(
      `${n(done)} songs brought in` +
        (left ? `; ${n(left)} other files left out` : '') +
        (problems.length ? `; ${problems.length} couldn't be: ${problems[0]}` : ''),
      problems.length > 0,
    )
    if (done) {
      pending = {}
      importing = null
      await openImport(batch)
    }
  }

  async function throwAwayImport() {
    if (importing === null) return
    if (!confirm(`Throw this import away? Its ${songs.length} files are deleted; the library isn't touched.`)) return
    try {
      await unlocked(() => api.editorImportThrowAway(importing as number))
      pending = {}
      importing = null
      songs = []
      total = 0
      chosen = null
      addPlan = null
      await loadImports()
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not throw it away', true)
    }
  }

  /** Takes the picked songs out of the import (deleting the waiting copies). */
  async function dropPicked() {
    const list = pickedSongs
    if (!list.length || !confirm(`Take ${list.length} songs out of this import? Their files are deleted.`)) return
    for (const song of list) {
      try {
        await unlocked(() => api.editorImportDropFile(song.id))
      } catch (e) {
        toast(e instanceof Error ? e.message : 'Could not take it out', true)
      }
    }
    picked = {}
    addPlan = null
    await loadImports()
    await loadSongs(0)
  }

  /** Where Add to the library would put the import's songs (or, with
   * `preview` false, puts them there and opens that folder). */
  async function addToLibrary(preview: boolean) {
    if (importing === null || !addFolder) return
    if (changeCount) {
      toast('Approve or throw away the changes first, so the songs go in as you see them', true)
      return
    }
    const ids = pickedSongs.map((s) => s.id)
    try {
      if (preview) {
        addPlan = (await unlocked(() => api.editorImportAdd(importing as number, addFolder, addPattern, true, ids))).files
        return
      }
      const going = addPlan?.filter((p) => !p.problem).length ?? 0
      if (!going || !confirm(`Add ${going} songs to the library? This can be undone from the history (Library side).`))
        return
      busy = true
      const done = await unlocked(() => api.editorImportAdd(importing as number, addFolder, addPattern, false, ids))
      const added = done.files.filter((f) => !f.problem)
      const failed = done.files.filter((f) => f.problem)
      toast(
        `${n(added.length)} songs added to the library` +
          (failed.length ? `; ${failed.length} weren't: ${failed[0].problem}` : ''),
        failed.length > 0,
      )
      addPlan = failed.length ? failed : null
      await loadImports()
      await loadHistory()
      const first = added[0]?.to
      if (first && !imports.some((i) => i.batch === importing)) {
        // Show them where they went, in the Library view.
        importing = null
        target = 'library'
        selected = first.slice(0, Math.max(first.lastIndexOf('/'), first.lastIndexOf('\\')))
        chosen = null
        picked = {}
        await loadSongs(0)
      } else {
        await loadSongs(0)
      }
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not add them', true)
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
      const multiDisc = list.suggested.some((t) => t.disc_no > 1)
      let set = 0
      for (const t of list.suggested) {
        const song = songs.find((s) => s.id === t.id)
        if (!song || !rowEditable(song)) continue
        setPending(song, 'track_no', String(t.track_no))
        if (multiDisc || shown(song, 'disc_no')) setPending(song, 'disc_no', String(t.disc_no))
        set++
      }
      toast(
        `"${list.release}" on MusicBrainz (${list.tracks} tracks): ${set} songs numbered` +
          (list.unmatched ? `, ${list.unmatched} with no matching track` : '') +
          '. Check them under Changes.',
      )
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not ask MusicBrainz', true)
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
    toast(changed ? `${changed} titles cleaned up; check them under Changes` : 'Nothing to clean up')
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
        song: song?.title ?? `Song ${id}`,
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
      !confirm(`Write ${changes.length} changes into ${songCount} music files? This can be undone from the history.`)
    )
      return
    busy = true
    try {
      const done = await unlocked(() =>
        importing !== null ? api.editorImportApply(changes) : api.editorApply(target, changes),
      )
      addPlan = null
      pending = {}
      report(done.songs, done.failed, target === 'library' ? 'files written' : 'songs changed')
      await loadSongs(0)
      if (target === 'library') await loadHistory()
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not approve', true)
    } finally {
      busy = false
    }
  }

  function report(count: number, failed: { id: number; error: string }[], what: string) {
    if (failed.length) {
      const first = failed[0]
      const name = songs.find((s) => s.id === first.id)?.title ?? `song ${first.id}`
      toast(`${n(count)} ${what}; ${failed.length} couldn't be: ${name}: ${first.error}`, true)
    } else {
      toast(`${n(count)} ${what}`)
    }
  }

  async function undo(batch: EditorBatch) {
    if (!confirm(`Put back what approval ${batch.batch} changed in ${batch.files} files?`)) return
    busy = true
    try {
      const done = await unlocked(() => api.editorUndo(batch.batch))
      report(done.songs, done.failed, 'files put back')
      await loadSongs(0)
      await loadHistory()
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not undo', true)
    } finally {
      busy = false
    }
  }

  /** The songs a cover goes to: those picked, else the chosen song's album here. */
  const coverSongsNow = $derived(
    (pickedSongs.length
      ? pickedSongs
      : chosen
        ? songs.filter((s) => (chosen?.album_id != null ? s.album_id === chosen.album_id : s.id === chosen?.id))
        : []
    ).filter((s) => target === 'database' || s.editable),
  )

  /** The album layout for the picked songs, or the chosen song's album. */
  function openLayout() {
    const list = pickedSongs.length
      ? pickedSongs
      : songs.filter((s) => (chosen?.album_id != null ? s.album_id === chosen.album_id : s.id === chosen?.id))
    if (list.length) layoutSongs = list
  }

  async function useLayout(changes: { song: EditorSong; field: EditorField; value: string }[], cover: Blob | null) {
    const list = layoutSongs ?? []
    let skipped = 0
    for (const c of changes) {
      if (rowEditable(c.song)) setPending(c.song, c.field, c.value)
      else skipped++
    }
    layoutSongs = null
    toast(
      `Changes from the release are waiting under Preview and approve${skipped ? `; ${skipped} left out (read only folders)` : ''}`,
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
        report(done.songs, done.failed, 'waiting files changed')
        await loadSongs(0)
      } catch (e) {
        toast(e instanceof Error ? e.message : 'Could not change the cover', true)
      } finally {
        busy = false
        if (coverInput) coverInput.value = ''
      }
      return
    }
    const where = target === 'library' ? 'into the files of' : 'for'
    const words = how === 'set' ? 'Use this cover' : how === 'remove' ? 'Take the cover out' : "Go back to the files' own cover"
    if (!confirm(`${words} ${where} ${coverSongs.length} songs?`)) return
    busy = true
    try {
      const done = await unlocked(() => api.editorCover(target, coverSongs.map((s) => s.id), file, how))
      report(done.songs, done.failed, target === 'library' ? 'files written' : 'songs changed')
      await loadSongs(0)
      if (target === 'library') await loadHistory()
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not change the cover', true)
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
    if (!confirm(`Put ${file.name} in place of ${fileName(song.path)}? The song keeps its plays, likes and playlists; the old file is kept so this can be undone.`))
      return
    busy = true
    try {
      await unlocked(() => api.editorReplace(song.id, file))
      toast('File replaced; it will be analyzed again')
      await loadSongs(0)
      await loadHistory()
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not replace it', true)
    } finally {
      busy = false
      if (replaceInput) replaceInput.value = ''
    }
  }

  async function makeFolderArt() {
    busy = true
    try {
      const done = await unlocked(() => api.editorFolderArt(coverSongsNow.map((s) => s.id), true))
      toast(done.failed.length ? `${done.songs} written; ${done.failed[0].error}` : `folder.jpg written in ${done.songs} folders`)
      await loadHistory()
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not write it', true)
    } finally {
      busy = false
    }
  }

  async function previewMove(kind: MoveKind) {
    if (kind === 'rename' && changeCount && !confirm('Rename from the tags as they are now? Changes not approved yet are left out.'))
      return
    if ((kind === 'move' || kind === 'copy') && !moveTo.trim()) return
    try {
      moveKind = kind
      const found = (await unlocked(() => relocation(kind, changeable.map((s) => s.id), true))).files
      // Only the MP3s that need it are worth listing.
      renames = kind === 'mp3' ? found.filter((f) => !f.problem) : found
      if (kind === 'mp3' && !renames.length) {
        renames = null
        toast('Every MP3 here shows its real length already')
      }
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not work out where they go', true)
    }
  }

  const renamable = $derived(renames?.filter((r) => !r.problem) ?? [])
  const MOVE_WORDS: Record<MoveKind, { title: string; verb: string; done: string }> = {
    rename: { title: 'New file names', verb: 'Rename', done: 'renamed' },
    move: { title: 'New places', verb: 'Move', done: 'moved' },
    copy: { title: 'Copies to make (each becomes a song of its own)', verb: 'Copy', done: 'copied' },
    delete: {
      title: 'Delete (into the trash in their library folder; undo from the history until the trash is emptied)',
      verb: 'Delete',
      done: 'deleted',
    },
    mp3: {
      title: 'MP3s other players show the wrong length for (DJINN counts it already)',
      verb: 'Fix',
      done: 'fixed',
    },
  }

  async function rename() {
    if (!renamable.length) return
    const words = MOVE_WORDS[moveKind]
    if (!confirm(`${words.verb} ${renamable.length} files on disk? This can be undone from the history.`)) return
    busy = true
    try {
      const done = await unlocked(() => relocation(moveKind, renamable.map((r) => r.id), false))
      const moved = done.files.filter((f) => !f.problem).length
      toast(`${n(moved)} files ${words.done}`)
      renames = null
      await loadSongs(0)
      await loadHistory()
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not rename', true)
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
  const KINDS: Record<string, string> = {
    tags: 'tags',
    cover: 'cover',
    rename: 'names and places',
    delete: 'deleted',
    copy: 'copies',
    folder: 'empty folders',
    'folder-art': 'folder.jpg',
    'mp3-length': 'MP3 lengths',
    replace: 'replaced file',
  }
  const kindName = (kinds: string) =>
    kinds.startsWith('undo') ? 'undo' : kinds.split(',').map((k) => KINDS[k] ?? k).join(', ')
</script>

{#snippet branch(nodes: EditorNode[], depth: number)}
  <ul class="tree" style:--depth={depth}>
    {#each nodes as node (node.path)}
      <li>
        <div class="node" class:picked={selected === node.path}>
          {#if node.has_folders}
            <button class="twisty" aria-label={open[node.path] ? 'Close' : 'Open'} onclick={() => toggle(node)}>
              {open[node.path] ? '−' : '+'}
            </button>
          {:else}
            <span class="twisty" aria-hidden="true"></span>
          {/if}
          <button class="label" onclick={() => choose(node)} title={node.path}>
            {node.name}
            <span class="count">{n(node.songs)}</span>
            {#if node.editable === true}<span class="mode on">read and write</span>{:else if node.editable === false}<span class="mode">read only</span>{/if}
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
    <p class="small">The editor is locked. <button class="link" onclick={unlock}>Unlock</button></p>
  </section>
{:else}
  <div class="editor" class:library={editingFiles} class:fill={windowed}>
    <div class="top">
      <div class="toggle" role="group" aria-label="What you are editing">
        <button class:on={target === 'library' && importing === null} onclick={() => setTarget('library')}>Library</button>
        <button class:on={target === 'database' && importing === null} onclick={() => setTarget('database')}>Database</button>
        {#if importing !== null}<button class="on">Import</button>{/if}
      </div>
      <p class="small muted">
        {#if importing !== null}
          Working on an import: songs waiting in DJINN's own folder, not in the library yet. Approving writes the tags
          into them; Add to the library (on the right) moves them into a music folder.
        {:else if target === 'library'}
          Editing the tags in the music files themselves. Only folders you allowed can be changed, and every approval
          can be undone from the history.
        {:else}
          Editing DJINN's own copy of the tags. The files are left alone. Clearing a cell goes back to the file's value.
        {/if}
      </p>
    </div>

    <div class="panes">
      <aside class="folders">
        <div class="imports">
          <div class="imports-head">
            <strong class="small">Imports</strong>
            <input
              type="file"
              multiple
              hidden
              accept=".mp3,.flac,.m4a,.mp4,.aac,.ogg,.oga,.wav"
              bind:this={importInput}
              onchange={(e) => {
                void importFiles(e.currentTarget.files)
                e.currentTarget.value = ''
              }}
            />
            <input
              type="file"
              hidden
              webkitdirectory
              bind:this={importFolderInput}
              onchange={(e) => {
                void importFiles(e.currentTarget.files)
                e.currentTarget.value = ''
              }}
            />
            <button class="small-button" disabled={!!uploading} onclick={() => importInput?.click()} title="Choose an album's song files"
              >Files...</button
            >
            <button
              class="small-button"
              disabled={!!uploading}
              onclick={() => importFolderInput?.click()}
              title="Choose an album's folder (song files in it and its folders; anything else is left out)">Folder...</button
            >
          </div>
          {#if uploading}
            <p class="small muted">Bringing in {uploading.done + 1 > uploading.of ? uploading.of : uploading.done + 1} of {uploading.of}...</p>
          {/if}
          {#each imports as imp (imp.batch)}
            <button class="label import-item" class:picked={importing === imp.batch} onclick={() => openImport(imp.batch)}>
              {imp.album || `Import ${imp.batch}`}
              <span class="count">{n(imp.files)}</span>
              {#if imp.artist}<span class="mode">{imp.artist}</span>{/if}
            </button>
          {:else}
            {#if !uploading}<p class="small muted">Bring in a new album to work on before it goes into the library.</p>{/if}
          {/each}
        </div>
        {#if roots.length}
          {@render branch(roots, 0)}
        {:else}
          <p class="small muted">No library folders.</p>
        {/if}
      </aside>

      <section class="grid">
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
            placeholder="Find songs by artist, title or album (a | b finds either)"
            aria-label="Find songs"
          />
          <button class="small-button" type="submit" disabled={!searchText.trim()}>Find</button>
        </form>
        {#if !selected && !searching}
          <p class="small muted pad">Choose a folder on the left, or find songs, to see them here.</p>
        {:else}
          <p class="small muted pad">
            {#if searching}Found {n(total)} songs for <b>{searching}</b>{total >= 1000 ? ' (the first 1,000)' : ''}{:else}{n(
                songs.length,
              )} of {n(total)} songs{/if}{pickedSongs.length ? ` · ${n(pickedSongs.length)} picked` : ''}
            {#if target === 'library' && !canEdit}· <strong>read only</strong>: changes aren't allowed in
              {searching ? "these songs' folders" : 'this folder'} (make it read and write under Library folders){:else if target ===
              'library' && searching}· songs in read only folders are dimmed{:else}· click a cell to change it{/if}
          </p>
          <div class="view pad">
            <label class="small"
              >Group by
              <select bind:value={groupBy}>
                {#each GROUPS as g (g.key)}<option value={g.key}>{g.label}</option>{/each}
              </select>
            </label>
            {#if sortBy}
              <button class="link small" onclick={() => (sortBy = null)}>Back to folder order</button>
            {/if}
            <span class="grow"></span>
            <select bind:value={addPick} aria-label="Column to add">
              <option value="">Add a column...</option>
              {#if columns.hidden.length}
                <optgroup label="Columns taken off">
                  {#each FIELDS.filter((f) => columns.hidden.includes(f.key)) as f (f.key)}<option value={f.key}>{f.label}</option>{/each}
                </optgroup>
              {/if}
              {#if editingFiles && inFiles.length}
                <optgroup label="In these files">
                  {#each inFiles as name (name)}<option value="x:{name}">{tagLabel(name)}</option>{/each}
                </optgroup>
              {/if}
              <optgroup label="Common tags">
                {#each notShown(COMMON_TAGS) as [name, label] (name)}<option value="x:{name}">{label}</option>{/each}
              </optgroup>
              <optgroup label="More tags">
                {#each notShown(MORE_TAGS) as [name, label] (name)}<option value="x:{name}">{label}</option>{/each}
              </optgroup>
            </select>
            <button class="small-button" disabled={!addPick} onclick={addColumn}>Add</button>
          </div>
          {#if !editingFiles && columns.extra.length}
            <p class="small muted pad">
              Your other tag columns ({columns.extra.map(tagLabel).join(', ')}) show on the Library side: those tags are
              only in the files.
            </p>
          {/if}
          <div class="scroll">
            <table>
              <thead>
                <tr>
                  <th class="pick">
                    <input type="checkbox" aria-label="Pick every song" checked={allPicked} onchange={(e) => pickAll(e.currentTarget.checked)} />
                  </th>
                  {#each shownColumns as col (col.key)}
                    <th aria-sort={sortBy?.field === col.key ? (sortBy.desc ? 'descending' : 'ascending') : undefined}>
                      <span class="head">
                        <button class="sort" title="Sort by {col.label}" onclick={() => sortOn(col.key)}>
                          {col.label}{#if sortBy?.field === col.key}<span class="arrow">{sortBy.desc ? '▼' : '▲'}</span>{/if}
                        </button>
                        {#if col.key !== 'title'}
                          <button class="drop" aria-label="Take the {col.label} column off" title="Take this column off" onclick={() => removeColumn(col.key)}
                            >×</button
                          >
                        {/if}
                      </span>
                    </th>
                  {/each}
                  <th aria-sort={sortBy?.field === 'file' ? (sortBy.desc ? 'descending' : 'ascending') : undefined}>
                    <button class="sort" title="Sort by file name" onclick={() => sortOn('file')}>
                      File{#if sortBy?.field === 'file'}<span class="arrow">{sortBy.desc ? '▼' : '▲'}</span>{/if}
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
                          aria-label="Pick every song in {group.label}"
                          checked={group.songs.every((s) => picked[s.id])}
                          onchange={(e) => pickGroup(group.songs, e.currentTarget.checked)}
                        />
                      </td>
                      <td colspan={shownColumns.length + 1}>
                        <strong>{group.label}</strong> <span class="muted">· {n(group.songs.length)} songs</span>
                      </td>
                    </tr>
                  {/if}
                {#each group.songs as song (song.id)}
                  <tr
                    class:chosen={chosen?.id === song.id}
                    class:unreadable={target === 'library' && importing === null && !rowEditable(song)}
                    title={target === 'library' && importing === null && !song.editable ? 'Read only folder' : undefined}
                  >
                    <td class="pick">
                      <input
                        type="checkbox"
                        aria-label="Pick {song.title}"
                        checked={!!picked[song.id]}
                        onclick={(e) => pick(song, position.get(song.id) ?? 0, e)}
                      />
                    </td>
                    {#each shownColumns as f (f.key)}
                      <td
                        class:narrow={f.narrow}
                        class:pending={isPending(song, f.key)}
                        title={isPending(song, f.key) ? `Was: ${original(song, f.key) || '(empty)'}` : undefined}
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
                    <td class="mono muted" title={song.rel_path}>{fileName(song.path)}</td>
                  </tr>
                {/each}
                {/each}
              </tbody>
            </table>
          </div>
          {#if songs.length < total && !searching}
            <button class="small-button pad" disabled={loading} onclick={() => loadSongs(songs.length)}>Show more</button>
          {/if}

          {#if canEdit && songs.length}
            <div class="tools">
              <div class="tool">
                <span class="small muted"
                  >{pickedSongs.length ? `For the ${changeable.length} picked songs` : `For every song shown (${changeable.length})`}:</span
                >
              </div>
              <div class="tool">
                <select bind:value={fillField} aria-label="Field to set">
                  {#each shownColumns as f (f.key)}<option value={f.key}>{labelOf(f.key)}</option>{/each}
                </select>
                <input bind:value={fillValue} placeholder="Value (empty clears it)" aria-label="Value" />
                <button class="small-button" onclick={fill}>Set</button>
              </div>
              <div class="tool">
                <button class="small-button" onclick={numberInOrder} title="Track numbers 1, 2, 3... in the grid's order, for each album and disc">
                  Number in order
                </button>
                <button
                  class="small-button"
                  disabled={!chosen && !pickedSongs.length}
                  onclick={openLayout}
                  title="Compare the album with its releases on MusicBrainz (or search MusicBrainz, Deezer or Apple), lay the songs on the right tracks and take the tags and cover"
                >
                  Album layout...
                </button>
                <button
                  class="small-button"
                  disabled={chosen?.album_id == null}
                  onclick={numbersFromMusicBrainz}
                  title="Track and disc numbers for the chosen song's album, from its track list on MusicBrainz, matched by title"
                >
                  Numbers from MusicBrainz
                </button>
                <button
                  class="small-button"
                  onclick={cleanTitles}
                  title="Takes off a leading track number and leftovers like (Official Video) or [HD]"
                >
                  Clean up titles
                </button>
              </div>
              {#if target === 'library' && importing === null}
                <div class="tool">
                  <input bind:value={pattern} aria-label="File name pattern" class="pattern" />
                  <button class="small-button" onclick={() => previewMove('rename')}>Rename files...</button>
                  <span class="small muted"
                    >{'{disc} {track} {title} {artist} {album} {album_artist} {year}'}; a / makes folders from the library
                    folder, like {'{album_artist}/{album}/{track} {title}'}</span
                  >
                </div>
                <div class="tool">
                  <input bind:value={moveTo} aria-label="Folder to move or copy to" class="pattern" placeholder="Folder to move or copy to" />
                  <button class="small-button" onclick={() => previewMove('move')} disabled={!moveTo.trim()}>Move...</button>
                  <button class="small-button" onclick={() => previewMove('copy')} disabled={!moveTo.trim()}>Copy...</button>
                  <button
                    class="small-button"
                    onclick={() => previewMove('delete')}
                    title="Into the trash in their library folder; undo from the history until the trash is emptied (Library folders)"
                  >
                    Delete...
                  </button>
                  <button
                    class="small-button"
                    onclick={() => previewMove('mp3')}
                    title="Variable bit rate MP3s with no length header look several times too long in other players. This writes the header (as LAME does); the sound isn't touched."
                  >
                    Fix MP3 lengths...
                  </button>
                </div>
                <div class="tool">
                  <label class="check small"><input type="checkbox" bind:checked={removeEmpty} /> Remove folders left with no songs</label>
                  <label class="check small"><input type="checkbox" bind:checked={folderArt} /> Make folder.jpg in new folders</label>
                </div>
              {/if}
            </div>
          {/if}

          {#if renames}
            <div class="renames">
              <div>
                <strong>{MOVE_WORDS[moveKind].title}</strong>
                <span class="small muted">({renamable.length} to {MOVE_WORDS[moveKind].verb.toLowerCase()})</span>
              </div>
              {#if moveKind === 'rename'}
                <p class="small muted">Names come from the tags as DJINN shows them, with its corrections.</p>
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
                {MOVE_WORDS[moveKind].verb} {renamable.length} files
              </button>
              <button onclick={() => (renames = null)}>Cancel</button>
            </div>
          {/if}
        {/if}
      </section>

      <aside class="side">
        <div class="box art">
          <div class="small muted">Album art</div>
          {#if chosen}
            <button class="cover-open" title="Open the cover: bigger, and search for another online" onclick={() => (coverOpen = true)}>
              <Art hash={chosen.art_hash} size={200} round={8} />
            </button>
            <div class="small">{chosen.album ?? ''}</div>
          {:else}
            <p class="small muted">Choose a song to see its album's cover.</p>
          {/if}
        </div>
        <div class="box">
          <div class="small muted">
            Cover {coverSongsNow.length ? `for ${coverSongsNow.length} ${pickedSongs.length ? 'picked songs' : 'songs of this album'}` : ''}
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
            Replace cover
          </button>
          <button class="small-button" disabled={!canEdit || busy || !coverSongsNow.length} onclick={() => setCover('remove')}>
            Remove cover
          </button>
          {#if importing !== null}
            <!-- An import's covers are in its files; nothing else to go back to. -->
          {:else if target === 'database'}
            <button class="small-button" disabled={busy || !coverSongsNow.length} onclick={() => setCover('reset')}>
              Use the files' cover
            </button>
          {:else}
            <button
              class="small-button"
              disabled={busy || !coverSongsNow.length}
              title="Saves the album's cover as folder.jpg in the songs' folders (replacing one that's there; undo from the history)"
              onclick={makeFolderArt}
            >
              Make folder.jpg
            </button>
          {/if}
        </div>
        {#if importing !== null}
          <div class="box add">
            <div><strong>Add to the library</strong></div>
            <label class="small"
              >Into
              <select bind:value={addFolder} onchange={() => (addPlan = null)}>
                {#each roots.filter((r) => r.editable) as root (root.path)}<option value={root.path}>{root.name}</option>{/each}
              </select>
            </label>
            {#if !roots.some((r) => r.editable)}
              <p class="small bad">No library folder allows changes yet. Allow one under Library folders first.</p>
            {/if}
            <label class="small"
              >Placed as
              <input bind:value={addPattern} class="pattern" oninput={() => (addPlan = null)} aria-label="Where each song goes" />
            </label>
            <span class="small muted">{'{disc} {track} {title} {artist} {album} {album_artist} {year}'}; a / makes folders</span>
            <p class="small muted">
              {pickedSongs.length ? `The ${pickedSongs.length} picked songs` : `All ${songs.length} songs`} go in, named from their
              tags.
            </p>
            <button class="small-button" disabled={busy || !addFolder} onclick={() => addToLibrary(true)}>Preview...</button>
            {#if addPlan}
              <ul class="changes">
                {#each addPlan as p (p.id)}
                  {@const [, to] = afterShared(p.from, p.to)}
                  <li class="small" class:bad={!!p.problem}>
                    <span class="mono">{to}</span>{#if p.problem}<em> ({p.problem})</em>{/if}
                  </li>
                {/each}
              </ul>
              <button class="primary" disabled={busy || !addPlan.some((p) => !p.problem)} onclick={() => addToLibrary(false)}>
                Add {addPlan.filter((p) => !p.problem).length} songs to the library
              </button>
            {/if}
            <div class="row-buttons">
              <button class="small-button" disabled={busy || !pickedSongs.length} onclick={dropPicked}>Take picked out</button>
              <button class="small-button danger" disabled={busy} onclick={throwAwayImport}>Throw this import away</button>
            </div>
          </div>
        {/if}
        {#if target === 'library' && importing === null && chosen}
          <div class="box">
            <div class="small muted">This song's file</div>
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
              title="For a song that won't play or a poor copy: puts a better file in its place"
              onclick={() => replaceInput?.click()}
            >
              Replace the file...
            </button>
          </div>
        {/if}
        <div class="box review">
          <div><strong>Preview and approve changes</strong></div>
          {#if review.length}
            <p class="small muted">
              {n(review.length)} changes to {n(Object.keys(pending).length)} songs, going
              {importing !== null
                ? 'into the waiting files'
                : target === 'library'
                  ? 'into the files'
                  : "into DJINN's database"}:
            </p>
            <ul class="changes">
              {#each review.slice(0, 200) as c (`${c.id}-${c.field}`)}
                <li class="small">
                  <span class="muted">{c.song} · {labelOf(c.field)}:</span>
                  <s>{c.from || '(empty)'}</s> → <b>{c.to || '(empty)'}</b>
                  <button class="link" aria-label="Leave this one" onclick={() => undoOne(c.id, c.field)}>×</button>
                </li>
              {/each}
              {#if review.length > 200}<li class="small muted">and {n(review.length - 200)} more</li>{/if}
            </ul>
            {#if blocked}<p class="small bad">{blocked} songs with changes are in read only folders.</p>{/if}
            <button class="primary" disabled={busy || !canEdit || blocked > 0} onclick={approve}>
              Approve {n(review.length)} changes
            </button>
            <button class="small-button" disabled={busy} onclick={() => (pending = {})}>Throw them away</button>
          {:else}
            <p class="small muted">No changes yet. Click a cell to change it, or use the tools under the grid.</p>
            <button class="primary" disabled>Approve changes</button>
          {/if}
        </div>
        {#if target === 'library' && importing === null}
          <div class="box">
            <div class="small muted">Changes made to files</div>
            {#if history.length}
              <ul class="history">
                {#each history.slice(0, 12) as b (b.batch)}
                  <li class="small">
                    {new Date(b.made_at).toLocaleString()} · {kindName(b.kinds)} · {n(b.files)} files{b.made_by
                      ? ` · ${b.made_by}`
                      : ''}
                    {#if b.undone_at}
                      <span class="muted">(undone)</span>
                    {:else if b.final_at}
                      <span class="muted">(trash emptied)</span>
                    {:else if !b.kinds.startsWith('undo')}
                      <button class="link" disabled={busy} onclick={() => undo(b)}>Undo</button>
                    {/if}
                  </li>
                {/each}
              </ul>
            {:else}
              <p class="small muted">None yet.</p>
            {/if}
          </div>
        {/if}
      </aside>
    </div>
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
      <div bind:this={layoutBox}>
        <AlbumLayout songs={layoutSongs} onapply={useLayout} onclose={() => (layoutSongs = null)} />
      </div>
    {/if}
  </div>
{/if}

<style>
  .editor {
    container-type: inline-size;
    --target: var(--accent);
    border: 2px solid var(--target);
    border-radius: 12px;
    padding: 12px;
    box-shadow: 0 0 14px var(--glow);
  }

  .editor.library {
    --target: #e8a317;
    box-shadow: 0 0 14px rgb(232 163 23 / 0.4);
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

  .pick input {
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
    gap: 6px;
  }

  .imports-head strong {
    flex: 1;
  }

  .import-item.picked {
    outline: 1px solid var(--target);
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
