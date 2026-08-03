package org.loopslcr.app

/**
 * The two paths a file can have been rendered for, put into words.
 *
 * # Why this is its own file
 *
 * The engine has always chosen correctly between them and has always reported
 * what it chose. What it could not do is make the choice *legible*: the screen
 * said `workflow: foldback (detected foldback)`, which states an outcome and
 * offers no way to disagree with it. A screen recording made the cost obvious —
 * the foldback appeared and vanished while markers were dragged, and nothing on
 * screen said why or what it meant.
 *
 * So the naming, the explanation and the "does this need saying?" decision live
 * here, out of the layout, where they can be tested. The rule this follows: the
 * app may make a choice for the user, but it may not make one they cannot see,
 * cannot understand, and cannot overrule.
 */
object Paths {

    /** The settings offered, in the order they are shown. */
    val offered = listOf("auto", "warmup", "foldback")

    /**
     * What to call a path on a button.
     *
     * `warmup` and `foldback` are the engine's words and describe the *file*;
     * `cut` and `fold` describe what the tool will *do*, which is what someone
     * pressing a button is choosing. The A and B are kept because that is how
     * the two paths are named everywhere else in this project.
     */
    fun label(name: String): String = when (name) {
        "auto" -> "auto"
        "warmup" -> "cut (A)"
        "foldback" -> "fold (B)"
        "trimmed" -> "already cut"
        else -> "unclear"
    }

    /** One line on what that path does to the audio. */
    fun explain(name: String): String = when (name) {
        "warmup" ->
            "the file plays the loop twice — keep the second pass, where it has settled, " +
                "and drop the tail"
        "foldback" ->
            "the file plays it once — the tail is added back onto the head, so the loop " +
                "starts already settled"
        "trimmed" ->
            "already exactly one loop with no tail — there is nothing to cut away"
        else ->
            "neither shape fits this file, so nothing can be assumed about it"
    }

    /** A line under the selector, and whether it wants attention. */
    data class Note(val text: String, val warn: Boolean)

    /**
     * What to say about the current choice.
     *
     * Three cases worth distinguishing, and the third is the reason this exists:
     *
     * - **Auto, and the shape was recognised.** Say which, and show the
     *   evidence. Nothing is wrong, so nothing is highlighted.
     * - **Auto, and it was not.** The engine falls back to the straight cut,
     *   deliberately — a wrong straight cut can be heard and redone, a wrong
     *   foldback quietly doubles the tails. That fallback is a decision made on
     *   the user's behalf and so it gets said out loud.
     * - **A manual choice that contradicts the detection.** Not an error: the
     *   user may well know something the file does not say. But it is worth one
     *   line, because the other likely cause is that they forgot the chip was
     *   set from a previous file.
     */
    fun note(chosen: String, plan: Plan): Note {
        val detected = plan.workflowDetected
        val evidence = plan.audibleLoops
            ?.let { "%.2f".format(it) + " loops of audible material" }

        fun withEvidence(head: String) =
            if (evidence == null) head else "$head · $evidence"

        return when {
            chosen == "auto" && detected == "unclear" -> Note(
                withEvidence("shape unclear — auto took the straight cut, the one you can undo"),
                warn = true,
            )
            chosen == "auto" -> Note(
                withEvidence("detected " + label(detected)),
                warn = false,
            )
            chosen != detected && detected != "unclear" -> Note(
                withEvidence("you chose ${label(chosen)}; the file looks like ${label(detected)}"),
                warn = true,
            )
            else -> Note(withEvidence("matches the file"), warn = false)
        }
    }
}
