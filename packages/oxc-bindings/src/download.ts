import { get } from "https";
import { writeFileSync, mkdirSync, rmSync } from "fs";
import { spawnSync } from "child_process";

const fetchJson = (url: string) =>
  new Promise<any>((res, rej) =>
    get(url, (r) => {
      let d = "";
      r.on("data", (c) => (d += c));
      r.on("end", () => res(JSON.parse(d)));
    }).on("error", rej)
  );

function hasTar(): boolean {
  return spawnSync("tar", ["--version"], { stdio: "ignore" }).status === 0;
}

export async function downloadPackage(pkg: string, toPath: string) {
  const m = await fetchJson(`https://registry.npmjs.org/${pkg}`);
  const v = m["dist-tags"].latest;
  const t = m.versions[v].dist.tarball;
  mkdirSync(toPath, { recursive: true });
  const tgz = `${toPath}.tgz`;

  await new Promise<void>((r) =>
    get(t, (rstr) => {
      const buff: Buffer[] = [];
      rstr.on("data", (c) => buff.push(c));
      rstr.on("end", () => {
        writeFileSync(tgz, Buffer.concat(buff));
        r();
      });
    })
  );

  if (process.platform === "win32" && !hasTar()) {
    spawnSync(
      "powershell",
      [
        "-NoProfile",
        "-Command",
        `gzip -d -c ${tgz} | tar -x --strip-components=1 -C ${toPath}`,
      ],
      { stdio: "inherit" }
    );
  } else {
    spawnSync("tar", ["-xzf", tgz, "--strip-components=1", "-C", toPath]);
  }

  rmSync(tgz);
  console.log(`${pkg}@${v} → ${toPath}`);
}
