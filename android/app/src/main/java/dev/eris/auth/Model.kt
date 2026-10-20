package dev.eris.auth

import org.json.JSONArray
import org.json.JSONObject

// What the daemon tells the app's screens, read from its JSON, and the words the person reads.
// Pure, so it runs under JVM tests.

private fun JSONArray?.strings(): List<String> = if (this == null) emptyList() else List(length()) { getString(it) }

private fun <T> JSONArray?.objects(read: (JSONObject) -> T): List<T> =
    if (this == null) emptyList() else List(length()) { read(getJSONObject(it)) }

private fun JSONObject.text(name: String): String? = if (!has(name) || isNull(name)) null else optString(name).ifEmpty { null }

private fun JSONObject.number(name: String): Long? = if (!has(name) || isNull(name)) null else optLong(name)

private fun JSONObject.flag(name: String): Boolean? = if (!has(name) || isNull(name)) null else optBoolean(name)

private fun JSONObject.child(name: String): JSONObject? = if (!has(name) || isNull(name)) null else optJSONObject(name)

/** A service as the person reads its name. */
fun serviceName(service: String?): String = when (service) {
    null -> "a service"
    "erisdb" -> "ErisDB"
    else -> service
}

/** A device in the mesh. */
data class Device(
    val id: String,
    val name: String,
    val addedBy: String?,
    val addedByName: String?,
    val addedAt: Long,
    val isThis: Boolean,
    /** Whether it answered when last asked; null before it has been asked. */
    val reachable: Boolean?,
) {
    companion object {
        fun parse(json: JSONObject) = Device(
            id = json.getString("id"),
            name = json.optString("name"),
            addedBy = json.text("added_by"),
            addedByName = json.text("added_by_name"),
            addedAt = json.optLong("added_at"),
            isThis = json.optBoolean("this"),
            reachable = json.flag("reachable"),
        )
    }
}

/** The mesh's ErisDB: of the apps approved to serve `erisdb`, the one this phone has heard from. */
data class ErisDb(
    val app: String,
    val name: String,
    /** The device that approved it. */
    val device: String,
    val approvedAt: Long,
    val reachable: Boolean,
) {
    companion object {
        fun parse(json: JSONObject) = ErisDb(
            app = json.getString("app"),
            name = json.optString("name"),
            device = json.optString("device"),
            approvedAt = json.optLong("approved_at"),
            reachable = json.optBoolean("reachable"),
        )
    }
}

data class Mesh(val id: String, val member: Boolean, val devices: List<Device>, val erisdb: ErisDb?) {
    companion object {
        fun parse(json: JSONObject) = Mesh(
            id = json.getString("id"),
            member = json.optBoolean("member"),
            devices = json.optJSONArray("devices").objects(Device::parse),
            erisdb = json.child("erisdb")?.let(ErisDb::parse),
        )
    }
}

/** A join this phone started, until it ends. */
data class Joining(val fingerprint: String, val host: String, val error: String?) {
    companion object {
        fun parse(json: JSONObject) =
            Joining(json.optString("fingerprint"), json.optString("host"), json.text("error"))
    }
}

