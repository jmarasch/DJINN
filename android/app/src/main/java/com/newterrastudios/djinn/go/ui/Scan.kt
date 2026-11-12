package com.newterrastudios.djinn.go.ui

import android.annotation.SuppressLint
import com.newterrastudios.djinn.go.i18n.t
import android.view.ViewGroup
import androidx.camera.core.CameraSelector
import androidx.camera.core.ImageAnalysis
import androidx.camera.core.ImageProxy
import androidx.camera.core.Preview
import androidx.camera.lifecycle.ProcessCameraProvider
import androidx.camera.view.PreviewView
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalLifecycleOwner
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.core.content.ContextCompat
import com.google.zxing.BarcodeFormat
import com.google.zxing.BinaryBitmap
import com.google.zxing.DecodeHintType
import com.google.zxing.MultiFormatReader
import com.google.zxing.NotFoundException
import com.google.zxing.PlanarYUVLuminanceSource
import com.google.zxing.common.HybridBinarizer
import java.util.concurrent.Executors

/** Reads QR codes from camera frames. Plain Java: no Google services needed. */
class QrReader {
    private val reader = MultiFormatReader().apply {
        setHints(mapOf(DecodeHintType.POSSIBLE_FORMATS to listOf(BarcodeFormat.QR_CODE)))
    }

    /** The code's text, or null if there's no code in this picture. */
    fun read(luminance: ByteArray, width: Int, height: Int): String? {
        val source = PlanarYUVLuminanceSource(luminance, width, height, 0, 0, width, height, false)
        return try {
            reader.decodeWithState(BinaryBitmap(HybridBinarizer(source))).text
        } catch (_: NotFoundException) {
            null
        } finally {
            reader.reset()
        }
    }
}

private fun ImageProxy.brightness(): ByteArray {
    val plane = planes[0]
    val buffer = plane.buffer.duplicate()
    val out = ByteArray(width * height)
    if (plane.rowStride == width) {
        buffer.get(out, 0, out.size.coerceAtMost(buffer.remaining()))
    } else {
        for (row in 0 until height) {
            buffer.position(row * plane.rowStride)
            buffer.get(out, row * width, width)
        }
    }
    return out
}

/** Points the camera at the code GO shows on a computer. */
@SuppressLint("RestrictedApi")
@Composable
fun ScanScreen(onFound: (String) -> Unit, onCancel: () -> Unit) {
    val context = LocalContext.current
    val owner = LocalLifecycleOwner.current
    val executor = androidx.compose.runtime.remember { Executors.newSingleThreadExecutor() }
    val reader = androidx.compose.runtime.remember { QrReader() }
    DisposableEffect(Unit) {
        onDispose {
            ProcessCameraProvider.getInstance(context).addListener(
                { ProcessCameraProvider.getInstance(context).get().unbindAll() },
                ContextCompat.getMainExecutor(context),
            )
            executor.shutdown()
        }
    }
    Box(Modifier.fillMaxSize()) {
        AndroidView(
            modifier = Modifier.fillMaxSize(),
            factory = { ctx ->
                val view = PreviewView(ctx).apply {
                    layoutParams = ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT)
                }
                val future = ProcessCameraProvider.getInstance(ctx)
                future.addListener({
                    val provider = future.get()
                    val preview = Preview.Builder().build().also { it.surfaceProvider = view.surfaceProvider }
                    val analysis = ImageAnalysis.Builder()
                        .setBackpressureStrategy(ImageAnalysis.STRATEGY_KEEP_ONLY_LATEST)
                        .build()
                    var done = false
                    analysis.setAnalyzer(executor) { image ->
                        val text = if (done) null else reader.read(image.brightness(), image.width, image.height)
                        image.close()
                        if (text != null) {
                            done = true
                            ContextCompat.getMainExecutor(ctx).execute { onFound(text) }
                        }
                    }
                    val camera = if (provider.hasCamera(CameraSelector.DEFAULT_BACK_CAMERA)) {
                        CameraSelector.DEFAULT_BACK_CAMERA
                    } else {
                        CameraSelector.DEFAULT_FRONT_CAMERA
                    }
                    provider.unbindAll()
                    provider.bindToLifecycle(owner, camera, preview, analysis)
                }, ContextCompat.getMainExecutor(ctx))
                view
            },
        )
        Box(
            Modifier
                .fillMaxSize()
                .safeDrawingPadding()
                .padding(24.dp),
        ) {
            Text(
                t("scan.hint"),
                color = MaterialTheme.colorScheme.onPrimary,
                modifier = Modifier.align(Alignment.TopCenter),
            )
            Button(onClick = onCancel, modifier = Modifier.align(Alignment.BottomCenter)) { Text(t("common.cancel")) }
        }
    }
}
