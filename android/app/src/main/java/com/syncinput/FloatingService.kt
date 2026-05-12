package com.syncinput

import android.annotation.SuppressLint
import android.app.*
import android.content.Context
import android.content.Intent
import android.graphics.PixelFormat
import android.graphics.Point
import android.os.Build
import android.os.IBinder
import android.view.*
import android.webkit.WebView
import android.webkit.WebViewClient
import android.widget.*

class FloatingService : Service() {

    private lateinit var windowManager: WindowManager
    private lateinit var bubbleView: View
    private var panelView: View? = null
    private var isExpanded = false

    override fun onBind(intent: Intent?): IBinder? = null

    @SuppressLint("ClickableViewAccessibility")
    override fun onCreate() {
        super.onCreate()
        windowManager = getSystemService(WINDOW_SERVICE) as WindowManager

        // ── floating bubble ──
        bubbleView = ImageView(this).apply {
            setImageResource(R.drawable.ic_bubble)
            setBackgroundResource(R.drawable.bubble_bg)
        }
        val bubbleParams = WindowManager.LayoutParams(
            WindowManager.LayoutParams.WRAP_CONTENT,
            WindowManager.LayoutParams.WRAP_CONTENT,
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O)
                WindowManager.LayoutParams.TYPE_APPLICATION_OVERLAY
            else
                WindowManager.LayoutParams.TYPE_PHONE,
            WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE,
            PixelFormat.TRANSLUCENT
        ).apply {
            gravity = Gravity.TOP or Gravity.START
            x = 100; y = 300
        }
        windowManager.addView(bubbleView, bubbleParams)
        makeDraggable(bubbleView, bubbleParams)

        bubbleView.setOnClickListener {
            if (!isExpanded) expand()
        }

        startNotification()
    }

    @SuppressLint("SetTextI18n")
    private fun expand() {
        val point = Point()
        windowManager.defaultDisplay.getSize(point)
        val pw = (point.x * 0.92).toInt()
        val ph = (point.y * 0.72).toInt()

        val container = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setBackgroundColor(0xFF1a1a2e.toInt())
        }

        // collapse button bar
        val topBar = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.END
            setPadding(0, 8, 8, 8)
        }
        val collapseBtn = Button(this).apply {
            text = "▽ 收起"
            setTextColor(0xFFE0E0E0.toInt())
            setBackgroundColor(0x00000000)
            textSize = 13f
            setOnClickListener { collapse() }
        }
        topBar.addView(collapseBtn)
        container.addView(topBar, LinearLayout.LayoutParams(
            LinearLayout.LayoutParams.MATCH_PARENT,
            LinearLayout.LayoutParams.WRAP_CONTENT
        ))

        // WebView
        val webView = WebView(this).apply {
            webViewClient = WebViewClient()
            settings.javaScriptEnabled = true
            settings.domStorageEnabled = true
            settings.allowFileAccess = false
            loadUrl("http://192.168.124.12:5200")
        }
        container.addView(webView, LinearLayout.LayoutParams(
            LinearLayout.LayoutParams.MATCH_PARENT,
            0, 1f
        ))

        panelView = container

        val panelParams = WindowManager.LayoutParams(
            pw, ph,
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O)
                WindowManager.LayoutParams.TYPE_APPLICATION_OVERLAY
            else
                WindowManager.LayoutParams.TYPE_PHONE,
            WindowManager.LayoutParams.FLAG_NOT_TOUCH_MODAL,
            PixelFormat.TRANSLUCENT
        ).apply {
            gravity = Gravity.CENTER
        }
        windowManager.addView(container, panelParams)
        bubbleView.visibility = View.GONE
        isExpanded = true
    }

    fun collapse() {
        panelView?.let { windowManager.removeView(it) }
        panelView = null
        bubbleView.visibility = View.VISIBLE
        isExpanded = false
    }

    @SuppressLint("ClickableViewAccessibility")
    private fun makeDraggable(view: View, params: WindowManager.LayoutParams) {
        var initialX = 0
        var initialY = 0
        var initialTouchX = 0f
        var initialTouchY = 0f
        var moving = false

        view.setOnTouchListener { _, event ->
            when (event.action) {
                MotionEvent.ACTION_DOWN -> {
                    initialX = params.x
                    initialY = params.y
                    initialTouchX = event.rawX
                    initialTouchY = event.rawY
                    moving = false
                    true
                }
                MotionEvent.ACTION_MOVE -> {
                    val dx = (event.rawX - initialTouchX).toInt()
                    val dy = (event.rawY - initialTouchY).toInt()
                    if (kotlin.math.abs(dx) > 5 || kotlin.math.abs(dy) > 5) moving = true
                    if (moving) {
                        params.x = initialX + dx
                        params.y = initialY + dy
                        windowManager.updateViewLayout(view, params)
                    }
                    true
                }
                MotionEvent.ACTION_UP -> !moving
                else -> false
            }
        }
    }

    private fun startNotification() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val channel = NotificationChannel(
                "syncinput", "SyncInput",
                NotificationManager.IMPORTANCE_LOW
            )
            (getSystemService(NOTIFICATION_SERVICE) as NotificationManager)
                .createNotificationChannel(channel)
        }
        val notification = Notification.Builder(this, "syncinput")
            .setContentTitle("SyncInput")
            .setContentText("悬浮球运行中")
            .setSmallIcon(R.drawable.ic_bubble)
            .build()
        startForeground(1, notification)
    }

    override fun onDestroy() {
        collapse()
        windowManager.removeView(bubbleView)
        super.onDestroy()
    }
}
