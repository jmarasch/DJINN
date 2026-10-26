// The TV page runs on TVs with Chromecast built in, whose browsers can be
// years old (Chrome 84 on a 2016 Vizio). The build rewrites newer syntax;
// these fill in the few newer functions the app relies on.

if (!String.prototype.replaceAll) {
  Object.defineProperty(String.prototype, 'replaceAll', {
    configurable: true,
    writable: true,
    value: function (this: string, search: string | RegExp, replacement: string) {
      if (search instanceof RegExp) return this.replace(search, replacement)
      if (typeof replacement === 'string') return this.split(search).join(replacement)
      const escaped = search.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
      return this.replace(new RegExp(escaped, 'g'), replacement)
    },
  })
}
