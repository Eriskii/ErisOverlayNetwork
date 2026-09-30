package dev.eris.auth

import android.Manifest
import android.app.Activity
import android.content.Intent
import android.content.pm.PackageManager
import android.graphics.Color
import android.os.Bundle
import android.webkit.WebResourceRequest
import android.webkit.WebView
import android.webkit.WebViewClient
import java.net.URLEncoder

/**
 * ErisAuth's approval page, from the daemon in this process. A pairing ticket opened with the
 * camera lands here and fills in the page's join form.
 */
class MainActivity : Activity() {
    private lateinit var web: WebView
    /** A ticket waiting for the page. */
    private var ticket: String? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        web = WebView(this).apply {
            setBackgroundColor(Color.parseColor("#262626"))
            settings.javaScriptEnabled = true
            settings.domStorageEnabled = true
            webViewClient = object : WebViewClient() {
                // The daemon's page stays here; anything else opens elsewhere.
                override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest): Boolean {
                    if (request.url.host == "127.0.0.1") return false
                    startActivity(Intent(Intent.ACTION_VIEW, request.url))
                    return true
                }
            }
        }
        setContentView(web)
        ticket = ticketIn(intent)
        if (checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED) {
            requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), 0)
        }
        Notifications.channel(this)
        start()
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        ticketIn(intent)?.let { ticket = it }
        start()
    }

    private fun ticketIn(intent: Intent?): String? =
        intent?.data?.takeIf { it.scheme == "erisdb" && it.host == "pair" }?.toString()

    private fun start() {
        Thread {
            val page = Native.ensureStarted(this)
            runOnUiThread {
                page.onSuccess { link ->
                    val join = ticket?.let { "&join=" + URLEncoder.encode(it, "UTF-8") } ?: ""
                    ticket = null
                    web.loadUrl(link + join)
                }.onFailure { e ->
                    web.loadData(
                        "<body style='background:#262626;color:#eee;font-family:monospace;padding:24px'>" +
                            "ErisAuth did not start: ${e.message}</body>",
                        "text/html",
                        "utf-8",
                    )
                }
            }
        }.start()
    }
}
