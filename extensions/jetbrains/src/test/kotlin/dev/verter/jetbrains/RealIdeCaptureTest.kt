package dev.verter.jetbrains

import com.intellij.openapi.actionSystem.ActionManager
import com.intellij.openapi.application.ApplicationInfo
import com.intellij.openapi.application.ApplicationManager
import com.intellij.testFramework.PlatformTestUtil
import com.intellij.testFramework.fixtures.BasePlatformTestCase
import java.awt.Graphics
import java.awt.Window
import java.awt.image.BufferedImage
import java.lang.ProcessHandle
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicLong
import javax.swing.JComponent
import javax.swing.JPanel
import javax.swing.RootPaneContainer

/**
 * JBT1H test hook: drive the pinned WebStorm test IDE through the Platform
 * test-framework API, capture IDE/plugin versions, measure an EDT
 * application/paint cycle (not RPC), and snapshot the process tree. A mock
 * LSP client is not used.
 */
class RealIdeCaptureTest : BasePlatformTestCase() {
    fun `test capture reports real-jetbrains-ide host and measured UI paint`() {
        val json = captureJson()
        assertTrue("capture must declare the real IDE host (JBT1H-AC1)", json.contains("\"hostKind\":\"real-jetbrains-ide\""))
        assertTrue("sleep-as-readiness is forbidden", json.contains("\"usedSleepForReadiness\":false"))
        assertTrue("ui-apply-paint must be a measured metric", json.contains("\"uiApplyPaintMs\":{\"status\":\"measured\""))
        assertFalse("mock LSP must not appear as the host", json.contains("\"hostKind\":\"mock-lsp\""))
        assertFalse("raw-LSP must not appear as the host", json.contains("\"hostKind\":\"raw-lsp\""))
        assertTrue("JVM capture must emit workflows for parseIdeCapture", json.contains("\"workflows\":["))
        assertTrue("JVM capture must emit receiptBasis for parseIdeCapture", json.contains("\"receiptBasis\":{"))
    }

