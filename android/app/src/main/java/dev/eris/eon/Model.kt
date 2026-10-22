package dev.eris.eon

import java.util.Base64
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

/** Each capability an app uses, in order, with the grants in each. */
private fun JSONObject?.uses(): Map<String, List<String>> =
    if (this == null) emptyMap() else keys().asSequence().sorted().associateWith { optJSONArray(it).strings() }

/** A capability as the person says it: `database/1` is "your database". */
fun your(capability: String): String = "your ${capability.substringBefore('/')}"

/** Things listed as a person lists them: "a", "a and b", "a, b and c". */
fun listed(items: List<String>): String =
    if (items.size < 2) items.firstOrNull().orEmpty() else items.dropLast(1).joinToString(", ") + " and " + items.last()

/** The name of the device a join ticket, `eris://join/…`, adds; null when it is not one. */
fun joinTicketName(link: String): String? = runCatching {
    val payload = link.trim().removePrefix("eris://join/").takeIf { it != link.trim() } ?: return null
    JSONObject(String(Base64.getUrlDecoder().decode(payload))).text("name")
}.getOrNull()

/** Serving a capability: [single] when a mesh has one server of it, [passphrase] when apps using it take one. */
data class Served(val capability: String, val single: Boolean, val passphrase: Boolean) {
    companion object {
        fun parse(json: JSONObject) =
            Served(json.optString("capability"), json.optBoolean("single"), json.optBoolean("passphrase"))
    }
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

/** An app serving a capability. */
data class ServerApp(
    val app: String,
    val name: String,
    /** The device that approved it. */
    val device: String,
    val approvedAt: Long,
    /** Whether it answered when last asked; null when it is not asked. */
    val reachable: Boolean?,
) {
    companion object {
        fun parse(json: JSONObject) = ServerApp(
            app = json.getString("app"),
            name = json.optString("name"),
            device = json.optString("device"),
            approvedAt = json.optLong("approved_at"),
            reachable = json.flag("reachable"),
        )
    }
}

/** What the mesh's apps serve: one capability, and the apps serving it, newest first. */
data class Server(val capability: String, val single: Boolean, val passphrase: Boolean, val apps: List<ServerApp>) {
    /** The apps in use: of a single one, the newest. */
    val inUse: List<ServerApp>
        get() = if (single) apps.take(1) else apps

    companion object {
        fun parse(json: JSONObject) = Server(
            capability = json.optString("capability"),
            single = json.optBoolean("single"),
            passphrase = json.optBoolean("passphrase"),
            apps = json.optJSONArray("apps").objects(ServerApp::parse),
        )
    }
}

data class Mesh(val id: String, val member: Boolean, val devices: List<Device>, val servers: List<Server>) {
    /** The capabilities whose servers take a passphrase. */
    val passphrases: List<String>
        get() = servers.filter { it.passphrase && it.apps.isNotEmpty() }.map { it.capability }

