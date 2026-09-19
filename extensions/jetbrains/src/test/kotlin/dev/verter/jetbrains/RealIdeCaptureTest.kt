package dev.verter.jetbrains

import com.intellij.openapi.actionSystem.ActionManager
import com.intellij.openapi.application.ApplicationInfo
import com.intellij.openapi.application.ApplicationManager
import com.intellij.testFramework.PlatformTestUtil
import com.intellij.testFramework.fixtures.BasePlatformTestCase
import java.lang.ProcessHandle
import java.util.concurrent.atomic.AtomicLong

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
        assertTrue(json.contains("\"typeProviderStatus\":\"unknown\"") || json.contains("\"typeProviderStatus\":\"missing\""))
        assertTrue("unavailable retained memory stays unknown", json.contains("\"rssBytes\":{\"status\":\"unknown\""))
        assertFalse("guessed zero RSS is forbidden", json.contains("\"rssBytes\":{\"status\":\"measured\",\"value\":0"))
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
            val start = System.nanoTime()
            ActionManager.getInstance().tryToExecute(action, null, null, "JBT1H", true)
            // 2025.3+ replacement for the removed dispatchAllInvocationEvents();
            // drains IdeEventQueue and awaits background write actions.
            PlatformTestUtil.dispatchAllInvocationEventsInIdeEventQueue()
            paintNs.set(System.nanoTime() - start)
        }
        return paintNs.get()
    }

    private fun captureJson(): String {
        val info = ApplicationInfo.getInstance()
        val paintMs = measureHealthActionPaintNs().toDouble() / 1_000_000.0
        val self = ProcessHandle.current()
        val descendants = self.descendants().toList()
        val members = buildString {
            append("[")
            append(memberJson(self.pid(), null, self.info().command().orElse("idea"), "ide"))
            descendants.forEach { handle ->
                append(",")
                append(
                    memberJson(
                        handle.pid(),
                        self.pid(),
                        handle.info().command().orElse(""),
                        "descendant",
                    ),
                )
            }
            append("]")
        }
        val pluginVersion = VerterHealthAction.installedPluginVersion() ?: "unknown"
        return """
            {"hostKind":"real-jetbrains-ide","usedSleepForReadiness":false,
            "versions":{"ideProduct":${esc(info.fullApplicationName)},"ideBuild":${esc(info.build.asString())},
            "pluginVersion":${esc(pluginVersion)},
            "engine":{"status":"unknown","reason":"skeleton carries no TypeScript engine (JBT2)"}},
            "paint":{"uiApplyPaintMs":{"status":"measured","value":$paintMs,"unit":"ms"},
            "rpcDurationMs":{"status":"unknown","reason":"no semantic RPC on the skeleton health-action path"}},
            "processTree":{"rootPid":${self.pid()},"members":$members,
            "typeProviderStatus":"unknown","typeProviderPids":[],
            "typeProviderReason":"no TypeScript provider process in the test-IDE tree (skeleton has no engine; JBT2 owns lifecycle)"},
            "side":"verter","sessionState":"cold","basisKind":"fresh"}
        """.trimIndent().replace("\n", "")
    }

    private fun memberJson(pid: Long, parentPid: Long?, image: String, role: String): String {
        val parent = parentPid?.toString() ?: "null"
        return """{"pid":$pid,"parentPid":$parent,"image":${esc(image)},"role":"$role",""" +
            """"rssBytes":{"status":"unknown","reason":"RSS not sampled in the platform test hook"}}"""
    }

    private fun esc(value: String): String {
        return "\"" + value.replace("\\", "\\\\").replace("\"", "\\\"") + "\""
    }
}
