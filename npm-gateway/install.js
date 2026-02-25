#!/usr/bin/env node
/**
 * @helm-protocol/helm-gateway — post-install
 *
 * 🔒 PASSWORD PROTECTED — Operator-only package
 *
 * To install:
 *   HELM_GATEWAY_KEY=<key> npm install @helm-protocol/helm-gateway
 *
 * The password is verified against a stored hash.
 * The actual password is NEVER stored in this file or any git commit.
 *
 * Security design:
 *   - HELM_GATEWAY_KEY env var → SHA256 → SHA256 → compare with stored hash
 *   - Hash is computed offline, only the double-hash lives here
 *   - npm publish uses --ignore-scripts to prevent CI leaks
 */

const { execSync } = require('child_process');
const fs    = require('fs');
const path  = require('path');
const os    = require('os');
const https = require('https');
const crypto= require('crypto');
const readline = require('readline');

const PKG_VERSION = require('./package.json').version;
const BIN_DIR     = path.join(__dirname, 'bin');
const REPO        = 'Helm-Protocol/gateway';
const IS_CI       = process.env.CI || !process.stdout.isTTY;

// ─── Auth: double-hash stored (no plaintext ever in git) ──────────
// DO NOT STORE THE ACTUAL PASSWORD HERE
// Hash: sha256(sha256(password)) — computed offline
const STORED_HASH = '5a60b2ffe317287cabcaaca87cbd18eb3fd92661ec77abe016fee5c6c60cc60f';

function verifyKey(inputKey) {
  const h1 = crypto.createHash('sha256').update(inputKey).digest('hex');
  const h2 = crypto.createHash('sha256').update(h1).digest('hex');
  return h2 === STORED_HASH;
}

// ─── 플랫폼 ──────────────────────────────────────────────────────
function getPlatformKey() {
  const p = { darwin:'apple-darwin', linux:'unknown-linux-gnu' }[os.platform()];
  const a = { x64:'x86_64', arm64:'aarch64' }[os.arch()];
  if (!p || !a) throw new Error(`Unsupported: ${os.platform()}-${os.arch()}`);
  return `${a}-${p}`;
}

// ─── 다운로드 ─────────────────────────────────────────────────────
function download(url) {
  return new Promise((resolve, reject) => {
    https.get(url, res => {
      if (res.statusCode === 301 || res.statusCode === 302)
        return download(res.headers.location).then(resolve, reject);
      if (res.statusCode !== 200) return reject(new Error(`HTTP ${res.statusCode}`));
      const chunks = [];
      res.on('data', c => chunks.push(c));
      res.on('end', () => resolve(Buffer.concat(chunks)));
      res.on('error', reject);
    }).on('error', reject);
  });
}

// ─── 템플릿 생성 ─────────────────────────────────────────────────
function createTemplates(destDir) {
  // .env.gateway
  const envPath = path.join(destDir, '.env.gateway');
  if (!fs.existsSync(envPath)) {
    fs.writeFileSync(envPath, `# Helm Gateway Configuration — DO NOT COMMIT THIS FILE
# Add .env.gateway to .gitignore immediately!

GATEWAY_DID=
GATEWAY_WALLET=
DATABASE_URL=postgres://helm:password@localhost:5432/helm_gateway
BASE_RPC_URL=https://mainnet.base.org
BNKR_CONTRACT=0x22af33fe49fd1fa80c7149773dde5890d3c76f3b
QKVG_ESCROW_ADDRESS=
GATEWAY_PORT=8080
GATEWAY_PUBLIC_URL=
HELM_REGISTRY_URL=https://registry.helm-protocol.io
JWT_SECRET=change-me-$(require('crypto').randomBytes(16).toString('hex'))
REDIS_URL=redis://localhost:6379
TELEGRAM_BOT_TOKEN=
TELEGRAM_ALLOWED_ID=
`);
    console.log('  ✅ Created .env.gateway (add to .gitignore!)');
  }

  // docker-compose.gateway.yml
  const composePath = path.join(destDir, 'docker-compose.gateway.yml');
  if (!fs.existsSync(composePath)) {
    fs.writeFileSync(composePath, `version: "3.9"
services:
  helm-gateway:
    image: ghcr.io/helm-protocol/gateway:latest
    restart: always
    ports: ["8080:8080"]
    env_file: [.env.gateway]
    depends_on: [postgres, redis]
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:8080/health"]
      interval: 30s

  postgres:
    image: postgres:16-alpine
    restart: always
    environment:
      POSTGRES_DB: helm_gateway
      POSTGRES_USER: helm
      POSTGRES_PASSWORD: password
    volumes: [pgdata:/var/lib/postgresql/data]

  redis:
    image: redis:7-alpine
    restart: always

volumes:
  pgdata:
`);
    console.log('  ✅ Created docker-compose.gateway.yml');
  }

  // deploy-gcp.sh
  const gcpPath = path.join(destDir, 'deploy-gcp.sh');
  if (!fs.existsSync(gcpPath)) {
    fs.writeFileSync(gcpPath, `#!/bin/bash
# Helm Gateway — GCP Cloud Run Deploy
set -e
PROJECT=\${GCP_PROJECT_ID:-"your-project"}
REGION=\${GCP_REGION:-"us-central1"}
IMAGE="gcr.io/\${PROJECT}/helm-gateway"
docker build -t "\${IMAGE}:latest" .
docker push "\${IMAGE}:latest"
gcloud run deploy helm-gateway \\
  --image "\${IMAGE}:latest" \\
  --platform managed --region "\${REGION}" \\
  --port 8080 --min-instances 1 --max-instances 10 \\
  --memory 512Mi --allow-unauthenticated \\
  --env-vars-file .env.gateway
echo "✅ URL: \$(gcloud run services describe helm-gateway --region \${REGION} --format 'value(status.url)')"
`);
    execSync(`chmod +x ${gcpPath}`);
    console.log('  ✅ Created deploy-gcp.sh');
  }
}

