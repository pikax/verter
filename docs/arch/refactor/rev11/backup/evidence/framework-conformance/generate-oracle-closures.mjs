#!/usr/bin/env node

import { readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";

const root = resolve(import.meta.dirname, "oracles");

function packageName(path, entry) {
  return entry.name ?? path.slice(path.lastIndexOf("node_modules/") + "node_modules/".length);
}

function resolveDependency(packages, parentPath, name) {
  let base = parentPath;
  while (true) {
    const candidate = base ? `${base}/node_modules/${name}` : `node_modules/${name}`;
    if (packages[candidate]) return packages[candidate].version;
    const marker = base.lastIndexOf("/node_modules/");
    if (marker < 0) break;
    base = base.slice(0, marker);
  }
  return packages[`node_modules/${name}`]?.version;
}

function dependencies(packages, path, entry, field, optionalPeer = false) {
  const source = entry[field] ?? {};
  const resolved = Object.keys(source)
    .sort()
    .map((name) => {
      const version = resolveDependency(packages, path, name);
      if (version) return `${name}@${version}`;
      if (optionalPeer && entry.peerDependenciesMeta?.[name]?.optional)
        return `${name}=OMITTED_OPTIONAL_PEER`;
      throw new Error(`${path}: unresolved ${field} dependency ${name}`);
    })
    .join(",");
  return resolved === "" ? "-" : resolved;
}

function generate(domain) {
  const lockPath = resolve(root, domain, "package-lock.json");
  const lock = JSON.parse(readFileSync(lockPath, "utf8"));
  const direct = new Set(Object.keys(lock.packages[""].dependencies ?? {}));
  const rows = [];
  for (const [path, entry] of Object.entries(lock.packages)) {
    if (path === "") continue;
    const name = packageName(path, entry);
    rows.push({
      path,
      name,
      version: entry.version,
      integrity: entry.integrity ?? "-",
      resolved: entry.resolved ?? "-",
      direct: direct.has(name) ? "yes" : "no",
      dependencies: dependencies(lock.packages, path, entry, "dependencies"),
      optional_dependencies: dependencies(lock.packages, path, entry, "optionalDependencies"),
      peer_dependencies: dependencies(lock.packages, path, entry, "peerDependencies", true),
    });
  }
  rows.sort((a, b) => a.name.localeCompare(b.name) || a.path.localeCompare(b.path));
  const columns = [
    "path",
    "name",
    "version",
    "integrity",
    "resolved",
    "direct",
    "dependencies",
    "optional_dependencies",
    "peer_dependencies",
  ];
  const text =
    [
      columns.join("\t"),
      ...rows.map((row) => columns.map((column) => row[column]).join("\t")),
    ].join("\n") + "\n";
  writeFileSync(resolve(root, domain, "closure.tsv"), text);
  return rows.length;
}

process.stdout.write(JSON.stringify({ vue: generate("vue"), svelte: generate("svelte") }) + "\n");
