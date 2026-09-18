// Negative controls for the public product-surface catalog.
//
// Discipline: assert pre-mutation state, apply the mutation, assert it
// applied, then assert validation refuses it for the intended reason.

import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import { CI_WORKFLOW } from "./closure-register.mjs";
import {
  loadCatalog,
  loadSchema,
  PACKAGE_ROOT,
  REPO_ROOT,
  lockDigest,
  validateProductSurfaceCatalogModel,
} from "./product-surface-catalog.mjs";
import { validateSchemaObject } from "./lib.mjs";

const schema = loadSchema();

function freshCatalog() {
  return loadCatalog();
}

function validate(catalog, packageRoot = PACKAGE_ROOT, options = {}) {
  return validateProductSurfaceCatalogModel(
    catalog,
    schema,
    validateSchemaObject,
    packageRoot,
    options,
  );
}

function refusedBecause(errors, needle) {
  assert.ok(errors.length > 0, "expected the mutated contract to be refused");
  assert.ok(
    errors.some((error) => error.includes(needle)),
    `expected a refusal mentioning ${JSON.stringify(needle)}, got:\n${errors.join("\n")}`,
  );
}

function mutatedWorkflow(t, find, replace) {
  const source = fs.readFileSync(path.join(REPO_ROOT, CI_WORKFLOW), "utf8");
  assert.equal(
    source.split(find).length - 1,
    1,
    `pre-state: ${JSON.stringify(find)} must occur exactly once in the workflow`,
  );
  const mutated = source.replace(find, replace);
  assert.equal(mutated.split(find).length - 1, 0, "post-state: the plant did not apply");
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "l0-workflow-"));
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }));
  const file = path.join(dir, "ci.yml");
  fs.writeFileSync(file, mutated, "utf8");
  return file;
}

test("the committed contract validates clean", () => {
  assert.deepEqual(validate(freshCatalog()), []);
});

test("a surface with a missing class is refused", () => {
  const catalog = freshCatalog();
  const surface = catalog.surface.find((row) => row.id === "vue.direct.compile.raw");
  assert.equal(surface.stability_class, "preview");
  delete surface.stability_class;
  assert.ok(!Object.hasOwn(surface, "stability_class"), "post-state: the mutation did not apply");
  refusedBecause(validate(catalog), "missing required property stability_class");
});

test("a surface with a missing command is refused", () => {
  const catalog = freshCatalog();
  const surface = catalog.surface.find((row) => row.id === "vue.direct.compile.raw");
  assert.ok(surface.command_id);
  delete surface.command_id;
  assert.ok(!Object.hasOwn(surface, "command_id"), "post-state: the mutation did not apply");
  refusedBecause(validate(catalog), "missing required property command_id");
});

test("a stable class without a limit is refused", () => {
  const catalog = freshCatalog();
  const surface = catalog.surface.find((row) => row.id === "vue.direct.compile.raw");
  assert.equal(surface.stability_class, "preview");
  surface.stability_class = "stable";
  delete surface.removing_node;
  delete surface.replacement_slo;
  const before = catalog.controlling_row.length;
  catalog.controlling_row = catalog.controlling_row.filter(
    (row) => !(row.surface_id === surface.id && row.kind === "wall"),
  );
  assert.equal(surface.stability_class, "stable");
  assert.ok(catalog.controlling_row.length < before, "post-state: the wall row was not removed");
  catalog.lock_record.digest = lockDigest(catalog);
  refusedBecause(validate(catalog), "a stable class without a wall limit is refused");
});

test("a non-stable class without a removing node is refused", () => {
  const catalog = freshCatalog();
  const surface = catalog.surface.find((row) => row.id === "vue.direct.compile.raw");
  assert.equal(surface.stability_class, "preview");
  assert.ok(surface.removing_node);
  delete surface.removing_node;
  assert.ok(!Object.hasOwn(surface, "removing_node"), "post-state: the mutation did not apply");
  refusedBecause(validate(catalog), "a non-stable class requires a removing node");
});

test("a row whose limit was edited after ratification is refused", () => {
  const catalog = freshCatalog();
  const row = catalog.controlling_row.find((entry) => entry.id === "vue.direct.compile.raw.wall");
  assert.ok(Number.isSafeInteger(row.limit));
  const before = row.limit;
  const digest = catalog.lock_record.digest;
  row.limit = before + 1;
  assert.equal(row.limit, before + 1);
  assert.equal(catalog.lock_record.digest, digest, "post-state: the digest was left in place");
  refusedBecause(validate(catalog), "a limit edited after ratification is refused");
});

test("a required advertised surface dropped from the catalog is refused", () => {
  const catalog = freshCatalog();
  const before = catalog.surface.length;
  catalog.surface = catalog.surface.filter((row) => row.id !== "svelte.css.semantics");
  catalog.controlling_row = catalog.controlling_row.filter(
    (row) => row.surface_id !== "svelte.css.semantics",
  );
  catalog.advertised_entrypoint = catalog.advertised_entrypoint.filter(
    (row) => row.surface_id !== "svelte.css.semantics",
  );
  assert.ok(catalog.surface.length < before);
  catalog.lock_record.digest = lockDigest(catalog);
  refusedBecause(validate(catalog), "required advertised surface svelte.css.semantics is absent");
});

test("a second class on one surface is refused", () => {
  const catalog = freshCatalog();
  const surface = catalog.surface.find((row) => row.id === "vue.host_lint");
  const clone = { ...surface, stability_class: "experimental-unranked" };
  catalog.surface.push(clone);
  catalog.lock_record.digest = lockDigest(catalog);
  refusedBecause(validate(catalog), "duplicate id vue.host_lint");
});

test("a locked-cell restatement that does not match the lock is refused", () => {
  const catalog = freshCatalog();
  const row = catalog.controlling_row.find((entry) => entry.id === "vue.direct.compile.raw.wall");
  assert.equal(row.source, "performance-gates");
  const before = row.limit;
  row.limit = before * 2;
  catalog.lock_record.digest = lockDigest(catalog);
  assert.notEqual(row.limit, before);
  refusedBecause(validate(catalog), "a locked cell must not change to accommodate a row");
});

test("a required command its gate profile no longer runs is refused", (t) => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "l0-profile-"));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  for (const relative of ["catalogs", "schemas", "authority", "tools"])
    fs.cpSync(path.join(PACKAGE_ROOT, relative), path.join(root, relative), { recursive: true });
  assert.deepEqual(validate(loadCatalog(root), root), [], "pre-state: the mirror validates");
  const profiles = path.join(root, "catalogs", "gate-profiles.toml");
  const command = '"node roadmap/0.1.0-tama/tools/validate-product-surface-catalog.mjs", ';
  const original = fs.readFileSync(profiles, "utf8");
  assert.equal(original.split(command).length - 1, 1, "pre-state: the profile runs it once");
  fs.writeFileSync(profiles, original.replace(command, ""), "utf8");
  assert.equal(
    fs.readFileSync(profiles, "utf8").split(command).length - 1,
    0,
    "post-state: the plant did not apply",
  );
  refusedBecause(validate(loadCatalog(root), root), "gate profile docs-domain does not run");
});

test("a required command the CI job no longer issues is refused", (t) => {
  const workflowFile = mutatedWorkflow(
    t,
    "node roadmap/0.1.0-tama/tools/validate-product-surface-catalog.mjs",
    "true",
  );
  refusedBecause(
    validate(freshCatalog(), PACKAGE_ROOT, { workflowFile }),
    "command catalog_validate: job tama-roadmap does not run",
  );
});
