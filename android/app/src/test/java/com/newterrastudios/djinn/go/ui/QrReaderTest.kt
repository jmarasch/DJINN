package com.newterrastudios.djinn.go.ui

import com.google.zxing.BarcodeFormat
import com.google.zxing.qrcode.QRCodeWriter
import com.newterrastudios.djinn.go.data.serverAddress
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class QrReaderTest {
    @Test
    fun `reads the address from a code like Admin GO's`() {
        val size = 300
        val matrix = QRCodeWriter().encode("http://192.168.1.20:8067", BarcodeFormat.QR_CODE, size, size)
        val picture = ByteArray(size * size) { i ->
            if (matrix.get(i % size, i / size)) 0 else -1
        }
        val text = QrReader().read(picture, size, size)
        assertEquals("http://192.168.1.20:8067", text)
        assertEquals("http://192.168.1.20:8067", serverAddress(text!!))
    }

    @Test
    fun `no code means no answer, and the reader keeps working`() {
        val reader = QrReader()
        assertNull(reader.read(ByteArray(200 * 200) { -1 }, 200, 200))
        assertNull(reader.read(ByteArray(200 * 200) { (it % 7).toByte() }, 200, 200))
    }
}
