#!/usr/bin/env node
/**
 * @helm-protocol/helm-gateway — post-install
 * Helm Oracle API Gateway | Grand Cross v1.0.0
 *
 * 🔒 OPERATOR PACKAGE — Password protected
 *
 * Usage:
 *   HELM_GATEWAY_KEY=<key> npm install -g @helm-protocol/helm-gateway
 *   
 *   Or interactive:
 *   npm install -g @helm-protocol/helm-gateway
 *   → prompts for key
 *
 * Security: SHA256(SHA256(key)) verified. No plaintext stored anywhere.
 */

'use strict';

const { execSync } = require('child_process');
const fs     = require('fs');
const path   = require('path');
const os     = require('os');
const https  = require('https');
const crypto = require('crypto');
const readline = require('readline');

const PKG     = require('./package.json');
const BIN_DIR = path.join(__dirname, 'bin');
const IS_CI   = !!(process.env.CI || process.env.HELM_NO_INTERACTIVE || !process.stdout.isTTY);

// ── Auth hash (no plaintext in git) ──────────────────────────────
const STORED = '5a60b2ffe317287cabcaaca87cbd18eb3fd92661ec77abe016fee5c6c60cc60f';

function verify(k) {
  const h1 = crypto.createHash('sha256').update(k).digest('hex');
  return crypto.createHash('sha256').update(h1).digest('hex') === STORED;
}

// ── Platform ──────────────────────────────────────────────────────
function platform() {
  const p = { darwin: 'apple-darwin', linux: 'unknown-linux-gnu' }[os.platform()];
  const a = { x64: 'x86_64', arm64: 'aarch64' }[os.arch()];
  if (!p || !a) throw new Error(`Unsupported platform: ${os.platform()}-${os.arch()}`);
  return `${a}-${p}`;
}

// ── Download binary ───────────────────────────────────────────────
function download(url) {
  return new Promise((res, rej) => {
    https.get(url, r => {
      if (r.statusCode === 301 || r.statusCode === 302)
        return download(r.headers.location).then(res, rej);
      if (r.statusCode !== 200)
        return rej(new Error(`HTTP ${r.statusCode} — binary not available yet`));
      const chunks = [];
      r.on('data', c => chunks.push(c));
      r.on('end',  () => res(Buffer.concat(chunks)));
      r.on('error', rej);
    }).on('error', rej);
  });
}

