package dev.eris.auth

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import org.json.JSONObject

/** A notification for each request waiting for the person; choosing it opens the page. */
object Notifications {
    private const val CHANNEL = "requests"

    private fun manager(context: Context) = context.getSystemService(NotificationManager::class.java)

    fun channel(context: Context) {
        manager(context).createNotificationChannel(
            NotificationChannel(CHANNEL, "Sign-in requests", NotificationManager.IMPORTANCE_HIGH),
        )
    }

    private fun tag(id: String) = id.hashCode()

    fun asked(context: Context, approval: JSONObject) {
        val id = approval.getString("id")
        val requested = approval.getJSONArray("requested")
        val wants = List(requested.length()) { requested.getString(it) }.joinToString(", ")
        val who = approval.optString("package").ifEmpty { "An app" }
        val open = PendingIntent.getActivity(
            context,
            tag(id),
            Intent(context, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        val notification = Notification.Builder(context, CHANNEL)
            .setSmallIcon(R.drawable.ic_notification)
            .setContentTitle("${approval.optString("client", "An app")} asks to sign in")
            .setContentText("$who wants $wants")
            .setStyle(Notification.BigTextStyle().bigText("$who wants $wants"))
            .setContentIntent(open)
            .setAutoCancel(true)
            .setCategory(Notification.CATEGORY_MESSAGE)
            .build()
        runCatching { manager(context).notify(tag(id), notification) }
    }

    fun done(context: Context, id: String) {
        manager(context).cancel(tag(id))
    }
}
