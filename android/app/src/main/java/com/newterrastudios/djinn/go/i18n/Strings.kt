package com.newterrastudios.djinn.go.i18n

import android.content.Context
import android.icu.text.PluralRules
import java.text.NumberFormat
import java.util.Locale
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.jsonObject

/**
 * Translations, shared with the web app: the same JSON files
 * (web/src/i18n/<lang>.json, copied into the app's assets at build time).
 * The phone's language when there's a file for it, English otherwise, and
 * the key itself as a last resort. `{name}` placeholders; an entry of
 * `{ "one": ..., "other": ... }` picks the plural form for `n`.
 */
object Strings {
    private var current: JsonObject? = null
    private var english: JsonObject? = null
    private var locale: Locale = Locale.ENGLISH

    /** Reads the files for the phone's language. Call when the app starts. */
    fun load(context: Context) {
        locale = Locale.getDefault()
        english = read(context, "en")
        current = read(context, locale.language) ?: english
    }

    /** Uses one catalog's text as English (for tests, which have no assets). */
    fun useEnglish(text: String) {
        locale = Locale.ENGLISH
        english = parse(text)
        current = english
    }

    private fun read(context: Context, code: String): JsonObject? = runCatching {
        context.assets.open("i18n/$code.json").bufferedReader().use { parse(it.readText()) }
    }.getOrNull()

    private fun parse(text: String): JsonObject = Json.parseToJsonElement(text).jsonObject

    fun t(key: String, vararg params: Pair<String, Any>): String {
        val entry = current?.get(key) ?: english?.get(key) ?: return key
        val text = when (entry) {
            is JsonObject -> {
                val n = (params.firstOrNull { it.first == "n" }?.second as? Number)?.toDouble() ?: 0.0
                val form = runCatching { PluralRules.forLocale(locale).select(n) }
                    .getOrDefault(if (n == 1.0) "one" else "other")
                val chosen = entry[form] ?: entry["other"]
                (chosen as? JsonPrimitive)?.content ?: key
            }
            is JsonPrimitive -> entry.content
            else -> key
        }
        return params.fold(text) { acc, (name, value) ->
            val shown = if (name == "n" && value is Number) NumberFormat.getInstance(locale).format(value) else value.toString()
            acc.replace("{$name}", shown)
        }
    }
}

/** Shorthand: `t("nav.library")`. */
fun t(key: String, vararg params: Pair<String, Any>): String = Strings.t(key, *params)
