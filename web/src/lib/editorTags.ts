// The other tags a music file can carry, as the library editor offers them
// for columns of their own. The names are the server's (lofty's tag names,
// `write::other_tag_key`); the labels are what the editor shows. The common
// ones come first in the menu; the rest follow under "More tags". The
// labels are English; translations are `tag.<name>` in the language files.
import { t } from './i18n.svelte'

export const COMMON_TAGS: [string, string][] = [
  ['Composer', 'Composer'],
  ['Comment', 'Comment'],
  ['Lyricist', 'Lyricist'],
  ['Label', 'Label'],
  ['Publisher', 'Publisher'],
  ['Conductor', 'Conductor'],
  ['Producer', 'Producer'],
  ['Remixer', 'Remixer'],
  ['Bpm', 'BPM'],
  ['InitialKey', 'Key'],
  ['Mood', 'Mood'],
  ['TrackTotal', 'Total tracks'],
  ['DiscTotal', 'Total discs'],
  ['FlagCompilation', 'Compilation (1 or 0)'],
  ['Isrc', 'ISRC'],
  ['Barcode', 'Barcode'],
  ['CatalogNumber', 'Catalog number'],
  ['ReleaseCountry', 'Release country'],
  ['CopyrightMessage', 'Copyright'],
  ['Language', 'Language'],
  ['EncodedBy', 'Encoded by'],
  ['ContentGroup', 'Grouping'],
  ['SetSubtitle', 'Disc subtitle'],
  ['TrackSubtitle', 'Subtitle'],
]

export const MORE_TAGS: [string, string][] = [
  ['AcoustId', 'AcoustID'],
  ['AlbumArtistSortOrder', 'Album artist sort'],
  ['AlbumTitleSortOrder', 'Album sort'],
  ['AppleXid', 'Apple ID'],
  ['Arranger', 'Arranger'],
  ['AudioFileUrl', 'Audio file web page'],
  ['AudioSourceUrl', 'Audio source web page'],
  ['Color', 'Color'],
  ['CommercialInformationUrl', 'Commercial information web page'],
  ['ComposerSortOrder', 'Composer sort'],
  ['CopyrightUrl', 'Copyright web page'],
  ['Description', 'Description'],
  ['Director', 'Director'],
  ['EncoderSettings', 'Encoder settings'],
  ['EncoderSoftware', 'Encoder software'],
  ['EncodingTime', 'Encoding time'],
  ['Engineer', 'Engineer'],
  ['FileOwner', 'File owner'],
  ['FlagPodcast', 'Podcast (1 or 0)'],
  ['IntegerBpm', 'BPM (whole number)'],
  ['InternetRadioStationName', 'Radio station name'],
  ['InternetRadioStationOwner', 'Radio station owner'],
  ['License', 'License'],
  ['MixDj', 'Mix DJ'],
  ['MixEngineer', 'Mix engineer'],
  ['Movement', 'Movement'],
  ['MovementNumber', 'Movement number'],
  ['MovementTotal', 'Total movements'],
  ['MusicBrainzArtistId', 'MusicBrainz artist ID'],
  ['MusicBrainzRecordingId', 'MusicBrainz recording ID'],
  ['MusicBrainzReleaseArtistId', 'MusicBrainz album artist ID'],
  ['MusicBrainzReleaseGroupId', 'MusicBrainz release group ID'],
  ['MusicBrainzReleaseId', 'MusicBrainz release ID'],
  ['MusicBrainzReleaseType', 'MusicBrainz release type'],
  ['MusicBrainzTrackId', 'MusicBrainz track ID'],
  ['MusicBrainzWorkId', 'MusicBrainz work ID'],
  ['OriginalAlbumTitle', 'Original album'],
  ['OriginalArtist', 'Original artist'],
  ['OriginalFileName', 'Original file name'],
  ['OriginalLyricist', 'Original lyricist'],
  ['OriginalMediaType', 'Original media'],
  ['ParentalAdvisory', 'Parental advisory'],
  ['PaymentUrl', 'Payment web page'],
  ['Performer', 'Performer'],
  ['PodcastDescription', 'Podcast description'],
  ['PodcastGlobalUniqueId', 'Podcast episode ID'],
  ['PodcastKeywords', 'Podcast keywords'],
  ['PodcastSeriesCategory', 'Podcast category'],
  ['PodcastUrl', 'Podcast web page'],
  ['PublisherUrl', 'Publisher web page'],
  ['R128AlbumGain', 'R128 album gain'],
  ['R128TrackGain', 'R128 track gain'],
  ['RadioStationUrl', 'Radio station web page'],
  ['ReplayGainAlbumGain', 'ReplayGain album gain'],
  ['ReplayGainAlbumPeak', 'ReplayGain album peak'],
  ['ReplayGainTrackGain', 'ReplayGain track gain'],
  ['ReplayGainTrackPeak', 'ReplayGain track peak'],
  ['Script', 'Script'],
  ['ShowName', 'Show name'],
  ['ShowNameSortOrder', 'Show name sort'],
  ['TaggingTime', 'Tagging time'],
  ['TrackArtistSortOrder', 'Artist sort'],
  ['TrackArtistUrl', 'Artist web page'],
  ['TrackTitleSortOrder', 'Title sort'],
  ['Work', 'Work'],
  ['Writer', 'Writer'],
]

const LABELS = new Map([...COMMON_TAGS, ...MORE_TAGS])

/** A tag's label in the chosen language (`tag.<name>`), else its English
 * one, else its name with spaces between the words. */
export function tagLabel(name: string): string {
  const key = `tag.${name.toLowerCase()}`
  const translated = t(key)
  if (translated !== key) return translated
  return LABELS.get(name) ?? name.replace(/([a-z])([A-Z])/g, '$1 $2')
}

/** Tags whose values are numbers, so they sort as numbers. */
export const NUMBER_TAGS = new Set([
  'Bpm',
  'IntegerBpm',
  'TrackTotal',
  'DiscTotal',
  'MovementNumber',
  'MovementTotal',
  'FlagCompilation',
  'FlagPodcast',
])
