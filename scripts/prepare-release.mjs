import { appendFileSync, readFileSync, writeFileSync } from "node:fs";

const rawVersion = (process.env.RELEASE_VERSION ?? "").trim();
const version = rawVersion.startsWith("v") ? rawVersion.slice(1) : rawVersion;

if (!/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(version)) {
  throw new Error(
    `Invalid release version "${rawVersion}". Expected vMAJOR.MINOR.PATCH or MAJOR.MINOR.PATCH.`,
  );
}

function updateJson(path, update) {
  const value = JSON.parse(readFileSync(path, "utf8"));
  update(value);
  writeFileSync(path, `${JSON.stringify(value, null, 2)}\n`);
}

function replaceExactlyOnce(path, pattern, replacement) {
  const source = readFileSync(path, "utf8");
  const matches = source.match(new RegExp(pattern.source, `${pattern.flags}g`)) ?? [];
  if (matches.length !== 1) {
    throw new Error(`Expected one version entry in ${path}, found ${matches.length}.`);
  }
  writeFileSync(path, source.replace(pattern, replacement));
}

updateJson("package.json", (pkg) => {
  pkg.version = version;
});
updateJson("package-lock.json", (lock) => {
  lock.version = version;
  lock.packages[""].version = version;
});

replaceExactlyOnce(
  "src-tauri/Cargo.toml",
  /(\[package\][\s\S]*?\nversion\s*=\s*")[^"]+("\r?\n)/,
  `$1${version}$2`,
);
replaceExactlyOnce(
  "src-tauri/Cargo.lock",
  /(\[\[package\]\]\r?\nname = "db-companion"\r?\nversion = ")[^"]+("\r?\n)/,
  `$1${version}$2`,
);

writeFileSync(
  "src-tauri/tauri.release.conf.json",
  `${JSON.stringify({ version }, null, 2)}\n`,
);

if (process.env.GITHUB_OUTPUT) {
  appendFileSync(process.env.GITHUB_OUTPUT, `version=${version}\ntag=v${version}\n`);
}

console.log(`Prepared DbCompanion v${version}`);
