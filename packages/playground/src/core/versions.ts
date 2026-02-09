/**
 * Version management for the playground.
 *
 * Fetches available versions from:
 * - npm registry (published releases, including pre-release)
 * - GitHub nightly release manifest (commit-based builds)
 */

const GITHUB_REPO = 'pikax/verter'
const NPM_PACKAGE = '@verter/wasm'

export interface VersionEntry {
  id: string
  label: string
  type: 'local' | 'release' | 'commit'
  version?: string
  sha?: string
  date?: string
}

export interface NightlyManifest {
  latest: string
  commits: Array<{
    sha: string
    short: string
    date: string
    message: string
  }>
}

/**
 * Fetch available versions from npm + nightly manifest.
 * Returns entries sorted: local build first, then releases (newest first),
 * then nightly commits (newest first).
 */
export async function fetchVersions(): Promise<VersionEntry[]> {
  const entries: VersionEntry[] = [
    { id: 'local', label: 'This Build', type: 'local' },
  ]

  // Fetch npm releases and nightly manifest in parallel
  const [releases, nightly] = await Promise.allSettled([
    fetchNpmReleases(),
    fetchNightlyManifest(),
  ])

  // Add published releases
  if (releases.status === 'fulfilled') {
    for (const version of releases.value) {
      entries.push({
        id: `release:${version}`,
        label: `v${version}`,
        type: 'release',
        version,
      })
    }
  }

  // Add nightly commits
  if (nightly.status === 'fulfilled' && nightly.value) {
    for (const commit of nightly.value.commits) {
      const shortMsg = commit.message.length > 40
        ? commit.message.slice(0, 40) + '...'
        : commit.message
      entries.push({
        id: `commit:${commit.short}`,
        label: `${commit.short} - ${shortMsg}`,
        type: 'commit',
        sha: commit.short,
        date: commit.date,
      })
    }
  }

  return entries
}

/** Fetch all published versions of @verter/wasm from npm registry */
async function fetchNpmReleases(): Promise<string[]> {
  try {
    const res = await fetch(`https://registry.npmjs.org/${NPM_PACKAGE}`, { mode: 'cors' })
    if (!res.ok) return []
    const data = await res.json()
    const versions = Object.keys(data.versions || {})
    // Sort newest first (reverse chronological by semver-ish)
    versions.sort((a, b) => {
      const ta = new Date(data.time?.[a] || 0).getTime()
      const tb = new Date(data.time?.[b] || 0).getTime()
      return tb - ta
    })
    return versions
  } catch {
    return []
  }
}

/** Fetch nightly manifest from GitHub Release */
async function fetchNightlyManifest(): Promise<NightlyManifest | null> {
  try {
    const url = `https://github.com/${GITHUB_REPO}/releases/download/nightly/nightly-manifest.json`
    const res = await fetch(url, { mode: 'cors' })
    if (!res.ok) return null
    return await res.json()
  } catch {
    return null
  }
}