// ─── 메인 ─────────────────────────────────────────────────────────
async function main() {
  console.log('\n╔════════════════════════════════════════════╗');
  console.log('║   Helm Gateway v' + PKG_VERSION.padEnd(24) + '║');
  console.log('║   Operator Package — Password Protected    ║');
  console.log('╚════════════════════════════════════════════╝\n');

  // ── 1. Password verification ──────────────────────────────────
  let key = process.env.HELM_GATEWAY_KEY || '';

  if (!key) {
    if (IS_CI) {
      console.error('❌ HELM_GATEWAY_KEY environment variable required.');
      console.error('   HELM_GATEWAY_KEY=<key> npm install @helm-protocol/helm-gateway');
      process.exit(1);
    }

    // Interactive: prompt for key
    const rl = readline.createInterface({ input: process.stdin, output: process.stdout });
    key = await new Promise(resolve => {
      // Hide input (like a password prompt)
      process.stdout.write('  🔑 Gateway access key: ');
      process.stdin.setRawMode(true);
      let input = '';
      process.stdin.on('data', function handler(ch) {
        ch = ch.toString();
        if (ch === '\n' || ch === '\r' || ch === '\u0003') {
          process.stdin.setRawMode(false);
          process.stdin.removeListener('data', handler);
          console.log('');
          resolve(input);
        } else if (ch === '\u007F') {
          input = input.slice(0, -1);
          process.stdout.clearLine(0);
          process.stdout.cursorTo(0);
          process.stdout.write('  🔑 Gateway access key: ' + '*'.repeat(input.length));
        } else {
          input += ch;
          process.stdout.write('*');
        }
      });
    });
    rl.close();
  }

  if (!verifyKey(key)) {
    console.error('\n  ❌ Invalid key. Access denied.');
    console.error('  Contact the Gateway administrator.');
    process.exit(1);
  }

  console.log('  ✅ Access granted.\n');

  // ── 2. 바이너리 다운로드 ────────────────────────────────────────
  process.stdout.write('  Downloading helm-gateway binary... ');
  try {
    const k = getPlatformKey();
    const url = `https://github.com/${REPO}/releases/download/v${PKG_VERSION}/helm-gateway-${k}.tar.gz`;
    const data = await download(url);
    if (!fs.existsSync(BIN_DIR)) fs.mkdirSync(BIN_DIR, { recursive: true });
    const tarPath = path.join(BIN_DIR, 'helm-gateway.tar.gz');
    fs.writeFileSync(tarPath, data);
    execSync('tar -xzf helm-gateway.tar.gz', { cwd: BIN_DIR });
    fs.unlinkSync(tarPath);
    const binPath = path.join(BIN_DIR, 'helm-gateway');
    if (fs.existsSync(binPath)) fs.chmodSync(binPath, 0o755);
    console.log('✅');
  } catch (e) {
    console.log(`⚠️  ${e.message}`);
    console.log('  Build from source: cargo build --release -p helm-gateway-server');
  }

  // ── 3. 템플릿 파일 생성 ──────────────────────────────────────
  console.log('\n  Setting up configuration templates...');
  createTemplates(process.cwd());

  // ── 4. Next steps ────────────────────────────────────────────
  console.log('\n╔════════════════════════════════════════════════════════════╗');
  console.log('║  🎉 helm-gateway installed successfully!                   ║');
  console.log('╚════════════════════════════════════════════════════════════╝');
  console.log('');
  console.log('  Next steps:');
  console.log('  ① helm-gateway init                # Generate DID + Registry registration');
  console.log('  ② vim .env.gateway                 # Set GATEWAY_WALLET, DATABASE_URL');
  console.log('  ③ helm-gateway start --port 8080   # Local test');
  console.log('  ④ ./deploy-gcp.sh                  # Deploy to GCP (already running ✅)');
  console.log('');
  console.log('  Revenue: 85% of all API traffic → your wallet');
  console.log('  Supported tokens: BNKR · ETH · USDC · USDT · SOL · CLANKER · VIRTUAL');
  console.log('');
  console.log('  ⚠️  Add .env.gateway to .gitignore immediately!');
  console.log('  📖 docs: https://docs.helm-protocol.io/gateway');
  console.log('');
}

main().catch(e => {
  console.error(`\n  Fatal: ${e.message}`);
  process.exit(1);
});
