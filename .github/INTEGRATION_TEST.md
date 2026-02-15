# Integration Test Workflow

## Overview

The integration test workflow validates Verter's compatibility with real-world Vue projects by building and testing popular open-source Vue applications with both the standard Vue compiler and Verter.

## Test Projects

The workflow currently tests against 4 major Vue projects:

1. **Vuetify** - Material Design component framework
2. **PrimeVue** - Rich UI component library
3. **Element Plus** - Enterprise-grade component library
4. **Shadcn-vue** - Modern UI components

## How It Works

For each test project, the workflow:


1. **Baseline Testing (Vue)**
   - Clones the project repository
   - Installs dependencies
   - Runs build with standard Vue compiler
   - Runs tests (if available)
   - Records build time and test results

2. **Verter Testing**
   - Installs `@verter/native` and `@verter/unplugin`
   - Replaces `vue()` with `verter()` in Vite config
   - Runs build with Verter compiler
   - Runs tests with Verter
   - Records build time and test results

3. **Comparison & Reporting**
   - Compares build times (Vue vs Verter)
   - Compares test results
   - Generates detailed markdown reports
   - Creates aggregate summary

## Triggering the Workflow

### 1. Manual Trigger (workflow_dispatch)

Go to **Actions** → **Integration Test** → **Run workflow**

**Parameters:**
- `source`: 
  - `artifact` (default) - Build Verter from current branch
  - `npm` - Use published version from npm
- `dist-tag`: NPM distribution tag (default: `alpha`)
- `projects`: Comma-separated list or `all` (default)

**Use cases:**
- Testing PR changes before merging
- Validating compatibility after code changes
- Debugging specific project failures

### 2. After Release (workflow_call)

Automatically triggered after successful npm publish in release workflow.

**When it runs:**
- After pushing a version tag (e.g., `v0.0.1-alpha.2`)
- After npm packages are published
- Uses the published npm package for testing

**Purpose:**
- Validate released version works correctly
- Catch integration issues before users do
- Build confidence in releases

### 3. PR Comment Trigger (/integration)

**How to trigger:**
1. Comment `/integration` on any pull request
2. Must have write permission to repository
3. Workflow builds and tests PR changes

**Response:**
- 👀 reaction - Acknowledged, starting tests
- 💬 comment - Results posted when complete
- ✓ check - Pass/Warning/Fail status set on PR:
  - ✅ **Pass** - Builds and tests succeed, Verter ≥ Vue performance
  - ⚠️ **Warning** - Builds and tests succeed, but Verter is slower
  - ❌ **Fail** - Build or test failures
- ❌ reaction - No permission (if unauthorized)

**Use cases:**
- Quick validation of PR changes
- On-demand testing before review
- Comparing PR behavior against baseline

## Interpreting Results

### Status Criteria

Each test project receives one of three statuses:

**✅ Pass (Green)**
- Verter build succeeds
- Verter tests pass (or no tests defined)
- Verter build time ≤ Vue build time
- Verter test time ≤ Vue test time

**⚠️ Warning (Yellow)**
- Verter build succeeds
- Verter tests pass
- BUT: Verter is slower than Vue (build or tests or both)

**❌ Fail (Red)**
- Verter build fails, OR
- Verter tests fail

When triggered from a PR (`/integration`), these statuses are reflected in:
1. **PR comment** - Detailed results with color-coded status
2. **Check run** - Green checkmark (pass), yellow dot (warning), or red X (fail)

### Job Summary

The workflow creates a comprehensive summary showing:

```markdown
## Overview
- ✅ **vuetify** - Passed (faster or equal)
- ⚠️ **primevue** - Warning (slower than Vue)  
- ❌ **element-plus** - Failed (build or tests failed)
- ✅ **shadcn-vue** - Passed (faster or equal)

**Results:** 2 passed, 1 warnings, 1 failed (total: 4)
**Overall Status:** ⚠️ Warning
```

### Individual Project Reports

Each project gets a detailed report with:

**Build Comparison Table**
- Build times (Vue vs Verter)
- Status (success/failed)
- Performance delta (±seconds and %)

**Test Comparison**
- Test times (Vue vs Verter)
- Pass/fail status
- Test count comparisons