/** A request waiting for the person: an app, or a new device. */
data class Prompt(
    val id: String,
    val isDevice: Boolean,
    /** It came by a ticket this phone showed, not from an app on this phone. */
    val viaTicket: Boolean,
    val name: String,
    /** The key asking: the app's, or the new device's. */
    val key: String,
    val service: String?,
    val grants: List<String>,
    val serves: Boolean,
    /** What Android says is asking, as the note records it. */
    val program: String?,
    val packageName: String?,
    val certificate: String?,
    /** For a request by ticket: the code both screens show. */
    val fingerprint: String?,
    /** Approving takes the ErisDB passphrase. */
    val passphrase: Boolean,
    /** Approving makes the app the mesh's ErisDB, in place of the one there is now. */
    val becomesErisDb: Boolean,
    val expires: Long,
    val busy: Boolean,
) {
    /** What it asks, after its name. */
    val asks: String
        get() = when {
            isDevice -> "asks to join your devices"
            becomesErisDb -> "asks to be your ErisDB"
            else -> "asks to use ${serviceName(service)}"
        }

    /** Who is asking, as far as this phone can say. */
    val who: String
        get() = when {
            packageName != null -> packageName
            program != null -> program
            viaTicket -> "An app with nothing on this phone to vouch for it"
            else -> "An unknown app"
        }

    /** The notification for it: a title and a body. */
    fun notification(): Pair<String, String> = if (isDevice) {
        "$name $asks" to "Compare the code ${fingerprint ?: "?"} with the new device's screen."
    } else {
        "$name $asks" to "$who wants ${grants.joinToString(", ").ifEmpty { "nothing more" }}."
    }

    /** The command approving it with the [chosen] grants and, when it takes one, the [secret]. */
    fun decide(chosen: Set<String>, secret: String): JSONObject = JSONObject().put("op", "decide").put("id", id).apply {
        if (!isDevice) put("grants", JSONArray(grants.filter { it in chosen }))
        if (passphrase) put("passphrase", secret)
    }

    companion object {
        fun parse(json: JSONObject) = Prompt(
            id = json.getString("id"),
            isDevice = json.optString("kind") == "device",
            viaTicket = json.optString("via") == "ticket",
            name = json.optString("name"),
            key = json.optString("key"),
            service = json.text("service"),
            grants = json.optJSONArray("grants").strings(),
            serves = json.optBoolean("serves"),
            program = json.text("program"),
            packageName = json.text("package"),
            certificate = json.text("certificate"),
            fingerprint = json.text("fingerprint"),
            passphrase = json.optBoolean("passphrase"),
            becomesErisDb = json.optBoolean("becomes_erisdb"),
            expires = json.optLong("expires"),
            busy = json.optBoolean("busy"),
        )
    }
}

/** A ticket this phone shows whose newcomer has not been answered yet. */
data class Ticket(val id: String, val purpose: String, val status: String, val expires: Long) {
    companion object {
        fun parse(json: JSONObject) =
            Ticket(json.getString("id"), json.optString("purpose"), json.optString("status"), json.optLong("expires"))
    }
}

/** A note this phone issued: one app it approved. */
data class Note(
    val id: String,
    val app: String,
    val name: String,
    val service: String,
    val grants: List<String>,
    val serves: Boolean,
    val program: String?,
    val at: Long,
    val revoked: Boolean,
    val counts: Boolean,
    /** It is the mesh's ErisDB. */
    val erisdb: Boolean,
) {
    /** How it stands, in a word or two; empty while it counts as approved. */
    val standing: String
        get() = when {
            revoked -> "revoked"
            erisdb -> "your ErisDB"
            !counts -> "no longer counts"
            else -> ""
        }

    companion object {
        fun parse(json: JSONObject) = Note(
            id = json.getString("id"),
            app = json.optString("app"),
            name = json.optString("name"),
            service = json.optString("service"),
            grants = json.optJSONArray("grants").strings(),
            serves = json.optBoolean("serves"),
            program = json.text("program"),
            at = json.optLong("at"),
            revoked = json.optBoolean("revoked"),
            counts = json.optBoolean("counts"),
            erisdb = json.optBoolean("erisdb"),
        )
    }
}

/** Something another device changed, as the person is told it. */
data class Event(val seq: Long, val at: Long, val text: String) {
    companion object {
        fun parse(json: JSONObject) = Event(json.optLong("seq"), json.optLong("at"), json.optString("text"))
    }
}

