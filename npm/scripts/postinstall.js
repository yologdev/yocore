#!/usr/bin/env node
const fs = require('fs');
const path = require('path');
const os = require('os');

const platform = os.platform();
const arch = os.arch();
const key = platform + '-' + arch;
const map = { 'darwin-x64': 'darwin-x64', 'darwin-arm64': 'darwin-arm64', 'linux-x64': 'linux-x64', 'win32-x64': 'win32-x64' };
const dir = map[key];
if (!dir) { console.error('Unsupported: ' + key); process.exit(1); }

const binDir = path.join(__dirname, '..', 'bin');
const src = path.join(binDir, dir, platform === 'win32' ? 'yocore.exe' : 'yocore');
const dst = path.join(binDir, platform === 'win32' ? 'yocore.exe' : 'yocore');

try {
  fs.copyFileSync(src, dst);
  if (platform !== 'win32') fs.chmodSync(dst, 0o755);
  console.log('Yocore installed for ' + key);
} catch (e) { console.error('Install failed:', e.message); process.exit(1); }
