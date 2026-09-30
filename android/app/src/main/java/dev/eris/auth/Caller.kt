package dev.eris.auth

import android.content.Context
import android.content.pm.PackageManager
import java.security.MessageDigest
import org.json.JSONObject

/** The app asking, as Android names it: its package and the certificate it is signed with. */
data class Caller(val packageName: String?, val certificate: String?, val pid: Int?) {
    fun json(): String = JSONObject()
        .put("package", packageName ?: JSONObject.NULL)
        .put("certificate", certificate ?: JSONObject.NULL)
        .put("pid", pid ?: JSONObject.NULL)
        .toString()

    companion object {
        /** The hex of a certificate's SHA-256, as `apksigner` and the Play console show it. */
        fun fingerprint(certificate: ByteArray): String =
            MessageDigest.getInstance("SHA-256").digest(certificate).joinToString("") { "%02x".format(it) }

        /** Who holds [uid]. A oneway call carries no pid, which Android reports as 0. */
        fun of(context: Context, uid: Int, pid: Int): Caller {
            val pm = context.packageManager
            val name = pm.getPackagesForUid(uid)?.firstOrNull()
            val certificate = name?.let {
                runCatching {
                    pm.getPackageInfo(it, PackageManager.GET_SIGNING_CERTIFICATES)
                        .signingInfo?.apkContentsSigners?.firstOrNull()?.toByteArray()?.let(::fingerprint)
                }.getOrNull()
            }
            return Caller(name, certificate, pid.takeIf { it > 0 })
        }
    }
}
