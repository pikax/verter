/**
 * @ai-generated - Tests for version fetching utilities.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { fetchVersions, type NightlyManifest } from "./versions";

describe("fetchVersions", () => {
  const originalFetch = globalThis.fetch;

  beforeEach(() => {
    globalThis.fetch = vi.fn();
  });

  afterEach(() => {
    globalThis.fetch = originalFetch;
  });

  function mockNpmResponse(versions: Record<string, any> = {}, time: Record<string, string> = {}) {
    return new Response(JSON.stringify({ versions, time }), { status: 200 });
  }

  function mockNightlyResponse(manifest: NightlyManifest) {
    return new Response(JSON.stringify(manifest), { status: 200 });
  }

  it("always includes 'local' as first entry", async () => {
    vi.mocked(globalThis.fetch).mockResolvedValue(new Response("", { status: 404 }));
    const versions = await fetchVersions();
    expect(versions[0]).toEqual({ id: "local", label: "This Build", type: "local" });
  });

  it("includes npm releases", async () => {
    vi.mocked(globalThis.fetch).mockImplementation(async (url) => {
      const urlStr = typeof url === "string" ? url : url.toString();
      if (urlStr.includes("registry.npmjs.org")) {
        return mockNpmResponse(
          { "0.0.1": {}, "0.0.2": {} },
          { "0.0.1": "2024-01-01T00:00:00Z", "0.0.2": "2024-06-01T00:00:00Z" },
        );
      }
      return new Response("", { status: 404 });
    });

    const versions = await fetchVersions();
    const releases = versions.filter((v) => v.type === "release");
    expect(releases).toHaveLength(2);
    expect(releases[0].version).toBe("0.0.2");
    expect(releases[1].version).toBe("0.0.1");
  });

  it("includes nightly commits", async () => {
    vi.mocked(globalThis.fetch).mockImplementation(async (url) => {
      const urlStr = typeof url === "string" ? url : url.toString();
      if (urlStr.includes("github.com")) {
        return mockNightlyResponse({
          latest: "abc1234",
          commits: [
            { sha: "abc1234567890", short: "abc1234", date: "2024-06-01", message: "fix stuff" },
          ],
        });
      }
      return new Response("", { status: 404 });
    });

    const versions = await fetchVersions();
    const commits = versions.filter((v) => v.type === "commit");
    expect(commits).toHaveLength(1);
    expect(commits[0].sha).toBe("abc1234");
    expect(commits[0].label).toContain("abc1234");
    expect(commits[0].label).toContain("fix stuff");
  });

  it("handles both fetches failing gracefully", async () => {
    vi.mocked(globalThis.fetch).mockRejectedValue(new Error("network error"));
    const versions = await fetchVersions();
    expect(versions).toHaveLength(1);
    expect(versions[0].type).toBe("local");
  });

  it("handles npm failure but nightly success", async () => {
    vi.mocked(globalThis.fetch).mockImplementation(async (url) => {
      const urlStr = typeof url === "string" ? url : url.toString();
      if (urlStr.includes("github.com")) {
        return mockNightlyResponse({
          latest: "abc1234",
          commits: [
            { sha: "abc1234567890", short: "abc1234", date: "2024-06-01", message: "test" },
          ],
        });
      }
      throw new Error("npm down");
    });

    const versions = await fetchVersions();
    expect(versions.some((v) => v.type === "commit")).toBe(true);
    expect(versions.some((v) => v.type === "release")).toBe(false);
  });

  it("truncates long commit messages", async () => {
    const longMessage = "a".repeat(50);
    vi.mocked(globalThis.fetch).mockImplementation(async (url) => {
      const urlStr = typeof url === "string" ? url : url.toString();
      if (urlStr.includes("github.com")) {
        return mockNightlyResponse({
          latest: "abc1234",
          commits: [
            { sha: "abc1234567890", short: "abc1234", date: "2024-06-01", message: longMessage },
          ],
        });
      }
      return new Response("", { status: 404 });
    });

    const versions = await fetchVersions();
    const commit = versions.find((v) => v.type === "commit");
    expect(commit?.label).toContain("...");
    expect(commit!.label.length).toBeLessThan(longMessage.length + 20);
  });

  it("sorts releases by time (newest first)", async () => {
    vi.mocked(globalThis.fetch).mockImplementation(async (url) => {
      const urlStr = typeof url === "string" ? url : url.toString();
      if (urlStr.includes("registry.npmjs.org")) {
        return mockNpmResponse(
          { "0.0.1": {}, "0.0.2": {}, "0.0.3": {} },
          {
            "0.0.1": "2024-01-01T00:00:00Z",
            "0.0.3": "2024-03-01T00:00:00Z",
            "0.0.2": "2024-02-01T00:00:00Z",
          },
        );
      }
      return new Response("", { status: 404 });
    });

    const versions = await fetchVersions();
    const releases = versions.filter((v) => v.type === "release");
    expect(releases.map((v) => v.version)).toEqual(["0.0.3", "0.0.2", "0.0.1"]);
  });

  it("handles empty npm versions object", async () => {
    vi.mocked(globalThis.fetch).mockImplementation(async (url) => {
      const urlStr = typeof url === "string" ? url : url.toString();
      if (urlStr.includes("registry.npmjs.org")) {
        return mockNpmResponse({}, {});
      }
      return new Response("", { status: 404 });
    });

    const versions = await fetchVersions();
    expect(versions.filter((v) => v.type === "release")).toHaveLength(0);
  });

  it("handles empty nightly commits array", async () => {
    vi.mocked(globalThis.fetch).mockImplementation(async (url) => {
      const urlStr = typeof url === "string" ? url : url.toString();
      if (urlStr.includes("github.com")) {
        return mockNightlyResponse({ latest: "", commits: [] });
      }
      return new Response("", { status: 404 });
    });

    const versions = await fetchVersions();
    expect(versions.filter((v) => v.type === "commit")).toHaveLength(0);
  });
});
