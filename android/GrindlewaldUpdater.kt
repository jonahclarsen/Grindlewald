package com.jonahclarsen.grindlewald

import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.pm.ApplicationInfo
import android.content.pm.PackageInstaller
import android.os.Build
import android.util.Log
import org.json.JSONObject
import java.io.File
import java.net.HttpURLConnection
import java.net.URL
import java.util.concurrent.atomic.AtomicBoolean

/** Android's installer verifies the APK signing identity and performs atomic replacement. */
object GrindlewaldUpdater {
    private val busy = AtomicBoolean(false)
    private const val REPOSITORY = "https://api.github.com/repos/jonahclarsen/Grindlewald/releases/latest"
    private const val DOWNLOAD = "https://github.com/jonahclarsen/Grindlewald/releases/download/"
    private const val APK = "grindlewald-arm64.apk"
    fun attach(context: Context) {
        val marker = File(context.filesDir, "updater-ci-selftest")
        if (context.applicationInfo.flags and ApplicationInfo.FLAG_DEBUGGABLE != 0 && marker.delete()) {
            Thread {
                Thread.sleep(5000)
                // A genuinely newer signed fixture, served by CI over the same download path.
                install(context, "http://127.0.0.1:39920/update.apk", true)
            }.start()
        }
    }
    fun check(context: Context, manual: Boolean): String {
        val prefs = context.getSharedPreferences("updates", Context.MODE_PRIVATE)
        if (!manual && System.currentTimeMillis() - prefs.getLong("checked", 0) < 6 * 60 * 60 * 1000) return ""
        if (!busy.compareAndSet(false, true)) return "An update is already running"
        try {
            val connection = URL(REPOSITORY).openConnection() as HttpURLConnection
            connection.connectTimeout = 15000; connection.readTimeout = 30000
            connection.setRequestProperty("Accept", "application/vnd.github+json")
            val release = try {
                check(connection.responseCode == 200) { "Could not check GitHub releases" }
                JSONObject(connection.inputStream.bufferedReader().use { it.readText() })
            } finally { connection.disconnect() }
            val tag = release.getString("tag_name")
            check(Regex("^v[0-9]+\\.[0-9]+\\.[0-9]+$").matches(tag)) { "Unexpected release version" }
            val parts = tag.drop(1).split('.').map { it.toLong() }
            val code = parts[0] * 1000000 + parts[1] * 1000 + parts[2]
            val installed = context.packageManager.getPackageInfo(context.packageName, 0)
            val current = if (Build.VERSION.SDK_INT >= 28) installed.longVersionCode else installed.versionCode.toLong()
            prefs.edit().putLong("checked", System.currentTimeMillis()).apply()
            if (code <= current) return "Up to date"
            val assets = release.getJSONArray("assets")
            check((0 until assets.length()).any { assets.getJSONObject(it).getString("name") == APK }) { "The Android update is not ready yet" }
            busy.set(false)
            return install(context, "$DOWNLOAD$tag/$APK", false)
        } catch (error: Exception) {
            return error.message ?: "Could not check for updates"
        } finally { busy.set(false) }
    }
    private fun install(app: Context, source: String, selfTest: Boolean): String {
        if (!busy.compareAndSet(false, true)) return "An update is already running"
        val installer = app.packageManager.packageInstaller
        var sessionId = -1
        try {
            val params = PackageInstaller.SessionParams(PackageInstaller.SessionParams.MODE_FULL_INSTALL)
            params.setAppPackageName(app.packageName)
            if (Build.VERSION.SDK_INT >= 31) params.setRequireUserAction(PackageInstaller.SessionParams.USER_ACTION_NOT_REQUIRED)
            sessionId = installer.createSession(params)
            installer.openSession(sessionId).use { session ->
                val connection = URL(source).openConnection() as HttpURLConnection
                connection.connectTimeout = 15000; connection.readTimeout = 30000
                try {
                    check(connection.responseCode == 200) { "Update download failed" }
                    connection.inputStream.use { input ->
                        session.openWrite(APK, 0, connection.contentLengthLong).use { output ->
                            input.copyTo(output, 65536); session.fsync(output)
                        }
                    }
                } finally { connection.disconnect() }
                val flags = PendingIntent.FLAG_UPDATE_CURRENT or if (Build.VERSION.SDK_INT >= 31) PendingIntent.FLAG_MUTABLE else 0
                val status = PendingIntent.getBroadcast(app, sessionId,
                    Intent(app, GrindlewaldUpdateReceiver::class.java).putExtra("selfTest", selfTest), flags)
                session.commit(status.intentSender)
            }
            return "Installing update…"
        } catch (error: Exception) {
            if (sessionId >= 0) try { installer.abandonSession(sessionId) } catch (_: Exception) {}
            if (selfTest) Log.e("GrindlewaldUpdate", "UPDATE_E2E_FAIL", error)
            return error.message ?: "Update failed"
        } finally { busy.set(false) }
    }
}
class GrindlewaldUpdateReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        when (intent.getIntExtra(PackageInstaller.EXTRA_STATUS, PackageInstaller.STATUS_FAILURE)) {
            PackageInstaller.STATUS_PENDING_USER_ACTION -> {
                @Suppress("DEPRECATION") val confirm = intent.getParcelableExtra<Intent>(Intent.EXTRA_INTENT)
                if (intent.getBooleanExtra("selfTest", false)) Log.e("GrindlewaldUpdate", "UPDATE_E2E_CONFIRMATION")
                confirm?.let { context.startActivity(it.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)) }
            }
            PackageInstaller.STATUS_SUCCESS -> Log.i("GrindlewaldUpdate", "UPDATE_INSTALLED")
            else -> {
                val message = intent.getStringExtra(PackageInstaller.EXTRA_STATUS_MESSAGE) ?: "Update failed"
                context.getSharedPreferences("updates", Context.MODE_PRIVATE).edit().putString("error", message).apply()
                Log.e("GrindlewaldUpdate", "UPDATE_FAILED: $message")
            }
        }
    }
}
