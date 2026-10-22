package dev.eris.eon

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import kotlin.concurrent.thread
import org.json.JSONObject

/**
 * What the person hears while ErisOverlayNetwork's screens are closed: a notification for each request
 * waiting for them, gone once it is answered, and one for each change another device makes to
 * the mesh. Choosing one opens ErisOverlayNetwork.
 */
object Notifications {
    private const val REQUESTS = "requests"
    private const val CHANGES = "changes"

    private fun manager(context: Context) = context.getSystemService(NotificationManager::class.java)

    fun channels(context: Context) {
        manager(context).createNotificationChannels(
            listOf(
                NotificationChannel(REQUESTS, "Requests", NotificationManager.IMPORTANCE_HIGH),
                NotificationChannel(CHANGES, "Changes to your devices", NotificationManager.IMPORTANCE_DEFAULT),
            ),
        )
    }

    private fun open(context: Context, tag: Int): PendingIntent = PendingIntent.getActivity(
        context,
        tag,
        Intent(context, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP),
        PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
    )

    private fun tag(id: String) = id.hashCode()

    private fun post(context: Context, channel: String, tag: Int, title: String, body: String) {
        val notification = Notification.Builder(context, channel)
            .setSmallIcon(R.drawable.ic_notification)
            .setContentTitle(title)
            .setContentText(body)
            .setStyle(Notification.BigTextStyle().bigText(body))
            .setContentIntent(open(context, tag))
            .setAutoCancel(true)
            .setCategory(if (channel == REQUESTS) Notification.CATEGORY_MESSAGE else Notification.CATEGORY_STATUS)
            .build()
        runCatching { manager(context).notify(tag, notification) }
    }

    fun asked(context: Context, prompt: Prompt) {
        val (title, body) = prompt.notification()
        post(context, REQUESTS, tag(prompt.id), title, body)
    }

    fun answered(context: Context, id: String) {
        manager(context).cancel(tag(id))
    }

    fun changed(context: Context, event: Event) {
        post(context, CHANGES, tag("event ${event.seq}"), "ErisOverlayNetwork", event.text)
    }
}

/**
 * Follows the daemon for as long as the process lives, once per process: whenever its state
 * changes, posts a notification for each new request while the screens are not up, and for each
 * new change, and takes back the notifications of requests that are gone.
 */
object Watcher {
    @Volatile private var started = false

    /** Whether ErisOverlayNetwork's screens are in front of the person. */
    @Volatile var visible = false

    @Synchronized
    fun start(context: Context) {
        if (started) return
        started = true
        val app = context.applicationContext
        thread(isDaemon = true, name = "eon-watcher") { watch(app) }
    }

    private fun watch(context: Context) {
        while (Native.ensureStarted(context).isFailure) Thread.sleep(5_000)
        Notifications.channels(context)
        var version = -1L
        var shown = emptySet<String>()
        // The daemon starts with this process, so every event it holds is news.
        var told = 0L
        while (true) {
            val read = runCatching { Native.run(JSONObject().put("op", "state")) }.getOrNull()
            if (read != null && !read.has("error")) {
                val state = State.parse(read)
                version = state.version
                val waiting = state.approvals.map { it.id }.toSet()
                // While the screens are up, they show what waits.
                if (!visible) state.approvals.filter { it.id !in shown }.forEach { Notifications.asked(context, it) }
                (shown - waiting).forEach { Notifications.answered(context, it) }
                shown = waiting
                state.events.filter { it.seq > told }.forEach { Notifications.changed(context, it) }
                told = maxOf(told, state.events.maxOfOrNull { it.seq } ?: 0)
            }
            runCatching { Native.run(JSONObject().put("op", "wait").put("after", version).put("within_ms", 60_000)) }
                .onFailure { Thread.sleep(1_000) }
        }
    }
}
