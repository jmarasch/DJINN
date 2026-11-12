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
 * The language picked in Settings, else the server's default (Admin >
 * System), else the phone's, when there's a file for it; English otherwise,
 * and the key itself as a last resort. `{name}` placeholders; an entry of
 * `{ "one": ..., "other": ... }` picks the plural form for `n`.
 */
object Strings {
    private var current: JsonObject? = null
    private var english: JsonObject? = null
    private var locale: Locale = Locale.ENGLISH

    /** Reads the files for the language to show. Call when the app starts and when the choice changes. */
    fun load(context: Context) {
        val prefs = context.getSharedPreferences(
            com.newterrastudios.djinn.go.data.SETTINGS_FILE,
            Context.MODE_PRIVATE,
        )
        val chosen = prefs.getString(com.newterrastudios.djinn.go.data.LANGUAGE_KEY, "").orEmpty()
        val house = prefs.getString(com.newterrastudios.djinn.go.data.HOUSE_LANGUAGE_KEY, "").orEmpty()
        english = read(context, "en")
        val phone = Locale.getDefault()
        val wanted = listOf(chosen, house).firstOrNull { it.isNotEmpty() && read(context, it) != null }
        locale = wanted?.let { Locale.forLanguageTag(it) } ?: phone
        current = read(context, wanted ?: phone.language) ?: english
    }

    /** The languages there are files for: code and the language's own name. */
    fun available(context: Context): List<Pair<String, String>> =
        context.assets.list("i18n").orEmpty()
            .filter { it.endsWith(".json") }
            .map { it.removeSuffix(".json") }
            .sorted()
            .map { code ->
                code to ((read(context, code)?.get("language.name") as? JsonPrimitive)?.content ?: code)
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
