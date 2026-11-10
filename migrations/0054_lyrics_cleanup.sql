-- Plain lyrics read from songs' tags or .lrc files that still hold web page
-- leftovers (scripts, HTML tags, wiki templates, entities, lyrics sites'
-- excerpts) are read again, now cleaned as they're read.
UPDATE lyrics SET file_mtime = NULL
WHERE source IN ('file', 'lrc')
  AND synced IS NULL
  AND (plain LIKE '%<%>%'
    OR plain LIKE '%{{%}}%'
    OR plain LIKE '%&amp;%'
    OR plain LIKE '%&quot;%'
    OR plain LIKE '%&#%;%'
    OR plain LIKE '%not licensed to display the full lyrics%');
