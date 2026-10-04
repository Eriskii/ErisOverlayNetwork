package dev.eris.auth

import org.json.JSONArray
import org.json.JSONObject

// What the daemon tells the app's screens, and the little arithmetic over
// grants that decides what the person sees. Pure, so it runs under JVM tests.

/**
 * True when [grant] covers [required], as ErisLogin decides it: segment by segment, `*` matches
 * one segment, and as a grant's last segment it matches every remaining one.
 */
fun covers(grant: String, required: String): Boolean {
    val grants = grant.split(':')
    val wanted = required.split(':')
    for (i in grants.indices) {
        if (i >= wanted.size) return false
        if (grants[i] == "*" && i == grants.size - 1) return true
        if (grants[i] != "*" && grants[i] != wanted[i]) return false
    }
    return wanted.size == grants.size
}

/** True when every grant in [child] is covered by some grant in [parent]. */
fun encloses(parent: List<String>, child: List<String>): Boolean =
    child.all { wanted -> parent.any { covers(it, wanted) } }

/** True when approving [granted] takes more than this device holds, so the passphrase. */
fun needsPassphrase(device: List<String>, granted: List<String>): Boolean = !encloses(device, granted)

/** A ceiling as typed: grants separated by commas or spaces; nothing typed means everything. */
fun parseCeiling(typed: String): List<String> =
    typed.split(Regex("[\\s,]+")).filter { it.isNotEmpty() }.ifEmpty { listOf("*") }

/** Time left until [expires] (Unix seconds) as m:ss. */
fun remaining(expires: Long, now: Long): String {
    val left = maxOf(0L, expires - now)
    return "${left / 60}:${(left % 60).toString().padStart(2, '0')}"
}

/** The button that approves [picked] of the [of] grants the person may choose among. */
fun approveLabel(picked: Int, of: Int): String = if (picked == of) "Approve" else "Approve $picked of $of"

private fun JSONArray?.strings(): List<String> = if (this == null) emptyList() else List(length()) { getString(it) }

private fun JSONObject.text(name: String): String? = if (isNull(name)) null else optString(name).ifEmpty { null }

/** A request waiting for the person: an app on this phone, or a device joining. */
data class Approval(
    val id: String,
    val isDevice: Boolean,
    val service: String,
    val client: String,
    val requested: List<String>,
    /** For a device: what makes it an ErisAuth device, granted or refused whole. */
    val essential: List<String>,
    /** For a device: the code both screens show, to compare. */
    val fingerprint: String?,
    val identityKind: String?,
    val identityKey: String?,
    /** The app asking, as Android names it, and the SHA-256 of its signing certificate. */
    val packageName: String?,
    val certificate: String?,
    /** On a computer: the program asking. */
    val executable: String?,
    val pid: Long?,
    val expires: Long,
) {
    /** The grants the person may leave out. */
    val optional: List<String> get() = requested.filter { it !in essential }

    /** What approving gives, with [chosen] of the optional grants. */
    fun granted(chosen: Set<String>): List<String> = requested.filter { it in essential || it in chosen }

    companion object {
        fun parse(json: JSONObject): Approval {
            val identity = json.optJSONObject("identity")
            return Approval(
                id = json.getString("id"),
                isDevice = json.optString("kind") == "device",
                service = json.optString("service"),
                client = json.optString("client"),
                requested = json.optJSONArray("requested").strings(),
                essential = json.optJSONArray("essential").strings(),
                fingerprint = json.text("fingerprint"),
                identityKind = identity?.optString("kind"),
                identityKey = identity?.optString("key"),
                packageName = json.text("package"),
                certificate = json.text("certificate"),
                executable = json.text("executable"),
                pid = if (json.isNull("pid") || !json.has("pid")) null else json.optLong("pid"),
                expires = json.optLong("expires"),
            )
        }
    }
}

/** A join started from this phone, until it ends. */
data class Joining(val fingerprint: String?, val error: String?)

/** This phone's ErisAuth as its screens show it. */
data class DeviceState(
    val name: String?,
    val joined: Boolean,
    val grants: List<String>,
    val approvals: List<Approval>,
    val joining: Joining?,
) {
    /** What this device approves for apps by itself: its grants without the fixed rights every device holds. */
    val approvesItself: List<String>
        get() = grants.filter { !it.startsWith("meta:") && !it.startsWith("erisauth:") }

    companion object {
        fun parse(json: JSONObject): DeviceState {
            val device = json.optJSONObject("device") ?: JSONObject()
            val approvals = json.optJSONArray("approvals")
            val joining = json.optJSONObject("joining")
            return DeviceState(
                name = device.text("name"),
                joined = device.optBoolean("joined"),
                grants = device.optJSONArray("grants").strings(),
                approvals = if (approvals == null) emptyList() else List(approvals.length()) { Approval.parse(approvals.getJSONObject(it)) },
                joining = joining?.let { Joining(it.text("fingerprint"), it.text("error")) },
            )
        }
    }
}

/** A QR code as the daemon lays it out: [side] modules a side, quiet zone included. */
class Qr(val side: Int, private val modules: String) {
    fun dark(x: Int, y: Int): Boolean = modules[y * side + x] == '1'

    companion object {
        fun parse(json: JSONObject): Qr = Qr(json.getInt("side"), json.getString("modules"))
    }
}

/** What the person was doing with the passphrase. */
enum class PassphraseAction { CHANGE, RESET, UNLOCK }

/** What went wrong with a passphrase request, in words; empty when nothing did. */
fun passphraseProblem(outcome: JSONObject, action: PassphraseAction): String {
    if (outcome.optBoolean("ok")) return ""
    return when (outcome.optString("error")) {
        "wrong_passphrase" -> "That is not the current passphrase."
        "forbidden" ->
            if (action == PassphraseAction.UNLOCK) "This device may not unlock devices."
            else "This device may not set the passphrase without the current one."
        "locked" -> "Too many wrong passphrases from this device; it may try again in ${remaining(outcome.optLong("retry_after"), 0)}."
        "no_passphrase" -> "There is no passphrase yet; leave the current one empty to set the first."
        else -> outcome.text("message") ?: outcome.text("error") ?: "ErisAuth could not do that."
    }
}

/** Why an answer to a request did not go through, in words. */
fun refusalOf(outcome: JSONObject): String = when (outcome.optString("error")) {
    "needs_passphrase" -> "This takes the passphrase."
    "wrong_passphrase" -> "That is not the passphrase."
    "locked" -> "Too many wrong passphrases from this device. Try again in ${remaining(outcome.optLong("retry_after"), 0)}."
    else -> outcome.text("message") ?: outcome.text("error") ?: "ErisAuth refused."
}
