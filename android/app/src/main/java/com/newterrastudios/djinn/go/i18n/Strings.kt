package com.newterrastudios.djinn.go.i18n

import android.content.Context
import android.icu.text.PluralRules
import org.json.JSONObject
import java.text.NumberFormat
import java.util.Locale

/**
 * Translations, shared with the web app: the same JSON files
 * (web/src/i18n/<lang>.json, copied into the app's assets at build time).
 * The phone's language when there's a file for it, English otherwise, and
 * the key itself as a last resort. `{name}` placeholders; an entry of
 * `{ "one": ..., "other": ... }` picks the plural form for `n`.
 */
object Strings {
    private var current: JSONObject? = null
    private var english: JSONObject? = null
    private var locale: Locale = Locale.ENGLISH

    /** Reads the files for the phone's language. Call when the app starts. */
    fun load(context: Context) {
        locale = Locale.getDefault()
        english = read(context, "en")
        current = read(context, locale.language) ?: english
    }

    private fun read(context: Context, code: String): JSONObject? = runCatching {
        context.assets.open("i18n/$code.json").bufferedReader().use { JSONObject(it.readText()) }
    }.getOrNull()

    fun t(key: String, vararg params: Pair<String, Any>): String {
        val entry = current?.opt(key) ?: english?.opt(key) ?: return key
        val text = when (entry) {
            is JSONObject -> {
                val n = (params.firstOrNull { it.first == "n" }?.second as? Number)?.toDouble() ?: 0.0
                val form = runCatching { PluralRules.forLocale(locale).select(n) }.getOrDefault("other")
                entry.optString(form).ifEmpty { entry.optString("other", key) }
            }
            else -> entry.toString()
        }
        return params.fold(text) { acc, (name, value) ->
            val shown = if (name == "n" && value is Number) NumberFormat.getInstance(locale).format(value) else value.toString()
            acc.replace("{$name}", shown)
        }
    }
}

/** Shorthand: `t("nav.library")`. */
fun t(key: String, vararg params: Pair<String, Any>): String = Strings.t(key, *params)