    companion object {
        fun parse(json: JSONObject) = Mesh(
            id = json.getString("id"),
            member = json.optBoolean("member"),
            devices = json.optJSONArray("devices").objects(Device::parse),
            servers = json.optJSONArray("servers").objects(Server::parse),
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

/** This phone's own ticket, while it is in no mesh, and what another device offers on it. */
data class Offer(
    val name: String,
    val ticket: String,
    val qr: Qr,
    val expires: Long,
    /** `showing`, `offered`, `accepted` or `joined`. */
    val status: String,
    /** The device offering to add this one. */
    val by: String?,
    /** Every device in the mesh it offers. */
    val devices: List<String>,
) {
    companion object {
        fun parse(json: JSONObject) = Offer(
            name = json.optString("name"),
            ticket = json.optString("ticket"),
            qr = Qr.parse(json.getJSONObject("qr")),
            expires = json.optLong("expires"),
            status = json.optString("status"),
            by = json.text("by"),
            devices = json.optJSONArray("devices").strings(),
        )
    }
}

/** A device this phone is adding by the ticket it shows: [status] is `waiting`, `added`, `declined` or `failed`. */
data class Inviting(val name: String, val status: String, val error: String?) {
    companion object {
        fun parse(json: JSONObject) = Inviting(json.optString("name"), json.optString("status"), json.text("error"))
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
    /** What the app uses, with the grants it asks for in each. */
    val uses: Map<String, List<String>>,
    /** What the app serves. */
    val serves: List<Served>,
    /** What Android says is asking, as the note records it. */
    val program: String?,
    val packageName: String?,
    val certificate: String?,
    /** For a request by ticket: the code both screens show. */
    val fingerprint: String?,
    /** Approving takes a passphrase: the app uses something whose server takes one. */
    val passphrase: Boolean,
    /** Approving makes the app the single server of these capabilities, in place of the apps named. */
    val replaces: List<Pair<String, String>>,
    val expires: Long,
    val busy: Boolean,
) {
    /** Every grant asked for, in each capability, in the order asked. */
    val asked: List<Pair<String, String>>
        get() = uses.flatMap { (capability, grants) -> grants.map { capability to it } }

    /** What it asks, after its name. */
    val asks: String
        get() {
            val single = serves.filter { it.single }.map { your(it.capability) }
            val served = serves.map { it.capability.substringBefore('/') }
            return when {
                isDevice -> "asks to join your devices"
                single.isNotEmpty() -> "asks to be ${listed(single)}"
                uses.isEmpty() -> "asks to serve ${listed(served)}"
                served.isEmpty() -> "asks to use ${listed(uses.keys.map(::your))}"
                else -> "asks to use ${listed(uses.keys.map(::your))}, and to serve ${listed(served)}"
            }
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
        "$name $asks" to "$who wants ${asked.joinToString(", ") { it.second }.ifEmpty { "nothing more" }}."
    }

    /**
     * The command approving it with the [chosen] grants, each a capability and a grant, and, when
     * it takes one, the [secret]. A capability whose grants were all unticked is left out; one
     * that asked for none stays.
     */
    fun decide(chosen: Set<Pair<String, String>>, secret: String): JSONObject =
        JSONObject().put("op", "decide").put("id", id).apply {
            if (!isDevice) {
                val grants = JSONObject()
                uses.forEach { (capability, asked) ->
                    val kept = asked.filter { (capability to it) in chosen }
                    if (kept.isNotEmpty() || asked.isEmpty()) grants.put(capability, JSONArray(kept))
                }
                put("grants", grants)
            }
            if (passphrase) put("passphrase", secret)
        }

    companion object {
        fun parse(json: JSONObject) = Prompt(
            id = json.getString("id"),
            isDevice = json.optString("kind") == "device",
            viaTicket = json.optString("via") == "ticket",
            name = json.optString("name"),
            key = json.optString("key"),
            uses = json.optJSONObject("uses").uses(),
            serves = json.optJSONArray("serves").objects(Served::parse),
            program = json.text("program"),
            packageName = json.text("package"),
            certificate = json.text("certificate"),
            fingerprint = json.text("fingerprint"),
            passphrase = json.optBoolean("passphrase"),
            replaces = json.optJSONArray("replaces").objects { it.optString("capability") to it.optString("name") },
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
    val uses: Map<String, List<String>>,
    val serves: List<Served>,
    val program: String?,
    val at: Long,
    val revoked: Boolean,
    val counts: Boolean,
) {
    /** How it stands, in a word or two; empty while it counts as approved. */
    val standing: String
        get() = when {
            revoked -> "revoked"
            !counts -> "no longer counts"
            else -> ""
        }

    /** What it approves, in a line: what it uses, and what it serves. */
    val approves: String
        get() = listOfNotNull(
            uses.keys.takeIf { it.isNotEmpty() }?.let { "uses ${listed(it.toList())}" },
            serves.takeIf { it.isNotEmpty() }?.let { s -> "serves ${listed(s.map { it.capability })}" },
        ).joinToString(", ")

    companion object {
        fun parse(json: JSONObject) = Note(
            id = json.getString("id"),
            app = json.optString("app"),
            name = json.optString("name"),
            uses = json.optJSONObject("uses").uses(),
            serves = json.optJSONArray("serves").objects(Served::parse),
            program = json.text("program"),
            at = json.optLong("at"),
            revoked = json.optBoolean("revoked"),
            counts = json.optBoolean("counts"),
        )
    }
}

/** Something another device changed, as the person is told it. */
data class Event(val seq: Long, val at: Long, val text: String) {
    companion object {
        fun parse(json: JSONObject) = Event(json.optLong("seq"), json.optLong("at"), json.optString("text"))
    }
}

/** This phone's ErisOverlayNetwork, as its screens show it. */
data class State(
    val version: Long,
    val deviceId: String,
    val deviceName: String?,
    /** What the system calls this phone. */
    val hostname: String?,
    val mesh: Mesh?,
    val joining: Joining?,
    val offer: Offer?,
    val inviting: Inviting?,
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
                hostname = device.text("hostname"),
                mesh = json.child("mesh")?.let(Mesh::parse),
                joining = json.child("joining")?.let(Joining::parse),
                offer = json.child("offer")?.let(Offer::parse),
                inviting = json.child("inviting")?.let(Inviting::parse),
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

/**
 * A ticket just opened, for a new device or for an app with no eon beside it; for a device, with
 * the short [code] a person types in its place.
 */
data class Opened(val id: String, val ticket: String, val code: String?, val qr: Qr, val expires: Long) {
    companion object {
        fun parse(json: JSONObject) = Opened(
            json.getString("id"),
            json.getString("ticket"),
            json.text("code"),
            Qr.parse(json.getJSONObject("qr")),
            json.optLong("expires"),
        )
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
        "needs_passphrase" -> "This takes a passphrase."
        "wrong_passphrase" -> "That is not the passphrase."
        "locked" -> "Too many wrong passphrases from this phone. Try again in ${remaining(outcome.optLong("retry_after"), 0)}."
        "not_asked" -> "Approve only what was asked for."
        else -> outcome.text("message") ?: outcome.text("error") ?: "eon refused."
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
