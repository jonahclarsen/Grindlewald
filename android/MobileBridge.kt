package com.jonahclarsen.grindlewald

import android.Manifest
import android.app.Activity
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.provider.Settings
import androidx.annotation.Keep

@Keep
object MobileBridge {
    init { System.loadLibrary("grindlewald_lib") }
    @JvmStatic external fun initialize()
    private lateinit var activity: Activity
    fun attach(value: Activity) {
        activity = value
        android.util.Log.i("GrindlewaldStartup", "Bluetooth bridge initializing")
        initialize()
        android.util.Log.i("GrindlewaldStartup", "Bluetooth bridge ready")
        GrindlewaldUpdater.attach(value)
    }
    @JvmStatic fun action(action: String): String {
        when (action) {
            "bluetooth" -> {
                val permissions = if (Build.VERSION.SDK_INT >= 31) arrayOf(Manifest.permission.BLUETOOTH_SCAN, Manifest.permission.BLUETOOTH_CONNECT)
                    else arrayOf(Manifest.permission.ACCESS_FINE_LOCATION)
                if (permissions.any { activity.checkSelfPermission(it) != PackageManager.PERMISSION_GRANTED }) {
                    activity.runOnUiThread { activity.requestPermissions(permissions, 71) }
                    return "Allow Nearby devices, then try again. You can also enable permission in Android Settings."
                }
            }
            "permissions" -> activity.runOnUiThread {
                activity.startActivity(Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.parse("package:" + activity.packageName)))
            }
            "update" -> return GrindlewaldUpdater.check(activity, true)
            "auto-update" -> return GrindlewaldUpdater.check(activity, false)
            "version" -> return activity.packageManager.getPackageInfo(activity.packageName, 0).versionName ?: ""
            else -> return "Unknown mobile action"
        }
        return ""
    }
    @JvmStatic fun copy(text: String): String {
        activity.runOnUiThread {
            (activity.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager).setPrimaryClip(ClipData.newPlainText("Grindlewald", text))
        }
        return ""
    }
}