/** This phone's ErisAuth, as its screens show it. */
data class State(
    val version: Long,
    val deviceId: String,
    val deviceName: String?,
    val mesh: Mesh?,
    val joining: Joining?,
    val approvals: List<Prompt>,
    val tickets: List<Ticket>,
    val notes: List<Note>,
    val events: List<Event>,
) {
    companion object {
        fun parse(json: JSONObject): State {
            val device = json.child("device") ?: JSONObject()
            return State(
                version = json.optLong("version"),
                deviceId = device.optString("id"),
                deviceName = device.text("name"),
                mesh = json.child("mesh")?.let(Mesh::parse),
                joining = json.child("joining")?.let(Joining::parse),
                approvals = json.optJSONArray("approvals").objects(Prompt::parse),
                tickets = json.optJSONArray("tickets").objects(Ticket::parse),
                notes = json.optJSONArray("notes").objects(Note::parse),
                events = json.optJSONArray("events").objects(Event::parse),
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

/** A ticket just opened, for a new device or for an app with no ErisAuth beside it. */
data class Opened(val id: String, val ticket: String, val qr: Qr, val expires: Long) {
    companion object {
        fun parse(json: JSONObject) =
            Opened(json.getString("id"), json.getString("ticket"), Qr.parse(json.getJSONObject("qr")), json.optLong("expires"))
    }
}

data class Named(val id: String, val name: String) {
    companion object {
        fun parse(json: JSONObject) = Named(json.getString("id"), json.optString("name"))
    }
}

/** What removing a device offers to remove with it: the devices it added. */
data class Removal(val device: Named, val also: List<Named>) {
    /** The ids one removal carries, keeping the devices in [kept]. */
    fun devices(kept: Set<String>): List<String> = listOf(device.id) + also.map { it.id }.filter { it !in kept }

    companion object {
        fun parse(json: JSONObject) =
            Removal(Named.parse(json.getJSONObject("device")), json.optJSONArray("also").objects(Named::parse))
    }
}

/** Time left until [expires] (Unix seconds) as m:ss. */
fun remaining(expires: Long, now: Long): String {
    val left = maxOf(0L, expires - now)
    return "${left / 60}:${(left % 60).toString().padStart(2, '0')}"
}

/** How long ago [at] (Unix seconds) was. */
fun ago(at: Long, now: Long): String {
    val secs = maxOf(0L, now - at)
    val (count, unit) = when {
        secs < 60 -> return "just now"
        secs < 3600 -> secs / 60 to "minute"
        secs < 86_400 -> secs / 3600 to "hour"
        else -> secs / 86_400 to "day"
    }
    return "$count $unit${if (count == 1L) "" else "s"} ago"
}

/** [text]'s start and end, for a key or a certificate too long to show whole. */
fun short(text: String, head: Int = 8, tail: Int = 6): String =
    if (text.length <= head + tail + 1) text else "${text.take(head)}…${text.takeLast(tail)}"

/** The button that approves [picked] of the [of] grants asked for. */
fun approveLabel(picked: Int, of: Int): String = if (picked == of) "Approve" else "Approve $picked of $of"

/** Why the daemon refused, in words; null when it did not. */
fun refusal(outcome: JSONObject): String? {
    if (!outcome.has("error")) return null
    return when (outcome.optString("error")) {
        "needs_passphrase" -> "This takes the ErisDB passphrase."
        "wrong_passphrase" -> "That is not the passphrase."
        "locked" -> "Too many wrong passphrases from this phone. Try again in ${remaining(outcome.optLong("retry_after"), 0)}."
        "not_asked" -> "Approve only what was asked for."
        "no_passphrase" -> "ErisDB has no passphrase yet; set one where it runs, with erisdb passphrase set."
        else -> outcome.text("message") ?: outcome.text("error") ?: "ErisAuth refused."
    }
}

/** Why changing the passphrase did not happen, in words; null when it did. */
fun passphraseProblem(outcome: JSONObject): String? = when (outcome.optString("error")) {
    "wrong_passphrase" -> "That is not the current passphrase."
    else -> refusal(outcome)
}

/** What is wrong with a new passphrase as typed twice, in words; null when nothing is. */
fun newPassphraseProblem(new: String, again: String): String? = when {
    new.isNotEmpty() && new.length < 8 -> "At least 8 characters."
    again.isNotEmpty() && again != new -> "The two differ."
    else -> null
}