// ── Create config templates ───────────────────────────────────────
function createTemplates() {
  const dest = process.cwd();

  // .env.gateway
  const envPath = path.join(dest, '.env.gateway');
  if (!fs.existsSync(envPath)) {
    const secret = crypto.randomBytes(32).toString('hex');
    fs.writeFileSync(envPath, [
      '# Helm Gateway Configuration',
      '# ⚠️  ADD .env.gateway TO .gitignore IMMEDIATELY',
      '',
      '# Identity',
      'GATEWAY_DID=',
      `JWT_SECRET=${secret}`,
      '',
      '# Wallet (receives 85% of all traffic revenue)',
      'GATEWAY_WALLET=',
      '',
      '# Database',
      'DATABASE_URL=postgres://helm:password@localhost:5432/helm_gateway',
      'REDIS_URL=redis://localhost:6379',
      '',
      '# Network',
      'GATEWAY_PORT=8080',
      'GATEWAY_PUBLIC_URL=',
      'BASE_RPC_URL=https://mainnet.base.org',
      '',
      '# API Keys (upstream providers)',
      'OPENAI_API_KEY=',
      'ANTHROPIC_API_KEY=',
      'BRAVE_API_KEY=',
      'COINGECKO_API_KEY=',
      '',
      '# Contracts',
      'BNKR_CONTRACT=0x22af33fe49fd1fa80c7149773dde5890d3c76f3b',
      'HELM_ESCROW_ADDRESS=',
      '',
      '# Registry',
      'HELM_REGISTRY_URL=https://registry.helm-protocol.io',
    ].join('\n'));
    console.log('  ✅ .env.gateway — edit before starting');
  }

  // docker-compose.gateway.yml
  const composePath = path.join(dest, 'docker-compose.gateway.yml');
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
      timeout: 10s
      retries: 3

  postgres:
    image: postgres:16-alpine
    restart: always
    environment:
      POSTGRES_DB: helm_gateway
      POSTGRES_USER: helm
      POSTGRES_PASSWORD: password
    volumes: [pgdata:/var/lib/postgresql/data]
    ports: ["5432:5432"]

  redis:
    image: redis:7-alpine
    restart: always
    ports: ["6379:6379"]

volumes:
  pgdata:
`);
    console.log('  ✅ docker-compose.gateway.yml');
  }

  // deploy-gcp.sh
  const gcpPath = path.join(dest, 'deploy-gcp.sh');
  if (!fs.existsSync(gcpPath)) {
    fs.writeFileSync(gcpPath, `#!/bin/bash
# Helm Gateway — GCP Cloud Run Deploy
set -euo pipefail

PROJECT=\${GCP_PROJECT_ID:?Set GCP_PROJECT_ID}
REGION=\${GCP_REGION:-us-central1}
IMAGE="gcr.io/\${PROJECT}/helm-gateway"

echo "Building Docker image..."
docker build -t "\${IMAGE}:latest" -f Dockerfile.gateway .
docker push "\${IMAGE}:latest"

echo "Deploying to Cloud Run..."
gcloud run deploy helm-gateway \\
  --image "\${IMAGE}:latest" \\
  --platform managed \\
  --region "\${REGION}" \\
  --port 8080 \\
  --min-instances 1 \\
  --max-instances 10 \\
  --memory 512Mi \\
  --allow-unauthenticated \\
  --env-vars-file .env.gateway

URL=\$(gcloud run services describe helm-gateway \\
  --region "\${REGION}" \\
  --format "value(status.url)")

echo ""
echo "✅ Helm Gateway deployed: \${URL}"
echo "   Update GATEWAY_PUBLIC_URL=\${URL} in .env.gateway"
echo "   Then run: helm-gateway register --url \${URL}"
`);
    execSync(`chmod +x "${gcpPath}"`);
    console.log('  ✅ deploy-gcp.sh');
  }
}

// ── Password prompt (hidden input) ───────────────────────────────
function promptKey() {
  return new Promise((resolve) => {
    if (!process.stdin.setRawMode) {
      // Non-TTY fallback
      const rl = readline.createInterface({ input: process.stdin, output: process.stdout });
      rl.question('\n  🔑 Gateway access key: ', k => { rl.close(); resolve(k.trim()); });
      return;
    }

    process.stdout.write('\n  🔑 Gateway access key: ');
    process.stdin.setRawMode(true);
    process.stdin.resume();
    let input = '';

    function handler(ch) {
      ch = ch.toString();
      if (ch === '\r' || ch === '\n' || ch === '\u0003') {
        process.stdin.setRawMode(false);
        process.stdin.pause();
        process.stdin.removeListener('data', handler);
        process.stdout.write('\n');
        resolve(input);
      } else if (ch === '\u007F') {
        if (input.length > 0) {
          input = input.slice(0, -1);
          process.stdout.clearLine(0);
          process.stdout.cursorTo(0);
          process.stdout.write('  🔑 Gateway access key: ' + '·'.repeat(input.length));
        }
      } else {
        input += ch;
        process.stdout.write('·');
      }
    }
    process.stdin.on('data', handler);
  });
}

// ── Main ──────────────────────────────────────────────────────────
async function main() {
  console.log('\n╔═══════════════════════════════════════════════╗');
  console.log(`║  ⚓ Helm Gateway v${PKG.version}                        ║`);
  console.log('║  Oracle API Gateway | Grand Cross v1.0.0     ║');
  console.log('║  Operator Package — Password Protected        ║');
  console.log('╚═══════════════════════════════════════════════╝');

  // 1. Verify access key
  let key = process.env.HELM_GATEWAY_KEY || '';

  if (!key) {
    if (IS_CI) {
      console.error('\n  ❌ Set HELM_GATEWAY_KEY environment variable first.');
      console.error('  HELM_GATEWAY_KEY=<key> npm install -g @helm-protocol/helm-gateway');
      process.exit(1);
    }
    key = await promptKey();
  }

  if (!verify(key.trim())) {
    console.error('\n  ❌ Invalid key. Access denied.');
    console.error('  Contact the gateway administrator for access.');
    process.exit(1);
  }

  console.log('\n  ✅ Access granted.\n');

  // 2. Download binary
  process.stdout.write('  Downloading helm-gateway binary... ');
  try {
    if (!fs.existsSync(BIN_DIR)) fs.mkdirSync(BIN_DIR, { recursive: true });
    const url = `https://github.com/Helm-Protocol/gateway/releases/download/v${PKG.version}/helm-gateway-${platform()}.tar.gz`;
    const buf = await download(url);
    const tarPath = path.join(BIN_DIR, 'helm-gateway.tar.gz');
    fs.writeFileSync(tarPath, buf);
    execSync('tar -xzf helm-gateway.tar.gz', { cwd: BIN_DIR });
    fs.unlinkSync(tarPath);
    const bin = path.join(BIN_DIR, 'helm-gateway');
    if (fs.existsSync(bin)) fs.chmodSync(bin, 0o755);
    console.log('✅');
  } catch (e) {
    console.log(`⚠️  ${e.message}`);
    console.log('  Binary will be available after Rust build. Continuing setup...');
  }

  // 3. Generate templates
  console.log('\n  Generating configuration templates...');
  try { createTemplates(); } catch (e) { console.log('  ⚠️ ', e.message); }

  // 4. Done
  console.log('\n╔═══════════════════════════════════════════════════════════╗');
  console.log('║  🎉 helm-gateway installed!                               ║');
  console.log('╚═══════════════════════════════════════════════════════════╝');
  console.log('');
  console.log('  Next steps:');
  console.log('  1) Edit .env.gateway          ← set GATEWAY_WALLET, DATABASE_URL');
  console.log('  2) helm-gateway init          ← generate DID + register');
  console.log('  3) helm-gateway start         ← start on port 8080');
  console.log('  4) ./deploy-gcp.sh            ← deploy to GCP (if using Cloud Run)');
  console.log('');
  console.log('  Revenue: 85% of all API traffic → your GATEWAY_WALLET');
  console.log('  Tokens:  BNKR · ETH · USDC · USDT · SOL · CLANKER · VIRTUAL');
  console.log('');
  console.log('  ⚠️  echo ".env.gateway" >> .gitignore');
  console.log('  📖 https://github.com/Helm-Protocol/gateway');
  console.log('');
}

main().catch(e => {
  console.error('\n  Install error:', e.message);
  process.exit(1);
});
