package io.github.erickramer42.sensio

import android.app.Activity
import android.app.AlertDialog
import android.content.Context
import android.content.res.ColorStateList
import android.graphics.drawable.ColorDrawable
import android.graphics.drawable.GradientDrawable
import android.graphics.drawable.StateListDrawable
import android.text.SpannableString
import android.text.Spanned
import android.text.style.ForegroundColorSpan
import android.util.TypedValue
import android.view.View
import android.view.WindowInsetsController
import android.widget.Button
import android.widget.CheckBox
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView

/**
 * Controls and dialogs drawn in the user's theme. The platform's defaults
 * (light gray buttons, light or dark dialogs as the system has it) would be
 * the brightest things on a Night screen, so every control and dialog the
 * app shows is built here from the palette.
 */
object Ui {
    fun palette(context: Context): Palette = Options.theme(context).palette

    private fun dp(context: Context, v: Float): Int = (v * context.resources.displayMetrics.density).toInt()

    /** The window, the title bar and the system bars in the theme; the bars' icons light, since every theme is dark. */
    fun window(activity: Activity) {
        val p = palette(activity)
        activity.window.decorView.setBackgroundColor(p.bg)
        activity.window.isNavigationBarContrastEnforced = false
        activity.window.insetsController?.setSystemBarsAppearance(
            0,
            WindowInsetsController.APPEARANCE_LIGHT_STATUS_BARS or WindowInsetsController.APPEARANCE_LIGHT_NAVIGATION_BARS,
        )
        activity.actionBar?.let { bar ->
            bar.setBackgroundDrawable(ColorDrawable(p.surface))
            val title = SpannableString(activity.title)
            title.setSpan(ForegroundColorSpan(p.text), 0, title.length, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
            bar.title = title
        }
    }

    fun text(context: Context, size: Float, color: Int = palette(context).text) = TextView(context).apply {
        textSize = size
        setTextColor(color)
        setPadding(0, 8, 0, 8)
    }

    private fun rounded(color: Int, stroke: Int?, radius: Float, context: Context) = GradientDrawable().apply {
        setColor(color)
        cornerRadius = radius
        stroke?.let { setStroke(dp(context, 1f), it) }
    }

    /** A rounded button at least 48 dp tall, with a gap around it; its label shrinks rather than wrapping. */
    fun button(context: Context, label: String, action: () -> Unit) = Button(context).apply {
        val p = palette(context)
        val radius = dp(context, 24f).toFloat()
        background = StateListDrawable().apply {
            addState(intArrayOf(android.R.attr.state_pressed), rounded(p.elevatedLight, p.outline, radius, context))
            addState(intArrayOf(), rounded(p.elevated, p.outline, radius, context))
        }
        stateListAnimator = null
        setTextColor(p.text)
        text = label
        isAllCaps = false
        maxLines = 1
        minHeight = dp(context, 48f)
        setAutoSizeTextTypeUniformWithConfiguration(10, 16, 1, TypedValue.COMPLEX_UNIT_SP)
        setOnClickListener { action() }
    }

    /** A dialog in the theme: a title, a body, and up to two buttons, the second the safe one. */
    fun dialog(
        context: Context,
        title: String,
        body: View?,
        positive: Pair<String, () -> Unit>? = null,
        negative: String = "Close",
    ): AlertDialog {
        val p = palette(context)
        val pad = dp(context, 20f)
        val heading = text(context, 18f).apply { text = title; setPadding(pad, pad, pad, pad / 2) }
        val builder = AlertDialog.Builder(context).setCustomTitle(heading).setNegativeButton(negative, null)
        body?.let { builder.setView(ScrollView(context).apply { setPadding(pad, 0, pad, 0); addView(it) }) }
        positive?.let { (label, action) -> builder.setPositiveButton(label) { _, _ -> action() } }
        val dialog = builder.create()
        dialog.setOnShowListener {
            dialog.window?.setBackgroundDrawable(rounded(p.surface, p.outline, dp(context, 16f).toFloat(), context))
            for (which in listOf(AlertDialog.BUTTON_POSITIVE, AlertDialog.BUTTON_NEGATIVE)) {
                dialog.getButton(which)?.setTextColor(p.text)
            }
        }
        dialog.show()
        return dialog
    }

    /** A dialog of choices, each a row at least 48 dp tall; picking one closes it. */
    fun choices(context: Context, title: String, items: List<Pair<String, () -> Unit>>, negative: String = "Close") {
        lateinit var dialog: AlertDialog
        val rows = LinearLayout(context).apply {
            orientation = LinearLayout.VERTICAL
            for ((label, action) in items) {
                addView(text(context, 16f).apply {
                    text = label
                    minHeight = dp(context, 48f)
                    gravity = android.view.Gravity.CENTER_VERTICAL
                    setOnClickListener {
                        dialog.dismiss()
                        action()
                    }
                })
            }
        }
        dialog = dialog(context, title, rows, null, negative)
    }

    /** A dialog of check boxes; Save hands back which are checked. */
    fun checks(context: Context, title: String, labels: List<String>, checked: BooleanArray, onSave: (BooleanArray) -> Unit) {
        val p = palette(context)
        val rows = LinearLayout(context).apply {
            orientation = LinearLayout.VERTICAL
            labels.forEachIndexed { i, label ->
                addView(CheckBox(context).apply {
                    text = label
                    textSize = 14f
                    setTextColor(p.text)
                    buttonTintList = ColorStateList.valueOf(p.text)
                    minHeight = dp(context, 48f)
                    isChecked = checked[i]
                    setOnCheckedChangeListener { _, on -> checked[i] = on }
                })
            }
        }
        dialog(context, title, rows, "Save" to { onSave(checked) }, "Cancel")
    }

    /** A message, with an action beside Cancel. */
    fun confirm(context: Context, title: String, message: String, action: String, onConfirm: () -> Unit) {
        dialog(context, title, text(context, 15f).apply { text = message }, action to onConfirm, "Cancel")
    }
}
