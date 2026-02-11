#!/usr/bin/env node

/**
 * Helm Protocol — post-install binary fetcher.
 *
 * Downloads the pre-built helm binary for the current platform.
 * Falls back to building from source if no pre-built binary is available.
 */

const { execSync } = require("child_process");
const fs = require("fs");
const path = require("path");
const os = require("os");
const https = require("https");

const PACKAGE_VERSION = require("./package.json").version;
const REPO = "Helm-Protocol/Helm";
const BIN_DIR = path.join(__dirname, "bin");
const BIN_NAME = os.platform() === "win32" ? "helm.exe" : "helm";

function getPlatformKey() {
  const platform = os.platform();
  const arch = os.arch();

  const platformMap = {
    darwin: "apple-darwin",
    linux: "unknown-linux-gnu",
    win32: "pc-windows-msvc",
  };

  const archMap = {
    x64: "x86_64",
    arm64: "aarch64",
  };

  const p = platformMap[platform];
  const a = archMap[arch];

  if (!p || !a) {
    throw new Error(
      `Unsupported platform: ${platform}-${arch}. ` +
        "Please build from source: cargo install --path crates/helm-node"
    );
  }

  return `${a}-${p}`;
}

function downloadUrl() {
  const key = getPlatformKey();
  return `https://github.com/${REPO}/releases/download/v${PACKAGE_VERSION}/helm-${key}.tar.gz`;
}

function download(url) {
  return new Promise((resolve, reject) => {
    https
      .get(url, (res) => {
        if (res.statusCode === 302 || res.statusCode === 301) {
          return download(res.headers.location).then(resolve, reject);
        }
        if (res.statusCode !== 200) {
          reject(new Error(`HTTP ${res.statusCode} for ${url}`));
          return;
        }
        const chunks = [];
        res.on("data", (chunk) => chunks.push(chunk));
        res.on("end", () => resolve(Buffer.concat(chunks)));
        res.on("error", reject);
      })
      .on("error", reject);
  });
}

async function installFromRelease() {
  const url = downloadUrl();
  console.log(`Downloading helm binary from: ${url}`);

  const data = await download(url);

  if (!fs.existsSync(BIN_DIR)) {
    fs.mkdirSync(BIN_DIR, { recursive: true });
  }

  const tarPath = path.join(BIN_DIR, "helm.tar.gz");
  fs.writeFileSync(tarPath, data);

  execSync(`tar -xzf helm.tar.gz`, { cwd: BIN_DIR });
  fs.unlinkSync(tarPath);

  const binPath = path.join(BIN_DIR, BIN_NAME);
  if (fs.existsSync(binPath)) {
    fs.chmodSync(binPath, 0o755);
    console.log(`Helm binary installed: ${binPath}`);
    return true;
  }

  return false;
}

function installFromSource() {
  console.log("No pre-built binary available. Building from source...");
  console.log("This requires Rust toolchain (rustup.rs)");

  try {
    execSync("cargo --version", { stdio: "pipe" });
  } catch {
    console.error(
      "ERROR: Rust toolchain not found. Install from https://rustup.rs"
    );
    process.exit(1);
  }

  const root = path.resolve(__dirname, "..");
  execSync("cargo build --release -p helm-node", {
    cwd: root,
    stdio: "inherit",
  });

  if (!fs.existsSync(BIN_DIR)) {
    fs.mkdirSync(BIN_DIR, { recursive: true });
  }

  const srcBin = path.join(root, "target", "release", BIN_NAME);
  const destBin = path.join(BIN_DIR, BIN_NAME);
  fs.copyFileSync(srcBin, destBin);
  fs.chmodSync(destBin, 0o755);
  console.log(`Helm binary built and installed: ${destBin}`);
}

async function main() {
  console.log(`\nHelm Protocol v${PACKAGE_VERSION}`);
  console.log("The Sovereign Agent Protocol\n");

  try {
    const success = await installFromRelease();
    if (success) return;
  } catch (err) {
    console.log(`Pre-built binary not available: ${err.message}`);
  }

  installFromSource();
}

main().catch((err) => {
  console.error(`Installation failed: ${err.message}`);
  console.error(
    "Manual install: cargo install --path crates/helm-node"
  );
  process.exit(1);
});
