/**
 * Remove every quarantined fixture dependency tree.
 *
 * The harness moves a `node_modules` it cannot prove it produced instead of
 * deleting it, and nothing removes those trees automatically — that is the point
 * of a quarantine. This is the explicit request.
 *
 * The compiled module is imported rather than the root re-derived here, so there
 * is one definition of where quarantines live. If the build is not there, the
 * path is printed by the run that made the quarantine and `rm -rf` on it is the
 * same operation.
 */
const { cleanFixtureQuarantine, fixtureQuarantineRoot } =
  await import("../../out-test/e2e/lib/fixtureDeps.js");

const root = fixtureQuarantineRoot();
const { removed, skipped } = cleanFixtureQuarantine(root);
if (removed.length === 0) {
  console.log(`No quarantined fixture dependency trees in ${root}`);
} else {
  for (const entry of removed) console.log(`Removed ${entry}`);
  console.log(`Removed ${removed.length} quarantined tree(s) from ${root}`);
}
// Reported rather than removed: this command deletes directories, so it only
// acts on the ones the harness marked as its own. Anything else in that
// directory is somebody else's, and saying so is the difference between "there
// was nothing to do" and "there was something here I would not touch".
for (const entry of skipped) console.log(`Left alone (not this harness's): ${entry}`);