**Build Logs**
- Expandable logs for debugging
- Last 100 lines of output
- Separate logs for Vue and Verter builds

### Artifacts

The workflow produces several artifacts:

1. **`integration-test-summary.md`** - Aggregate report
2. **`report-{project}`** - Individual project reports
3. **`logs-{project}`** - Build and test logs
4. **`verter-unplugin`** - Built plugin (artifact mode only)

Download from: **Actions** → Workflow Run → **Artifacts** section

## Handling Failures

### Non-Blocking Mode (Current)

- Tests run in **comparison mode**
- Failures are recorded but don't fail the workflow
- Allows tracking compatibility progress
- Suitable for alpha/beta stages

**Why?**
- Verter is in early stages (alpha)
- Some incompatibilities are expected
- Focus is on tracking progress, not blocking releases

### Future: Strict Mode

When Verter matures, we can switch to strict mode:
- Test failures block releases
- Enforce 100% compatibility
- Gate for production readiness

## Adding New Test Projects

To add more projects to the test matrix:

1. Edit [.github/workflows/integration-test.yml](.github/workflows/integration-test.yml)
2. Add to the `matrix.project` array:

```yaml
- name: your-project
  repo: https://github.com/org/project.git
  branch: main
  build-cmd: npm run build
  test-cmd: npm run test
  vite-config-path: .  # or subdirectory path
  package-manager: pnpm  # or npm/yarn
```

3. Test the addition with manual trigger

**Good candidates:**
- Popular Vue libraries/frameworks
- Projects with comprehensive test suites
- Diverse use cases (SSR, SPA, component libs)
- Well-maintained with CI

## Troubleshooting

### "No vite.config found"

**Cause:** Project doesn't use Vite or config is in unexpected location

**Fix:** Update `vite-config-path` in matrix config

### "sed: command not found"

**Cause:** Running on Windows runner (should use Ubuntu)

**Fix:** Verify job runs on `ubuntu-latest`

### "Permission denied" for /integration

**Cause:** User doesn't have write access

**Fix:** Only repo collaborators with write permission can trigger

### Build failures with Verter

**Expected behavior during alpha stage**

**Actions:**
1. Review project-specific logs in artifacts
2. Identify failing compilation patterns
3. Create issues for Verter core fixes
4. Re-run tests after fixes

### Timeout issues

**Cause:** Large projects may take >60 minutes

**Fix:** Add `timeout-minutes` to matrix job:

```yaml
test-project:
  timeout-minutes: 120  # 2 hours
```

## Performance Tips

### Parallel Execution

- Matrix jobs run in parallel (4 projects simultaneously)
- Total runtime: ~30-60 minutes
- GitHub Actions free tier: 2000 minutes/month

### Caching

The workflow caches:
- Cargo dependencies
- pnpm store
- Node modules (per project)

### Selective Testing

Run specific projects:

```yaml
projects: 'vuetify,primevue'  # Only these two
```

## Monitoring & Metrics

Track over time:
- **Compatibility rate** - % of projects passing
- **Build time delta** - Performance vs Vue
- **Failure patterns** - Common issues

**Goal:** Reach 100% compatibility with competitive performance

## Future Enhancements

Potential improvements:

1. **Performance benchmarking**
   - Memory usage tracking
   - Bundle size comparison
   - Cold vs hot build times

2. **Visual diff**
   - Compare rendered output
   - Screenshot comparison
   - DOM structure validation

3. **Extended project matrix**
   - Add more projects (10-20 total)
   - Include Nuxt applications
   - Test SSR scenarios

4. **Automated issue creation**
   - Auto-file issues for new failures
   - Link to specific error patterns
   - Suggest potential fixes

5. **Regression detection**
   - Compare against previous runs
   - Alert on compatibility drops
   - Track performance trends

## Related Files

- [.github/workflows/integration-test.yml](../workflows/integration-test.yml) - Main workflow
- [.github/workflows/release.yml](../workflows/release.yml) - Release workflow (triggers integration tests)
- [.github/workflows/ci.yml](../workflows/ci.yml) - Standard CI checks

## Support

For issues with the integration test workflow:

1. Check [GitHub Actions logs](../../actions/workflows/integration-test.yml)
2. Review workflow artifacts for detailed errors
3. Open an issue with `ci` label
4. Include workflow run link and relevant logs
