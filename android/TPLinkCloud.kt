package com.jonahclarsen.grindlewald

import android.content.Context
import org.json.JSONObject
import java.io.File
import java.io.IOException
import java.net.HttpURLConnection
import java.net.URL
import java.net.URLEncoder
import java.util.UUID

/** Controls the floodlight plugs through TP-Link's Kasa cloud, the same API the Mac's script uses. */
object TPLinkCloud {
    private const val HOST = "https://wap.tplinkcloud.com"
    // Plugs whose name contains this are floodlights, matching the Mac's script.
    private const val MATCH = "light"
    private const val TOKEN_EXPIRED = -20651

    /** Account data lives in no-backup app storage, so it never leaves this phone. */
    private fun file(context: Context) = File(context.noBackupFilesDir, "tplink.json")
    private fun load(context: Context) = file(context).takeIf { it.isFile }?.let { JSONObject(it.readText()) }
    private fun store(context: Context, value: JSONObject?) {
        if (value == null) file(context).delete() else file(context).writeText(value.toString())
    }

    private fun post(url: String, body: JSONObject, terminal: String, token: String? = null): JSONObject {
        val params = linkedMapOf("appName" to "Kasa_Android", "termID" to terminal, "appVer" to "1.4.4.607",
            "ospf" to "Android+6.0.1", "netType" to "wifi", "locale" to "en_US")
        if (token != null) params["token"] = token
        val query = params.entries.joinToString("&") { "${it.key}=${URLEncoder.encode(it.value, "UTF-8")}" }
        val connection = URL("$url?$query").openConnection() as HttpURLConnection
        try {
            connection.requestMethod = "POST"
            connection.connectTimeout = 8000
            connection.readTimeout = 10000
            connection.doOutput = true
            connection.setRequestProperty("Content-Type", "application/json")
            connection.setRequestProperty("User-Agent", "Dalvik/2.1.0 (Linux; U; Android 6.0.1; A0001 Build/M4B30X)")
            connection.outputStream.use { it.write(body.toString().toByteArray()) }
            if (connection.responseCode != 200) throw IOException("TP-Link returned ${connection.responseCode}")
            return JSONObject(connection.inputStream.bufferedReader().use { it.readText() })
        } finally {
            connection.disconnect()
        }
    }

    private class CloudError(val code: Int, message: String) : IOException(message)

    private fun result(response: JSONObject): JSONObject {
        val code = response.optInt("error_code", -1)
        if (code != 0) throw CloudError(code, response.optString("msg").ifEmpty { "TP-Link error $code" })
        return response.optJSONObject("result") ?: JSONObject()
    }

    private fun login(email: String, password: String, terminal: String): String {
        val body = JSONObject().put("method", "login").put("url", HOST).put("params", JSONObject()
            .put("appType", "Kasa_Android").put("cloudUserName", email)
            .put("cloudPassword", password).put("terminalUUID", terminal))
        return result(post(HOST, body, terminal)).optString("token").ifEmpty { throw IOException("TP-Link sign-in failed") }
    }

    /** Returns the signed-in email, or "" when signed out. */
    fun account(context: Context, request: JSONObject): String {
        when {
            request.optBoolean("signOut") -> store(context, null)
            request.has("email") -> {
                val email = request.getString("email").trim()
                val password = request.getString("password")
                val terminal = load(context)?.optString("terminal")?.ifEmpty { null } ?: UUID.randomUUID().toString()
                val token = login(email, password, terminal)
                store(context, JSONObject().put("email", email).put("password", password)
                    .put("terminal", terminal).put("token", token))
            }
        }
        return load(context)?.optString("email") ?: ""
    }

    fun floodlights(context: Context, request: JSONObject): String {
        val account = load(context) ?: throw IOException("Sign in to TP-Link in Settings")
        val on = request.getBoolean("on")
        val offAfter = request.optInt("offAfterSeconds", 0).takeIf { on && it > 0 }
        val terminal = account.getString("terminal")
        var token = account.getString("token")
        var renewed = false
        fun <T> withToken(call: (String) -> T): T = try {
            call(token)
        } catch (error: CloudError) {
            if (renewed || error.code != TOKEN_EXPIRED) throw error
            // Tokens expire; sign in again once with the saved account.
            renewed = true
            token = login(account.getString("email"), account.getString("password"), terminal)
            store(context, account.put("token", token))
            call(token)
        }
        val devices = withToken { result(post(HOST, JSONObject().put("method", "getDeviceList"), terminal, it)) }
            .optJSONArray("deviceList") ?: throw IOException("TP-Link returned no devices")
        val plugs = (0 until devices.length()).map { devices.getJSONObject(it) }
            .filter { it.optString("alias").lowercase().contains(MATCH) }
        if (plugs.isEmpty()) throw IOException("No TP-Link plug has “$MATCH” in its name")
        for (plug in plugs) {
            fun send(data: JSONObject): JSONObject {
                val body = JSONObject().put("method", "passthrough").put("params", JSONObject()
                    .put("deviceId", plug.getString("deviceId")).put("requestData", data.toString()))
                val url = plug.optString("appServerUrl").ifEmpty { HOST }
                val response = withToken { result(post(url, body, terminal, it)) }
                return JSONObject(response.optString("responseData").ifEmpty { "{}" })
            }
            fun countdown(command: String, value: Any) =
                send(JSONObject().put("count_down", JSONObject().put(command, value)))
            val name = plug.optString("alias")
            try {
                // Any new command replaces a pending timed off, like on the Mac.
                if (offAfter == null) runCatching { countdown("delete_all_rules", JSONObject()) }
                else {
                    countdown("delete_all_rules", JSONObject())
                    val added = countdown("add_rule", JSONObject().put("enable", 1).put("delay", offAfter)
                        .put("act", 0).put("name", "Grindlewald off"))
                    val code = added.optJSONObject("count_down")?.optJSONObject("add_rule")?.optInt("err_code", -1)
                    if (code != 0) throw IOException("$name has no timer")
                }
                send(JSONObject().put("system", JSONObject().put("set_relay_state", JSONObject().put("state", if (on) 1 else 0))))
            } catch (error: CloudError) {
                throw IOException("$name: ${error.message}")
            }
        }
        return when {
            offAfter != null -> "Floodlights on for ${offAfter / 60} min"
            else -> "Floodlights ${if (on) "on" else "off"}"
        }
    }
}
