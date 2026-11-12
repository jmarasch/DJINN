package com.newterrastudios.djinn.go.ui

import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.onClick
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.newterrastudios.djinn.go.data.Jump
import com.newterrastudios.djinn.go.i18n.t
import java.text.Normalizer

/** A place on the strip, and where in the list it goes. */
data class JumpEntry(val label: String, val offset: Int)

private const val LETTERS = "ABCDEFGHIJKLMNOPQRSTUVWXYZ"

/**
 * Where each letter starts in a list of names already in order, for lists the
 * server doesn't send jumps for. Accents are dropped, and anything that isn't a
 * letter goes under "#", as on the server.
 */
fun letterJumps(names: List<String>): List<Jump> {
    val found = LinkedHashMap<String, Int>()
    names.forEachIndexed { index, name ->
        val plain = Normalizer.normalize(name.trimStart().take(1), Normalizer.Form.NFD).take(1).uppercase()
        found.getOrPut(if (plain.isNotEmpty() && plain[0] in LETTERS) plain else "#") { index }
    }
    return found.map { (label, offset) -> Jump(label, offset) }
}

/**
 * Everything on the strip, top to bottom, for a list in this order. Letters and
 * years missing from the list go to the next one there is, so the strip always
 * looks the same. Empty for orders with no letters or years, like recently added.
 */
fun jumpEntries(jumps: List<Jump>, sort: String): List<JumpEntry> {
    if (jumps.isEmpty()) return emptyList()
    val at = jumps.associate { it.label to it.offset }
    val labels = if (sort == "year" || sort == "oldest") {
        val known = jumps.mapNotNull { it.label.toIntOrNull() }
        if (known.isEmpty()) return emptyList()
        val years = (known.min()..known.max()).map { it.toString() }
        if (sort == "year") years.reversed() else years
    } else {
        (if ("#" in at) listOf("#") else emptyList()) + LETTERS.map { it.toString() }
    }
    var next = jumps.last().offset
    return labels.asReversed().map { label ->
        next = at[label] ?: next
        JumpEntry(label, next)
    }.asReversed()
}

/**
 * Which entries get a label when not all of them fit: evenly spread letters, or
 * round years plus the first and last.
 */
fun labelledEntries(entries: List<JumpEntry>, room: Int): List<Int> {
    val count = entries.size
    val fits = room.coerceAtLeast(2)
    if (count <= fits) return entries.indices.toList()
    if (entries.first().label.toIntOrNull() == null) {
        val step = (count + fits - 1) / fits
        return entries.indices.filter { it % step == 0 }
    }
    val step = listOf(2, 5, 10, 20, 25, 50, 100).firstOrNull { count / it + 2 <= fits } ?: 100
    val gap = count.toFloat() / fits
    val picked = mutableListOf(0)
    for (i in 1 until count - 1) {
        val round = (entries[i].label.toIntOrNull() ?: 1) % step == 0
        if (round && i - picked.last() >= gap && count - 1 - i >= gap) picked.add(i)
    }
    picked.add(count - 1)
    return picked
}

/**
 * The letters or years beside a long list. Tap one or slide a finger along them
 * to jump; the whole height counts, so hidden entries can still be reached.
 */
@Composable
fun JumpStrip(entries: List<JumpEntry>, onPressed: (String?) -> Unit, onJump: (Int) -> Unit) {
    val jump by rememberUpdatedState(onJump)
    val pressedLabel by rememberUpdatedState(onPressed)
    var pressed by remember(entries) { mutableStateOf<Int?>(null) }
    val years = entries.first().label.toIntOrNull() != null
    // In sp, so larger text gets more room rather than being cut off.
    val density = LocalDensity.current
    val labelHeight = with(density) { 16.sp.toDp() }
    val stripWidth = with(density) { (if (years) 36.sp else 24.sp).toDp() }
    BoxWithConstraints(
        Modifier
            .fillMaxHeight()
            .width(stripWidth)
            .pointerInput(entries) {
                fun pick(y: Float) {
                    val index = (y / size.height * entries.size).toInt().coerceIn(0, entries.lastIndex)
                    if (index == pressed) return
                    pressed = index
                    pressedLabel(entries[index].label)
                    jump(entries[index].offset)
                }
                awaitEachGesture {
                    val down = awaitFirstDown()
                    down.consume()
                    pick(down.position.y)
                    while (true) {
                        val change = awaitPointerEvent().changes.firstOrNull { it.id == down.id } ?: break
                        if (!change.pressed) break
                        change.consume()
                        pick(change.position.y)
                    }
                    pressed = null
                    pressedLabel(null)
                }
            },
    ) {
        val room = (maxHeight / labelHeight).toInt()
        labelledEntries(entries, room).forEach { index ->
            val entry = entries[index]
            Box(
                Modifier
                    .fillMaxWidth()
                    .height(labelHeight)
                    .offset(y = maxHeight * ((index + 0.5f) / entries.size) - labelHeight / 2)
                    .semantics {
                        role = Role.Button
                        contentDescription = t("library.jump_to", "label" to entry.label)
                        onClick {
                            jump(entry.offset)
                            true
                        }
                    },
                contentAlignment = Alignment.Center,
            ) {
                Text(
                    entry.label,
                    style = MaterialTheme.typography.labelSmall,
                    maxLines = 1,
                    softWrap = false,
                    overflow = TextOverflow.Clip,
                    color = if (pressed == index) {
                        MaterialTheme.colorScheme.onSurface
                    } else {
                        MaterialTheme.colorScheme.primary
                    },
                )
            }
        }
    }
}

/** A finger on the strip covers the letter it's on, so this shows it. */
@Composable
fun JumpBubble(label: String, modifier: Modifier = Modifier) {
    Surface(
        modifier,
        shape = MaterialTheme.shapes.large,
        color = MaterialTheme.colorScheme.primaryContainer,
    ) {
        Text(
            label,
            style = MaterialTheme.typography.displaySmall,
            modifier = Modifier.padding(horizontal = 24.dp, vertical = 12.dp),
        )
    }
}