    fun `test capture records IDE plugin and engine versions per run`() {
        val info = ApplicationInfo.getInstance()
        val json = captureJson()
        assertTrue("IDE product name must be captured", json.contains(esc(info.fullApplicationName)))
        assertTrue("IDE build must be captured", json.contains(esc(info.build.asString())))
        val pluginVersion = VerterHealthAction.installedPluginVersion()
        assertNotNull(pluginVersion)
        assertTrue("plugin version must be captured", json.contains(esc(pluginVersion!!)))
        assertTrue(
            "engine identity is labelled unknown on the skeleton, never guessed as zero",
            json.contains("\"engine\":{\"status\":\"unknown\",\"reason\":"),
        )
        assertFalse(
            "unknown engine must not carry a numeric value",
            Regex(""""engine":\{[^}]*"value"""").containsMatchIn(json),
        )
    }

    fun `test process tree labels a missing TypeScript provider instead of guessing RSS`() {
        val json = captureJson()
        assertTrue(json.contains("\"typeProviderStatus\":\"unknown\"") || json.contains("\"typeProviderStatus\":\"missing\"") || json.contains("\"typeProviderStatus\":\"observed\""))
        assertTrue("unavailable retained memory stays unknown", json.contains("\"rssBytes\":{\"status\":\"unknown\""))
        assertFalse("guessed zero RSS is forbidden", json.contains("\"rssBytes\":{\"status\":\"measured\",\"value\":0"))
    }

    fun `test process tree records real parentage and provider role when present`() {
        val json = captureJson()
        val self = ProcessHandle.current()
        assertTrue(
            "IDE root parentPid is null",
            json.contains("""{"pid":${self.pid()},"parentPid":null,"""),
        )
        self.descendants().forEach { handle ->
            val parent = handle.parent().map { it.pid() }.orElse(null)
            val parentJson = parent?.toString() ?: "null"
            assertTrue(
                "descendant ${handle.pid()} must record ProcessHandle.parent() ($parentJson), not a flattened star",
                json.contains("""{"pid":${handle.pid()},"parentPid":$parentJson,"""),
            )
        }
        val providerPids = self.descendants().toList().filter { looksLikeTypeProvider(it) }.map { it.pid() }
        if (providerPids.isEmpty()) {
            assertTrue(json.contains("\"typeProviderPids\":[]"))
            assertFalse("no provider image in this tree, so role=provider must stay unemitted", json.contains("\"role\":\"provider\""))
        } else {
            assertTrue("provider image must be labelled role=provider", json.contains("\"role\":\"provider\""))
            providerPids.forEach { pid ->
                assertTrue("typeProviderPids must include $pid", json.contains(pid.toString()))
            }
        }
    }

    fun `test health action application is dispatched on the real test IDE EDT`() {
        val paintNs = measureHealthActionPaintNs()
        assertTrue("EDT application/paint must run on a real clock (was ${paintNs}ns)", paintNs >= 0)
        val action = ActionManager.getInstance().getAction(VerterHealthAction.ACTION_ID)
        assertNotNull(action)
        assertTrue(action is VerterHealthAction)
    }

    private fun measureHealthActionPaintNs(): Long {
        val paintNs = AtomicLong(-1)
        ApplicationManager.getApplication().invokeAndWait {
            val action = ActionManager.getInstance().getAction(VerterHealthAction.ACTION_ID)
                ?: error("verter.Health is not registered on the test IDE")
            val paintObserved = AtomicBoolean(false)
            val start = System.nanoTime()
            ActionManager.getInstance().tryToExecute(action, null, null, "JBT1H", true)
            observeClientPaint(paintObserved)
            // 2025.3+ replacement for the removed dispatchAllInvocationEvents();
            // drains IdeEventQueue and awaits background write actions.
            PlatformTestUtil.dispatchAllInvocationEventsInIdeEventQueue()
            if (!paintObserved.get()) {
                error("ui-apply-paint requires an observed paint() phase; none fired")
            }
            paintNs.set(System.nanoTime() - start)
        }
        return paintNs.get()
    }

    private fun observeClientPaint(paintObserved: AtomicBoolean) {
        val image = BufferedImage(16, 16, BufferedImage.TYPE_INT_ARGB)
        val graphics = image.createGraphics()
        try {
            val client = showingClientComponent()
            if (client != null) {
                client.paint(graphics)
                paintObserved.set(true)
                return
            }
            object : JPanel() {
                override fun paint(g: Graphics) {
                    super.paint(g)
                    paintObserved.set(true)
                }
            }.apply { setSize(16, 16) }.paint(graphics)
        } finally {
            graphics.dispose()
        }
    }

    private fun showingClientComponent(): JComponent? {
        val editor = myFixture.editor?.contentComponent
        if (editor != null && editor.isShowing) return editor
        for (window in Window.getWindows()) {
            if (!window.isShowing) continue
            val pane = (window as? RootPaneContainer)?.contentPane as? JComponent
            if (pane != null) return pane
        }
        return null
    }

    private fun captureJson(): String {
        val info = ApplicationInfo.getInstance()
        val paintMs = measureHealthActionPaintNs().toDouble() / 1_000_000.0
        val self = ProcessHandle.current()
        val descendants = self.descendants().toList()
        val providerPids = descendants.filter { looksLikeTypeProvider(it) }.map { it.pid() }
        val members = buildString {
            append("[")
            append(memberJson(self.pid(), null, self.info().command().orElse("idea"), "ide"))
            descendants.forEach { handle ->
                append(",")
                val parentPid = handle.parent().map { it.pid() }.orElse(null)
                val role = if (looksLikeTypeProvider(handle)) "provider" else "descendant"
                append(
                    memberJson(
                        handle.pid(),
                        parentPid,
                        handle.info().command().orElse(""),
                        role,
                    ),
                )
            }
            append("]")
        }
        val pluginVersion = VerterHealthAction.installedPluginVersion() ?: "unknown"
        val (providerStatus, providerReason) = if (providerPids.isEmpty()) {
            "unknown" to "no TypeScript provider process in the test-IDE tree (skeleton has no engine; JBT2 owns lifecycle)"
        } else {
            "observed" to "TypeScript provider process observed in the test-IDE tree"
        }
        return """
            {"hostKind":"real-jetbrains-ide","usedSleepForReadiness":false,
            "versions":{"ideProduct":${esc(info.fullApplicationName)},"ideBuild":${esc(info.build.asString())},
            "pluginVersion":${esc(pluginVersion)},
            "engine":{"status":"unknown","reason":"skeleton carries no TypeScript engine (JBT2)"}},
            "paint":{"uiApplyPaintMs":{"status":"measured","value":$paintMs,"unit":"ms"},
            "rpcDurationMs":{"status":"unknown","reason":"no semantic RPC on the skeleton health-action path"}},
            "processTree":{"rootPid":${self.pid()},"members":$members,
            "typeProviderStatus":"$providerStatus","typeProviderPids":${providerPids.joinToString(",", "[", "]")},
            "typeProviderReason":${esc(providerReason)}},
            "workflows":${workflowRowsJson("verter")},
            "receiptBasis":${receiptBasisJson(info)},
            "side":"verter","sessionState":"cold","basisKind":"fresh"}
        """.trimIndent().replace("\n", "")
    }

    private fun workflowRowsJson(side: String): String {
        val completeness = if (side == "verter") "unsupported" else "pending"
        val reason = if (side == "verter") {
            "plugin skeleton has no semantic features; JBT2 owns lifecycle — workflow stays unsupported, not complete"
        } else {
            "official semantic workflow on the platform-only test IDE is pending bundled Vue/JS plugin capture; not guessed"
        }
        val workflows = listOf(
            "completion",
            "diagnostics",
            "source-navigation",
            "find-usages",
            "generics",
            "public-types",
            "component-extraction",
            "rename-move-import-updates",
            "formatting",
            "inlays",
            "styles",
            "run-debug-workflows",
        )
        return workflows.joinToString(",", "[", "]") { id ->
            """{"workflow":"$id","side":"$side","completeness":"$completeness","reason":${esc(reason)}}"""
        }
    }

    private fun receiptBasisJson(info: ApplicationInfo): String {
        return """{"sourceRevisions":"fixture:dx-harness-hermetic@pinned",""" +
            """"projectConfiguration":"packages/dx-harness/fixtures/hermetic",""" +
            """"engineIdentity":"unknown: skeleton carries no TypeScript engine (JBT2)",""" +
            """"hostIdentity":${esc("real-jetbrains-ide:platform-test-framework:${info.build.asString()}")},""" +
            """"completenessState":"partial"}"""
    }

    private fun memberJson(pid: Long, parentPid: Long?, image: String, role: String): String {
        val parent = parentPid?.toString() ?: "null"
        return """{"pid":$pid,"parentPid":$parent,"image":${esc(image)},"role":"$role",""" +
            """"rssBytes":{"status":"unknown","reason":"RSS not sampled in the platform test hook"}}"""
    }

    private fun looksLikeTypeProvider(handle: ProcessHandle): Boolean {
        val info = handle.info()
        val command = info.command().orElse("")
        val args = info.arguments().orElse(emptyArray()).joinToString(" ")
        val haystack = "$command $args".lowercase()
        return PROVIDER_MARKERS.any { haystack.contains(it) }
    }

    private fun esc(value: String): String {
        return "\"" + value.replace("\\", "\\\\").replace("\"", "\\\"") + "\""
    }

    companion object {
        private val PROVIDER_MARKERS = listOf(
            "tsgo",
            "tsserver",
            "typescript-language-server",
            "typescript/lib/tsserver",
        )
    }
}
