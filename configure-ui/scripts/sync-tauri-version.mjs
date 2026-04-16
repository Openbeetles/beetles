import { readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const projectRoot = path.resolve(__dirname, "..");

const PACKAGE_JSON_PATH = path.join(projectRoot, "package.json");
const TAURI_CONF_PATH = path.join(projectRoot, "src-tauri", "tauri.conf.json");
const CARGO_TOML_PATH = path.join(projectRoot, "src-tauri", "Cargo.toml");

function replaceCargoVersion(cargoToml, version) {
  let inPackageSection = false;
  const lines = cargoToml.split("\n");

  const nextLines = lines.map((line) => {
    const trimmed = line.trim();
    if (trimmed.startsWith("[") && trimmed.endsWith("]")) {
      inPackageSection = trimmed === "[package]";
    }
    if (inPackageSection && /^version\s*=/.test(trimmed)) {
      return `version = "${version}"`;
    }
    return line;
  });

  return nextLines.join("\n");
}

export function syncVersionArtifacts({
  packageJsonText,
  tauriConfText,
  cargoTomlText,
}) {
  const pkg = JSON.parse(packageJsonText);
  const version = pkg.version;

  if (typeof version !== "string" || version.trim() === "") {
    throw new Error("configure-ui/package.json must contain a non-empty version");
  }

  const tauriConf = JSON.parse(tauriConfText);
  tauriConf.version = version;

  return {
    version,
    nextTauriConfText: `${JSON.stringify(tauriConf, null, 2)}\n`,
    nextCargoTomlText: replaceCargoVersion(cargoTomlText, version),
  };
}

async function main() {
  const checkOnly = process.argv.includes("--check");

  const [packageJsonText, tauriConfText, cargoTomlText] = await Promise.all([
    readFile(PACKAGE_JSON_PATH, "utf8"),
    readFile(TAURI_CONF_PATH, "utf8"),
    readFile(CARGO_TOML_PATH, "utf8"),
  ]);

  const { version, nextTauriConfText, nextCargoTomlText } = syncVersionArtifacts({
    packageJsonText,
    tauriConfText,
    cargoTomlText,
  });

  const tauriChanged = tauriConfText !== nextTauriConfText;
  const cargoChanged = cargoTomlText !== nextCargoTomlText;
  const changed = tauriChanged || cargoChanged;

  if (checkOnly) {
    if (changed) {
      throw new Error(
        `Tauri desktop version drift detected for configure-ui ${version}. Run: npm run tauri:sync-version`,
      );
    }
    return;
  }

  if (!changed) {
    return;
  }

  await Promise.all([
    tauriChanged ? writeFile(TAURI_CONF_PATH, nextTauriConfText) : Promise.resolve(),
    cargoChanged ? writeFile(CARGO_TOML_PATH, nextCargoTomlText) : Promise.resolve(),
  ]);
}

if (import.meta.url === `file://${process.argv[1]}`) {
  main().catch((error) => {
    console.error(error instanceof Error ? error.message : String(error));
    process.exitCode = 1;
  });
}
