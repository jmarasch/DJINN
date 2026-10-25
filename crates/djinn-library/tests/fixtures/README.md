# Library fixtures

`tagged.flac`: a short tone with Vorbis comments (title, artist, album
artist, album, track 3/10, disc 1/1, date 1997-09-22, genre) and an
embedded 16x16 front cover. Made from `../../djinn-engine/tests/fixtures/gapless_b.flac`:

```sh
ffmpeg -f lavfi -i "color=c=purple:s=16x16" -frames:v 1 cover.jpg
ffmpeg -i gapless_b.flac -i cover.jpg -map 0:a -map 1:v -c copy \
  -disposition:v attached_pic -metadata:s:v comment="Cover (front)" \
  -metadata title="Jóga" -metadata artist="Björk" -metadata album_artist="Björk" \
  -metadata album="Homogenic" -metadata track="3/10" -metadata disc="1/1" \
  -metadata date="1997-09-22" -metadata genre="Electronic" tagged.flac
```
